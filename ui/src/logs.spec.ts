// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { LogEntry, SiteLogs } from './api';
import { createLogsStore, type LogsApi } from './logs.svelte';

const entry = (level: string, message: string, detail = ''): LogEntry => ({
  time: '2026-11-03 10:00:00', env: 'local', level, message, detail, truncated: false,
});

const sites: SiteLogs[] = [
  { site: 'blog', files: [{ name: 'laravel.log', size: 10, modified: 1 }] },
  { site: 'shop', files: [{ name: 'laravel-2026-11-03.log', size: 20, modified: 2 }, { name: 'laravel-2026-11-02.log', size: 5, modified: 1 }] },
];

function fakeApi(over: Partial<LogsApi> = {}) {
  const calls: string[] = [];
  let entries = [entry('error', 'Division by zero', '#0 CartController.php(40)'), entry('warning', 'Slow cart'), entry('info', 'Logged in'), entry('debug', 'tick')];
  let size = 100;
  const api: LogsApi & { calls: string[]; append(e: LogEntry): void } = {
    calls,
    append(e) {
      entries = [e, ...entries];
      size += 10;
    },
    logsSites: async () => structuredClone(sites),
    logsRead: async (site, file, known) => {
      calls.push(`read ${site} ${file} ${known ?? '-'}`);
      return known === size ? { size, unchanged: true } : { size, entries: structuredClone(entries) };
    },
    logsClear: async (site, file) => {
      calls.push(`clear ${site} ${file}`);
      entries = [];
      size = 0;
    },
    ...over,
  };
  return api;
}

describe('Logs store', () => {
  test('opens the first site and its newest file', async () => {
    const api = fakeApi();
    const store = createLogsStore(api);
    await store.load();
    expect([store.site, store.file]).toEqual(['blog', 'laravel.log']);
    await store.choose('shop');
    expect([store.site, store.file]).toEqual(['shop', 'laravel-2026-11-03.log']);
    expect(api.calls.at(-1)).toBe('read shop laravel-2026-11-03.log -');
  });

  test('a level filter keeps that level and worse; search looks in message and detail', async () => {
    const store = createLogsStore(fakeApi());
    await store.load();
    store.level = 'warning';
    expect(store.entries.map((e) => e.level)).toEqual(['error', 'warning']);
    store.level = 'all';
    store.search = 'CARTCONTROLLER';
    expect(store.entries.map((e) => e.message)).toEqual(['Division by zero']);
  });

  test('the poll sends the known size and keeps the entries when nothing changed', async () => {
    const api = fakeApi();
    const store = createLogsStore(api);
    await store.load();
    await store.poll();
    expect(api.calls.at(-1)).toBe('read blog laravel.log 100');
    expect(store.entries).toHaveLength(4);
    api.append(entry('error', 'New'));
    await store.poll();
    expect(store.entries[0].message).toBe('New');
  });

  test('emptying asks first, then clears', async () => {
    const api = fakeApi();
    const store = createLogsStore(api);
    await store.load();
    store.askClear();
    expect(store.confirming).toBe(true);
    expect(api.calls.some((c) => c.startsWith('clear'))).toBe(false);
    await store.confirmClear();
    expect(api.calls).toContain('clear blog laravel.log');
    expect([store.confirming, store.entries.length]).toEqual([false, 0]);
  });
});
