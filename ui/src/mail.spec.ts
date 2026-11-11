// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { Mail, MailStatus, MailSummary } from './api';
import { createMailStore, links, type MailApi } from './mail.svelte';

const summary = (id: string, site: string | null, seen = false): MailSummary => ({
  id, site, from: 'app@example.test', to: ['ann@example.test'], subject: `Mail ${id}`, received: 1_800_000_000,
  size: 100, attachments: 0, seen,
});

const status: MailStatus = { port: 2525, listening: true, error: null, env: 'MAIL_PORT=2525' };

function fakeApi(over: Partial<MailApi> = {}) {
  let mails = [summary('3', 'shop'), summary('2', null), summary('1', 'shop', true)];
  const calls: string[] = [];
  const api: MailApi & { calls: string[] } = {
    calls,
    mailStatus: async () => ({ ...status }),
    mailList: async () => structuredClone(mails),
    mailGet: async (id) => {
      calls.push(`get ${id}`);
      mails = mails.map((m) => (m.id === id ? { ...m, seen: true } : m));
      const m = mails.find((x) => x.id === id)!;
      return { ...m, cc: [], html: '<p>Hi</p>', text: 'Hi', files: [] } satisfies Mail;
    },
    mailRaw: async () => ({ text: 'Subject: x', truncated: false }),
    mailDelete: async (id) => {
      calls.push(`delete ${id}`);
      mails = mails.filter((m) => m.id !== id);
    },
    mailClear: async (site) => {
      calls.push(`clear ${site === undefined ? 'all' : String(site)}`);
      mails = site === undefined ? [] : mails.filter((m) => m.site !== site);
    },
    mailPort: async (port) => ({ ...status, port, env: `MAIL_PORT=${port}` }),
    mailSaveAttachment: async (id, index, path) => {
      calls.push(`save ${id} ${index} ${path}`);
    },
    pickSavePath: async (name) => `C:/Downloads/${name}`,
    ...over,
  };
  return api;
}

describe('Mail store', () => {
  test('inboxes count mails and unread ones, apps first, unnamed last', async () => {
    const store = createMailStore(fakeApi());
    await store.load();
    expect(store.inboxes).toEqual([
      { site: 'shop', label: 'shop', count: 2, unread: 1 },
      { site: null, label: 'Without a name', count: 1, unread: 1 },
    ]);
    expect(store.unread).toBe(2);
    expect(store.mails.map((m) => m.id)).toEqual(['3', '2', '1']);
  });

  test('a filter shows one inbox and Clear empties only that one', async () => {
    const api = fakeApi();
    const store = createMailStore(api);
    await store.load();
    store.filter = 'shop';
    expect(store.mails.map((m) => m.id)).toEqual(['3', '1']);
    await store.clear();
    expect(api.calls).toEqual(['clear shop']);
    expect(store.mails).toEqual([]);
    store.filter = undefined;
    expect(store.mails.map((m) => m.id)).toEqual(['2']);
    store.filter = null;
    await store.clear();
    expect(api.calls).toEqual(['clear shop', 'clear null']);
  });

  test('opening marks a mail read; deleting it closes it', async () => {
    const api = fakeApi();
    const store = createMailStore(api);
    await store.load();
    await store.open('3');
    expect(store.current?.html).toBe('<p>Hi</p>');
    expect(store.unread).toBe(1);
    expect(await store.raw()).toEqual({ text: 'Subject: x', truncated: false });
    await store.remove('3');
    expect(store.current).toBe(null);
    expect(store.mails.map((m) => m.id)).toEqual(['2', '1']);
  });

  test('a mail gone from the list closes on refresh', async () => {
    const api = fakeApi();
    const store = createMailStore(api);
    await store.load();
    await store.open('2');
    await api.mailClear(undefined);
    await store.refresh();
    expect(store.current).toBe(null);
  });

  test('an error stays until the next action, not until the next refresh', async () => {
    const api = fakeApi({
      mailDelete: async () => {
        throw new Error('This mail is gone');
      },
    });
    const store = createMailStore(api);
    await store.load();
    await store.remove('3');
    expect(store.error).toBe('This mail is gone');
    await store.refresh();
    expect(store.error).toBe('This mail is gone');
    await store.open('2');
    expect(store.error).toBe(null);
  });

  test('a port error shows, and a port that works clears it', async () => {
    const store = createMailStore(
      fakeApi({
        mailStatus: async () => ({ ...status, listening: false, error: 'Port 2525 is taken by another program' }),
      }),
    );
    await store.load();
    expect(store.status?.error).toBe('Port 2525 is taken by another program');
    await store.setPort(2626);
    expect(store.status).toEqual({ ...status, port: 2626, env: 'MAIL_PORT=2626' });

    const refusing = createMailStore(
      fakeApi({
        mailPort: async () => {
          throw new Error('Port 3000 is taken');
        },
      }),
    );
    await refusing.load();
    await refusing.setPort(3000);
    expect(refusing.portError).toBe('Port 3000 is taken');
    expect(refusing.status?.port).toBe(2525);
  });

  test('an attachment is saved where the dialog says; a cancelled dialog saves nothing', async () => {
    const api = fakeApi();
    const store = createMailStore(api);
    await store.load();
    await store.open('3');
    await store.saveAttachment({ index: 0, name: 'invoice.pdf', contentType: 'application/pdf', size: 3 });
    expect(api.calls).toContain('save 3 0 C:/Downloads/invoice.pdf');
    const cancelled = createMailStore(fakeApi({ pickSavePath: async () => null }));
    await cancelled.load();
    await cancelled.open('3');
    await cancelled.saveAttachment({ index: 0, name: 'a', contentType: 'x', size: 1 });
    expect(api.calls.filter((c) => c.startsWith('save'))).toHaveLength(1);
  });
});

describe('links', () => {
  test('anchors first, then bare URLs in the text, http(s) only, each once', () => {
    const html = `<p><a href="https://shop.test/verify?x=1&amp;y=2"> Verify </a>
      <a href="javascript:alert(1)">bad</a><a href="mailto:a@b.test">mail</a>
      <a href="https://shop.test/verify?x=1&amp;y=2">again</a><a href="#top">top</a></p>`;
    const text = 'Or open https://shop.test/reset. Also (http://localhost:8000/x) and https://shop.test/verify?x=1&y=2';
    expect(links(html, text)).toEqual([
      { url: 'https://shop.test/verify?x=1&y=2', text: 'Verify' },
      { url: 'https://shop.test/reset', text: '' },
      { url: 'http://localhost:8000/x', text: '' },
    ]);
    expect(links(null, null)).toEqual([]);
  });
});

describe('mail store loading', () => {
  test('is loading until the first answer, also when it fails', async () => {
    const store = createMailStore(fakeApi());
    expect(store.loaded).toBe(false);
    await store.load();
    expect(store.loaded).toBe(true);
    const failing = createMailStore(fakeApi({ mailStatus: async () => { throw 'down'; } }));
    await failing.load().catch(() => {});
    expect(failing.loaded).toBe(true);
  });
});
