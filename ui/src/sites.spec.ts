// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { SitesList, WebStatus } from './api';
import { createSitesStore, type SitesApi } from './sites.svelte';

const shop = { name: 'shop', kind: 'linked' as const, path: 'C:\\x\\shop', upstream: null, docroot: 'C:\\x\\shop\\public', php: null, problem: null };

function fakeApi(over: Partial<SitesApi> = {}) {
  let list: SitesList = { sites: [shop], parked: [], httpPort: 8080 };
  let status: WebStatus = { httpPort: 8080, running: true, processes: [], error: null, notes: {} };
  const calls: string[] = [];
  const api: SitesApi & { calls: string[]; setList(l: SitesList): void; setStatus(s: WebStatus): void } = {
    calls,
    setList: (l) => (list = l),
    setStatus: (s) => (status = s),
    sitesList: async () => structuredClone(list),
    webStatus: async () => structuredClone(status),
    phpList: async () => ({ default: '8.4', installed: [{ minor: '8.4', version: '8.4.27', extensions: {} }, { minor: '8.3', version: '8.3.36', extensions: {} }] }),
    sitesPark: async (dir) => void calls.push(`park ${dir}`),
    sitesUnpark: async (dir) => void calls.push(`unpark ${dir}`),
    sitesLink: async (name, dir) => void calls.push(`link ${name} ${dir}`),
    sitesUnlink: async (name) => void calls.push(`unlink ${name}`),
    sitesProxy: async (name, upstream) => void calls.push(`proxy ${name} ${upstream}`),
    sitesUnproxy: async (name) => void calls.push(`unproxy ${name}`),
    sitesIsolate: async (name, minor) => void calls.push(`isolate ${name} ${minor}`),
    sitesPort: async (port) => void calls.push(`port ${port}`),
    ...over,
  };
  return api;
}

describe('sites store', () => {
  test('load brings sites, the web server state and the PHP versions together', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, running: true, processes: [], error: null, notes: { shop: 'Uses the default PHP 8.4 (8.3 was removed)' } });
    const store = createSitesStore(api);
    await store.load();
    expect(store.sites.map((s) => s.name)).toEqual(['shop']);
    expect(store.url(store.sites[0])).toBe('http://shop.test:8080');
    expect(store.note('shop')).toBe('Uses the default PHP 8.4 (8.3 was removed)');
    expect(store.phpChoices).toEqual(['8.4', '8.3']);
    expect(store.defaultPhp).toBe('8.4');
  });

  test('choosing a PHP version applies at once and rolls back when refused', async () => {
    const api = fakeApi({ sitesIsolate: async () => { throw 'There is no site shop'; } });
    const store = createSitesStore(api);
    await store.load();
    const pending = store.isolate('shop', '8.3');
    expect(store.sites[0].php).toBe('8.3');
    await pending;
    expect(store.sites[0].php).toBeNull();
    expect(store.errors.shop).toBe('There is no site shop');
  });

  test('parking a folder shows its sites after the reload', async () => {
    const api = fakeApi();
    const store = createSitesStore(api);
    await store.load();
    api.setList({ sites: [shop, { ...shop, name: 'blog', kind: 'parked', path: 'C:\\p\\blog' }], parked: ['C:\\p'], httpPort: 8080 });
    await store.park('C:\\p');
    expect(api.calls).toContain('park C:\\p');
    expect(store.sites.map((s) => s.name)).toEqual(['shop', 'blog']);
    expect(store.parked).toEqual(['C:\\p']);
  });

  test('the server line never claims there is nothing to serve when sites exist', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, running: false, processes: [], error: null, notes: {} });
    const store = createSitesStore(api);
    await store.load();
    expect(store.server).toEqual({ kind: 'stopped', text: 'Not running yet' });
    api.setList({ sites: [], parked: [], httpPort: 8080 });
    await store.load();
    expect(store.server).toEqual({ kind: 'idle', text: 'Not serving yet: park or link a folder' });
    api.setList({ sites: [shop], parked: [], httpPort: 8080 });
    api.setStatus({ httpPort: 8080, running: false, processes: [], error: 'caddy exited (exit code: 1)', notes: {} });
    await store.load();
    expect(store.server).toEqual({ kind: 'error', text: 'caddy exited (exit code: 1)' });
  });

  test('a taken port suggests the next one', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, running: false, processes: [], error: 'Port 8080 is used by another program', notes: {} });
    const store = createSitesStore(api);
    await store.load();
    expect(store.portSuggestion).toBe(8081);
    await store.setPort(8081);
    expect(api.calls).toContain('port 8081');
  });
});
