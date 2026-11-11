import type { Mail, MailAttachment, MailStatus, MailSummary } from './api';

export type MailApi = {
  mailStatus(): Promise<MailStatus>;
  mailList(): Promise<MailSummary[]>;
  mailGet(id: string): Promise<Mail>;
  mailRaw(id: string): Promise<{ text: string; truncated: boolean }>;
  mailDelete(id: string): Promise<void>;
  /** `undefined` clears every inbox, `null` the mails sent without a username. */
  mailClear(site: string | null | undefined): Promise<void>;
  mailPort(port: number): Promise<MailStatus>;
  mailSaveAttachment(id: string, index: number, path: string): Promise<void>;
  /** Where to save a file, from the system dialog; null when cancelled. */
  pickSavePath(name: string): Promise<string | null>;
};

/** One inbox: the mails of one app (its SMTP username), or those sent without one. */
export interface Inbox {
  site: string | null;
  label: string;
  count: number;
  unread: number;
}

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** The http(s) links of a mail: its anchors first, then bare URLs in its text; each URL once. */
export function links(html: string | null, text: string | null): { url: string; text: string }[] {
  const found: { url: string; text: string }[] = [];
  const add = (url: string, label: string) => {
    if (/^https?:\/\//i.test(url) && !found.some((l) => l.url === url)) found.push({ url, text: label });
  };
  if (html) {
    const doc = new DOMParser().parseFromString(html, 'text/html');
    for (const a of doc.querySelectorAll('a[href]')) add(a.getAttribute('href')!.trim(), (a.textContent ?? '').trim());
  }
  for (const match of (text ?? '').matchAll(/https?:\/\/[^\s<>"'()[\]]+/gi)) add(match[0].replace(/[.,;:!?]+$/, ''), '');
  return found;
}

export function createMailStore(api: MailApi) {
  let status = $state<MailStatus | null>(null);
  let all = $state<MailSummary[]>([]);
  // undefined: every inbox; null: mails without a username; a string: that app.
  let filter = $state<string | null | undefined>(undefined);
  let current = $state<Mail | null>(null);
  let portError = $state<string | null>(null);
  let error = $state<string | null>(null);

  // False until the first answer (or failure): the page shows its loader instead of its empty state.
  let loaded = $state(false);

  async function attempt(f: () => Promise<void>) {
    error = null;
    try {
      await f();
    } catch (e) {
      error = asMessage(e);
    }
  }

  async function refresh() {
    all = await api.mailList();
    if (current && !all.some((m) => m.id === current!.id)) current = null;
  }

  return {
    get status() {
      return status;
    },
    get portError() {
      return portError;
    },
    get error() {
      return error;
    },
    get current() {
      return current;
    },
    get filter() {
      return filter;
    },
    set filter(value: string | null | undefined) {
      filter = value;
    },
    get inboxes(): Inbox[] {
      const by = new Map<string | null, Inbox>();
      for (const m of all) {
        const inbox = by.get(m.site) ?? { site: m.site, label: m.site ?? 'Without a name', count: 0, unread: 0 };
        inbox.count += 1;
        if (!m.seen) inbox.unread += 1;
        by.set(m.site, inbox);
      }
      return [...by.values()].sort((a, b) =>
        a.site === null ? 1 : b.site === null ? -1 : a.label.localeCompare(b.label),
      );
    },
    get unread() {
      return all.filter((m) => !m.seen).length;
    },
    /** The mails of the chosen inbox, newest first. */
    get mails() {
      return filter === undefined ? all : all.filter((m) => m.site === filter);
    },
    get loaded() {
      return loaded;
    },
    async load() {
      try {
        await attempt(async () => {
          status = await api.mailStatus();
          await refresh();
        });
      } finally {
        loaded = true;
      }
    },
    /** The background poll: a failure shows, but an error from an action stays until the next one. */
    async refresh() {
      try {
        await refresh();
      } catch (e) {
        error = asMessage(e);
      }
    },
    async open(id: string) {
      await attempt(async () => {
        current = await api.mailGet(id);
        all = all.map((m) => (m.id === id ? { ...m, seen: true } : m));
      });
    },
    close() {
      current = null;
    },
    raw() {
      return current ? api.mailRaw(current.id) : Promise.resolve({ text: '', truncated: false });
    },
    async remove(id: string) {
      await attempt(async () => {
        await api.mailDelete(id);
        if (current?.id === id) current = null;
        await refresh();
      });
    },
    /** Empties the chosen inbox (all of them when none is chosen). */
    async clear() {
      await attempt(async () => {
        await api.mailClear(filter);
        await refresh();
      });
    },
    async setPort(port: number) {
      portError = null;
      try {
        status = await api.mailPort(port);
      } catch (e) {
        portError = asMessage(e);
      }
    },
    async saveAttachment(file: MailAttachment) {
      const id = current?.id;
      if (!id) return;
      await attempt(async () => {
        const path = await api.pickSavePath(file.name);
        if (path) await api.mailSaveAttachment(id, file.index, path);
      });
    },
    async copyEnv(write: (text: string) => Promise<void> = (t) => navigator.clipboard.writeText(t)) {
      if (status) await attempt(() => write(status!.env));
    },
  };
}

export type MailStore = ReturnType<typeof createMailStore>;
