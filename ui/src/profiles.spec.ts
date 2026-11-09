// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { PhpList, ProfileReport, SitesList } from './api';
import { createProfilesStore, type ProfilesApi } from './profiles.svelte';

const site = (name: string, secured = false) => ({ name, kind: 'linked' as const, path: `/x/${name}`, upstream: null, docroot: `/x/${name}/public`, php: null, secured, debug: false, profile: false, problem: null });
const report = (key: string, host: string, over: Partial<ProfileReport> = {}): ProfileReport => ({
  key, time: 1_700_000_000, host, method: 'GET', uri: '/', cli: false, wallMs: 42, peakMemory: 2_097_152, functions: 310, ...over,
});

function fakeApi(over: Partial<ProfilesApi> = {}) {
  const calls: string[] = [];
  let reports = [report('spx-full-2', 'shop.test'), report('spx-full-1', 'admin.test')];
  const list: SitesList = { sites: [site('shop'), site('admin', true)], parked: [], httpPort: 8080, httpsPort: 8443 };
  const api: ProfilesApi & { calls: string[] } = {
    calls,
    profileStatus: async () => ({ available: true, key: 'k' }),
    profileList: async () => ({ reports: structuredClone(reports) }),
    profileClear: async () => {
      calls.push('clear');
      reports = [];
      return { deleted: 2 };
    },
    sitesList: async () => structuredClone(list),
    phpList: async (): Promise<PhpList> => ({ default: '8.4', installed: [{ minor: '8.4', version: '8.4.27', extensions: { spx: true } }, { minor: '8.3', version: '8.3.36', extensions: { spx: false } }] }),
    openUrl: async (url) => void calls.push(`open ${url}`),
    ...over,
  };
  return api;
}

describe('profiles store', () => {
  test('a report opens only through a site whose PHP has SPX on; a command picks such a site', async () => {
    const list: SitesList = { sites: [{ ...site('legacy'), php: '8.3' }, site('shop')], parked: [], httpPort: 8080, httpsPort: 8443 };
    const cli = report('spx-full-cli', '', { cli: true, uri: 'php artisan migrate' });
    const store = createProfilesStore(fakeApi({ sitesList: async () => structuredClone(list), profileList: async () => ({ reports: [report('spx-full-l', 'legacy.test'), cli] }) }));
    await store.load();
    expect(store.reportUrl(store.reports[0])).toBeNull();
    expect(store.reportUrl(store.reports[1])).toBe('http://shop.test:8080/?SPX_KEY=k&SPX_UI_URI=/report.html&key=spx-full-cli');
  });

  test('a report opens in SPX\'s own UI through its site, over HTTPS for a secured one', async () => {
    const api = fakeApi();
    const store = createProfilesStore(api);
    await store.load();
    expect(store.reports.map((r) => r.key)).toEqual(['spx-full-2', 'spx-full-1']);
    expect(store.reportUrl(store.reports[0])).toBe('http://shop.test:8080/?SPX_KEY=k&SPX_UI_URI=/report.html&key=spx-full-2');
    expect(store.reportUrl(store.reports[1])).toBe('https://admin.test:8443/?SPX_KEY=k&SPX_UI_URI=/report.html&key=spx-full-1');
    await store.open(store.reports[0]);
    expect(api.calls).toContain('open http://shop.test:8080/?SPX_KEY=k&SPX_UI_URI=/report.html&key=spx-full-2');
  });

  test('a command-line report opens through the first site; with no site there is no link', async () => {
    const cli = report('spx-full-cli', '', { cli: true, uri: 'php artisan migrate' });
    const store = createProfilesStore(fakeApi({ profileList: async () => ({ reports: [cli] }) }));
    await store.load();
    expect(store.reportUrl(store.reports[0])).toBe('http://shop.test:8080/?SPX_KEY=k&SPX_UI_URI=/report.html&key=spx-full-cli');
    const lonely = createProfilesStore(fakeApi({ profileList: async () => ({ reports: [cli] }), sitesList: async () => ({ sites: [], parked: [], httpPort: 8080, httpsPort: 8443 }) }));
    await lonely.load();
    expect(lonely.reportUrl(lonely.reports[0])).toBeNull();
  });

  test('clear empties the list', async () => {
    const api = fakeApi();
    const store = createProfilesStore(api);
    await store.load();
    await store.clear();
    expect(api.calls).toContain('clear');
    expect(store.reports).toEqual([]);
  });

  test('without SPX the page explains why and lists nothing', async () => {
    let listed = false;
    const store = createProfilesStore(
      fakeApi({
        profileStatus: async () => ({ available: false, key: 'k' }),
        profileList: async () => {
          listed = true;
          return { reports: [] };
        },
      }),
    );
    await store.load();
    expect(store.available).toBe(false);
    expect(store.unavailable).toBe("Profiling needs SPX, which Stoke's macOS PHP builds include. Windows builds have no SPX.");
    expect(listed).toBe(false);
  });

  test('a failed load shows its reason', async () => {
    const store = createProfilesStore(fakeApi({ profileList: async () => { throw 'disk gone'; } }));
    await store.load();
    expect(store.error).toBe('disk gone');
  });
});
