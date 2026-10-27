// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { ServiceRow, ServicesAvailable } from './api';
import { createServicesStore, type ServicesApi } from './services.svelte';

const redis: ServiceRow = { id: 'redis', kind: 'redis', name: 'Redis 8', line: '8', port: 6379, autostart: false, running: true, pid: 12, error: null, log: '/logs/redis.log' };
const available: ServicesAvailable = {
  builds: [
    { kind: 'mysql', line: '8.4', version: '8.4.11', name: 'MySQL' },
    { kind: 'mysql', line: '9.7', version: '9.7.4', name: 'MySQL' },
    { kind: 'redis', line: '8', version: '8.10.3', name: 'Redis' },
  ],
  unavailable: [{ kind: 'typesense', name: 'Typesense', reason: 'Not available on Windows (it needs WSL)' }],
};

function fakeApi(over: Partial<ServicesApi> = {}) {
  let rows: ServiceRow[] = [structuredClone(redis)];
  const calls: string[] = [];
  const api: ServicesApi & { calls: string[]; setRows(r: ServiceRow[]): void } = {
    calls,
    setRows: (r) => (rows = r),
    servicesList: async () => structuredClone(rows),
    servicesAvailable: async () => structuredClone(available),
    servicesSuggestPort: async () => 3307,
    servicesCreate: async (kind, line, port, name) => void calls.push(`create ${kind} ${line} ${port} ${name ?? ''}`),
    servicesStart: async (id) => void calls.push(`start ${id}`),
    servicesStop: async (id) => void calls.push(`stop ${id}`),
    servicesDelete: async (id) => void calls.push(`delete ${id}`),
    servicesAutostart: async (id, on) => void calls.push(`autostart ${id} ${on}`),
    servicesEnv: async () => 'REDIS_HOST=127.0.0.1\nREDIS_PORT=6379',
    servicesLog: async () => 'ready to accept connections',
    ...over,
  };
  return api;
}

describe('services store', () => {
  test('a new service starts with the first version and a free port', async () => {
    const api = fakeApi();
    const store = createServicesStore(api);
    await store.load();
    expect(store.kinds.map((k) => k.kind)).toEqual(['mysql', 'redis']);
    await store.begin('mysql');
    expect(store.draft).toEqual({ kind: 'mysql', line: '8.4', port: 3307, name: '' });
    expect(store.lines('mysql')).toEqual(['8.4', '9.7']);
    await store.create();
    expect(api.calls).toContain('create mysql 8.4 3307 ');
    expect(store.draft).toBeNull();
  });

  test('a taken port offers the free one it names', async () => {
    const api = fakeApi({ servicesCreate: async () => { throw 'Port 3307 is used by another program (3308 is free)'; } });
    const store = createServicesStore(api);
    await store.load();
    await store.begin('mysql');
    await store.create();
    expect(store.createError).toBe('Port 3307 is used by another program (3308 is free)');
    expect(store.freePort).toBe(3308);
    store.useFreePort();
    expect(store.draft?.port).toBe(3308);
  });

  test('a start that fails shows the error on its row', async () => {
    const api = fakeApi({ servicesStart: async () => { throw 'Redis 8 stopped right away: bind failed'; } });
    const store = createServicesStore(api);
    await store.load();
    await store.start('redis');
    expect(store.errors.redis).toBe('Redis 8 stopped right away: bind failed');
  });

  test('deleting asks first', async () => {
    const api = fakeApi();
    const store = createServicesStore(api);
    await store.load();
    store.askDelete('redis');
    expect(store.confirming).toBe('redis');
    expect(api.calls).not.toContain('delete redis');
    await store.confirmDelete();
    expect(api.calls).toContain('delete redis');
    expect(store.confirming).toBeNull();
  });

  test('autostart rolls back when refused, and .env lines are copied', async () => {
    const api = fakeApi({ servicesAutostart: async () => { throw 'No service named redis'; } });
    const store = createServicesStore(api);
    await store.load();
    await store.setAutostart('redis', true);
    expect(store.services[0].autostart).toBe(false);
    let copied = '';
    await store.copyEnv('redis', async (text) => void (copied = text));
    expect(copied).toBe('REDIS_HOST=127.0.0.1\nREDIS_PORT=6379');
    expect(store.unavailable[0].reason).toBe('Not available on Windows (it needs WSL)');
  });
});
