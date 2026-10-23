// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { AvailableBuild, JobStatus, PhpList } from './api';
import { createPhpStore, type PhpApi } from './php.svelte';

const available: AvailableBuild[] = [
  { minor: '8.6', version: '8.6.0RC4', channel: 'rc', installedVersion: null, updateAvailable: false },
  { minor: '8.5', version: '8.5.12', channel: 'stable', installedVersion: null, updateAvailable: false },
  { minor: '8.4', version: '8.4.28', channel: 'stable', installedVersion: '8.4.27', updateAvailable: true },
  { minor: '8.3', version: '8.3.36', channel: 'stable', installedVersion: '8.3.36', updateAvailable: false },
];

const job = (state: JobStatus['state'], progress = 0, error: string | null = null): JobStatus => ({
  id: 1, kind: 'php.install', key: 'php.install:8.5', label: 'Installing PHP 8.5', progress,
  message: 'Downloading PHP 8.5.12', state, error,
});

function fakeApi(over: Partial<PhpApi> = {}) {
  let list: PhpList = {
    default: '8.4',
    installed: [
      { minor: '8.4', version: '8.4.27', extensions: { redis: true, xdebug: false } },
      { minor: '8.3', version: '8.3.36', extensions: { redis: true, xdebug: false } },
    ],
  };
  const calls: string[] = [];
  const jobs: JobStatus[] = [];
  const api: PhpApi & { calls: string[]; jobs: JobStatus[]; setList(l: PhpList): void } = {
    calls,
    jobs,
    setList(l) {
      list = l;
    },
    phpList: async () => structuredClone(list),
    phpAvailable: async () => structuredClone(available),
    phpInstall: async (minor) => {
      calls.push(`install ${minor}`);
      return 1;
    },
    jobGet: async () => jobs.shift() ?? job('done', 1),
    phpUninstall: async (minor) => {
      calls.push(`uninstall ${minor}`);
      return '8.3';
    },
    phpUse: async (minor) => {
      calls.push(`use ${minor}`);
    },
    phpExtension: async (minor, name, on) => {
      calls.push(`ext ${minor} ${name} ${on}`);
    },
    ...over,
  };
  return api;
}

describe('PHP store', () => {
  test('rows are newest first with previews last', async () => {
    const store = createPhpStore(fakeApi());
    await store.load();
    expect(store.rows.map((r) => r.minor)).toEqual(['8.5', '8.4', '8.3', '8.6']);
    const r84 = store.rows.find((r) => r.minor === '8.4')!;
    expect([r84.installedVersion, r84.isDefault, r84.updateAvailable, r84.version]).toEqual(['8.4.27', true, true, '8.4.28']);
  });

  test('an install shows its progress and ends installed', async () => {
    const api = fakeApi();
    api.jobs.push(job('running', 0.45));
    const store = createPhpStore(api);
    await store.load();
    await store.install('8.5');
    expect(store.rows.find((r) => r.minor === '8.5')!.job).toEqual({ progress: 0.45, message: 'Downloading PHP 8.5.12' });
    api.setList({ default: '8.4', installed: [{ minor: '8.5', version: '8.5.12', extensions: { redis: true, xdebug: false } }] });
    await store.poll();
    const r85 = store.rows.find((r) => r.minor === '8.5')!;
    expect([r85.job, r85.installedVersion]).toEqual([null, '8.5.12']);
    expect(store.busy).toBe(false);
  });

  test('a failed install shows the error on its row until the next try', async () => {
    const api = fakeApi();
    api.jobs.push(job('running', 0.1), job('failed', 0.1, 'The download of PHP 8.5.12 is damaged; try again'));
    const store = createPhpStore(api);
    await store.load();
    await store.install('8.5');
    await store.poll();
    expect(store.rows.find((r) => r.minor === '8.5')!.error).toBe('The download of PHP 8.5.12 is damaged; try again');
    await store.install('8.5');
    expect(store.rows.find((r) => r.minor === '8.5')!.error).toBeNull();
  });

  test('removing the default moves the badge', async () => {
    const api = fakeApi();
    const store = createPhpStore(api);
    await store.load();
    api.setList({ default: '8.3', installed: [{ minor: '8.3', version: '8.3.36', extensions: { redis: true, xdebug: false } }] });
    await store.uninstall('8.4');
    expect(api.calls).toContain('uninstall 8.4');
    expect(store.rows.find((r) => r.minor === '8.3')!.isDefault).toBe(true);
    expect(store.rows.find((r) => r.minor === '8.4')!.installedVersion).toBeNull();
  });

  test('an extension switch applies at once and rolls back when refused', async () => {
    const api = fakeApi({ phpExtension: async () => { throw 'PHP 8.4 has no xdebug extension'; } });
    const store = createPhpStore(api);
    await store.load();
    const pending = store.toggle('8.4', 'xdebug');
    expect(store.rows.find((r) => r.minor === '8.4')!.extensions.xdebug).toBe(true);
    await pending;
    const r84 = store.rows.find((r) => r.minor === '8.4')!;
    expect([r84.extensions.xdebug, r84.error]).toEqual([false, 'PHP 8.4 has no xdebug extension']);
  });

  test('without a catalog the installed versions still show', async () => {
    const store = createPhpStore(fakeApi({ phpAvailable: async () => { throw 'Could not download the catalog'; } }));
    await store.load();
    expect(store.catalogError).toBe('Could not download the catalog');
    expect(store.rows.map((r) => r.minor)).toEqual(['8.4', '8.3']);
  });
});
