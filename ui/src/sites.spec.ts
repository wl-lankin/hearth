// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { SitesList, WebStatus } from './api';
import { createSitesStore, type SitesApi } from './sites.svelte';

const shop = { name: 'shop', kind: 'linked' as const, path: 'C:\\x\\shop', upstream: null, docroot: 'C:\\x\\shop\\public', php: null, secured: false, problem: null };

function fakeApi(over: Partial<SitesApi> = {}) {
  let list: SitesList = { sites: [shop], parked: [], httpPort: 8080, httpsPort: 8443 };
  let status: WebStatus = { httpPort: 8080, httpsPort: 8443, caTrusted: true, dns: { ready: true, problem: null }, running: true, processes: [], error: null, notes: {} };
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
    sitesSecure: async (name) => void calls.push(`secure ${name}`),
    sitesUnsecure: async (name) => void calls.push(`unsecure ${name}`),
    tlsTrust: async () => void calls.push('trust'),
    dnsSetup: async () => void calls.push('dns'),
    ...over,
  };
  return api;
}

describe('sites store', () => {
  test('load brings sites, the web server state and the PHP versions together', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, httpsPort: 8443, caTrusted: true, dns: { ready: true, problem: null }, running: true, processes: [], error: null, notes: { shop: 'Uses the default PHP 8.4 (8.3 was removed)' } });
    const store = createSitesStore(api);
    await store.load();
    expect(store.sites.map((s) => s.name)).toEqual(['shop']);
    expect(store.url(store.sites[0])).toBe('http://shop.test:8080');
    expect(store.note('shop')).toBe('Uses the default PHP 8.4 (8.3 was removed)');
    expect(store.phpChoices).toEqual(['8.4', '8.3']);
    expect(store.defaultPhp).toBe('8.4');
  });

  test('a site with a .nvmrc shows the Node it runs, or why it cannot', async () => {
    const blog = { ...shop, name: 'blog', path: 'C:/x/blog' };
    const plain = { ...shop, name: 'plain', path: 'C:/x/plain' };
    const api = fakeApi({
      nodeWhich: async (dir) =>
        dir === shop.path
          ? { version: '20.19.5', file: 'C:/x/shop/.nvmrc', error: null }
          : dir === blog.path
            ? { version: null, file: 'C:/x/blog/.nvmrc', error: 'This project wants Node 18 (C:/x/blog/.nvmrc), which is not installed' }
            : { version: '22.21.1', file: null, error: null },
    });
    api.setList({ sites: [shop, blog, plain], parked: [], httpPort: 8080, httpsPort: 8443 });
    const store = createSitesStore(api);
    await store.load();
    expect(store.node('shop')).toEqual({ text: 'Node 20.19.5', problem: null });
    expect(store.node('blog')).toEqual({ text: 'Node not installed', problem: 'This project wants Node 18 (C:/x/blog/.nvmrc), which is not installed' });
    expect(store.node('plain')).toBe(null);
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
    api.setList({ sites: [shop, { ...shop, name: 'blog', kind: 'parked', path: 'C:\\p\\blog' }], parked: ['C:\\p'], httpPort: 8080, httpsPort: 8443 });
    await store.park('C:\\p');
    expect(api.calls).toContain('park C:\\p');
    expect(store.sites.map((s) => s.name)).toEqual(['shop', 'blog']);
    expect(store.parked).toEqual(['C:\\p']);
  });

  test('the server line never claims there is nothing to serve when sites exist', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, httpsPort: 8443, caTrusted: true, dns: { ready: true, problem: null }, running: false, processes: [], error: null, notes: {} });
    const store = createSitesStore(api);
    await store.load();
    expect(store.server).toEqual({ kind: 'stopped', text: 'Not running yet' });
    api.setList({ sites: [], parked: [], httpPort: 8080, httpsPort: 8443 });
    await store.load();
    expect(store.server).toEqual({ kind: 'idle', text: 'Not serving yet: park or link a folder' });
    api.setList({ sites: [shop], parked: [], httpPort: 8080, httpsPort: 8443 });
    api.setStatus({ httpPort: 8080, httpsPort: 8443, caTrusted: true, dns: { ready: true, problem: null }, running: false, processes: [], error: 'caddy exited (exit code: 1)', notes: {} });
    await store.load();
    expect(store.server).toEqual({ kind: 'error', text: 'caddy exited (exit code: 1)' });
  });

  test('a taken port suggests the next one', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, httpsPort: 8443, caTrusted: true, dns: { ready: true, problem: null }, running: false, processes: [], error: 'Port 8080 is used by another program', notes: {} });
    const store = createSitesStore(api);
    await store.load();
    expect(store.portSuggestion).toBe(8081);
    await store.setPort(8081);
    expect(api.calls).toContain('port 8081');
  });

  test('a secured site links to HTTPS, and a refused toggle rolls back', async () => {
    const api = fakeApi();
    const store = createSitesStore(api);
    await store.load();
    api.setList({ sites: [{ ...shop, secured: true }], parked: [], httpPort: 8080, httpsPort: 8443 });
    await store.secure('shop', true);
    expect(api.calls).toContain('secure shop');
    expect(store.url(store.sites[0])).toBe('https://shop.test:8443');
    const refusing = fakeApi({ sitesUnsecure: async () => { throw 'No site named shop.test'; } });
    refusing.setList({ sites: [{ ...shop, secured: true }], parked: [], httpPort: 8080, httpsPort: 8443 });
    const other = createSitesStore(refusing);
    await other.load();
    const pending = other.secure('shop', false);
    expect(other.sites[0].secured).toBe(false);
    await pending;
    expect(other.sites[0].secured).toBe(true);
    expect(other.errors.shop).toBe('No site named shop.test');
  });

  test('setup asks for .test names first, then for trust once a site is secured', async () => {
    const api = fakeApi();
    const notTrusted = { httpPort: 8080, httpsPort: 8443, caTrusted: false, running: true, processes: [], error: null, notes: {} };
    api.setStatus({ ...notTrusted, dns: { ready: false, problem: 'The .test helper is not installed' } });
    api.setList({ sites: [{ ...shop, secured: true }], parked: [], httpPort: 8080, httpsPort: 8443 });
    const store = createSitesStore(api);
    await store.load();
    expect(store.setup).toBe('dns');
    api.setStatus({ ...notTrusted, dns: { ready: true, problem: null } });
    await store.setupDns();
    expect(api.calls).toContain('dns');
    expect(store.setup).toBe('trust');
    api.setStatus({ ...notTrusted, caTrusted: true, dns: { ready: true, problem: null } });
    await store.trust();
    expect(api.calls).toContain('trust');
    expect(store.setup).toBeNull();
  });

  test('no setup card without a way to manage names or before anything is secured', async () => {
    const api = fakeApi();
    api.setStatus({ httpPort: 8080, httpsPort: 8443, caTrusted: false, dns: { ready: false, problem: null }, running: true, processes: [], error: null, notes: {} });
    const store = createSitesStore(api);
    await store.load();
    expect(store.setup).toBeNull();
  });
});
