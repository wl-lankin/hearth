// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { Dump, DumpsStatus } from './api';
import { createDumpsStore, fold, shortPath, type DumpsApi } from './dumps.svelte';

const dump = (id: number, site: string | null, text = `dump ${id}`): Dump => ({
  id, time: 1_800_000_000_000 + id, site, file: '/srv/shop/app/X.php', line: id, sapi: 'fpm-fcgi', uri: '/cart', text, truncated: false,
});

const status: DumpsStatus = { port: 2306, listening: true, error: null, enabled: true };

function fakeApi(over: Partial<DumpsApi> = {}) {
  let dumps = [dump(3, 'shop.test'), dump(2, null), dump(1, 'shop.test')];
  const calls: string[] = [];
  const api: DumpsApi & { calls: string[]; push(d: Dump): void } = {
    calls,
    push(d) {
      dumps = [d, ...dumps];
    },
    dumpsStatus: async () => ({ ...status }),
    dumpsList: async (after) => structuredClone(dumps.filter((d) => after === undefined || d.id > after)),
    dumpsClear: async () => {
      calls.push('clear');
      dumps = [];
    },
    dumpsEnable: async (on) => {
      calls.push(`enable ${on}`);
      return { ...status, enabled: on };
    },
    dumpsPort: async (port) => ({ ...status, port }),
    ...over,
  };
  return api;
}

describe('Dumps store', () => {
  test('sites with counts, a filter, and new dumps on refresh', async () => {
    const api = fakeApi();
    const store = createDumpsStore(api);
    await store.load();
    expect(store.sites).toEqual([
      { site: 'shop.test', label: 'shop.test', count: 2 },
      { site: null, label: 'Unknown', count: 1 },
    ]);
    store.filter = 'shop.test';
    expect(store.dumps.map((d) => d.id)).toEqual([3, 1]);
    api.push(dump(4, 'shop.test'));
    await store.refresh();
    expect(store.dumps.map((d) => d.id)).toEqual([4, 3, 1]);
  });

  test('the poll asks only for dumps newer than the newest it has', async () => {
    const asked: (number | undefined)[] = [];
    const api = fakeApi();
    const list = api.dumpsList;
    api.dumpsList = async (after) => {
      asked.push(after);
      const all = await list();
      return after === undefined ? all : all.filter((d) => d.id > after);
    };
    const store = createDumpsStore(api);
    await store.load();
    api.push(dump(4, null));
    await store.refresh();
    await store.refresh();
    expect(asked).toEqual([undefined, 3, 4]);
    expect(store.dumps.map((d) => d.id)).toEqual([4, 3, 2, 1]);
  });

  test('clear empties the list', async () => {
    const api = fakeApi();
    const store = createDumpsStore(api);
    await store.load();
    await store.clear();
    expect(api.calls).toEqual(['clear']);
    expect(store.dumps).toEqual([]);
  });

  test('the switch calls dumps.enable and shows the new state', async () => {
    const api = fakeApi();
    const store = createDumpsStore(api);
    await store.load();
    await store.setEnabled(false);
    expect(api.calls).toEqual(['enable false']);
    expect(store.status?.enabled).toBe(false);
  });

  test('a port error shows and the status stays', async () => {
    const store = createDumpsStore(
      fakeApi({
        dumpsPort: async () => {
          throw new Error('Port 2400 is taken by another program');
        },
      }),
    );
    await store.load();
    await store.setPort(2400);
    expect(store.portError).toBe('Port 2400 is taken by another program');
    expect(store.status?.port).toBe(2306);
  });
});

describe('fold and shortPath', () => {
  test('long text folds after 30 lines', () => {
    const long = Array.from({ length: 40 }, (_, i) => `line ${i}`).join('\n');
    expect(fold(long)).toEqual({ text: long.split('\n').slice(0, 30).join('\n'), hidden: 10 });
    expect(fold('a\nb')).toEqual({ text: 'a\nb', hidden: 0 });
  });

  test('paths are cut to the part inside the project', () => {
    expect(shortPath('/Users/ann/Sites/shop/app/Http/Controllers/CartController.php')).toBe('app/Http/Controllers/CartController.php');
    expect(shortPath(String.raw`C:\Projects\shop\routes\web.php`)).toBe('routes/web.php');
    expect(shortPath('/srv/shop/vendor/laravel/framework/src/X.php')).toBe('vendor/laravel/framework/src/X.php');
    expect(shortPath('/tmp/script.php')).toBe('/tmp/script.php');
    expect(shortPath(null)).toBe('');
  });
});
