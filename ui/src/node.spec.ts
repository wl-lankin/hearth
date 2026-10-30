// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { JobStatus, NodeBuild, NodeList } from './api';
import { createNodeStore, type NodeApi } from './node.svelte';

const available: NodeBuild[] = [
  { major: 22, version: '22.21.1', lts: 'Jod', installed: false },
  { major: 21, version: '21.7.3', lts: null, installed: false },
  { major: 20, version: '20.19.5', lts: 'Iron', installed: false },
];

const job = (state: JobStatus['state'], progress = 0, error: string | null = null): JobStatus => ({
  id: 1, kind: 'node.install', key: 'node.install:20.19.5', label: 'Installing Node 20.19.5', progress,
  message: 'Downloading Node 20.19.5', state, error,
});

function fakeApi(over: Partial<NodeApi> = {}) {
  let list: NodeList = { default: '22.12.0', installed: [{ version: '22.12.0', major: 22, lts: 'Jod' }] };
  const calls: string[] = [];
  const jobs: JobStatus[] = [];
  const api: NodeApi & { calls: string[]; jobs: JobStatus[]; setList(l: NodeList): void } = {
    calls,
    jobs,
    setList(l) {
      list = l;
    },
    nodeList: async () => structuredClone(list),
    nodeAvailable: async () => structuredClone(available),
    nodeInstall: async (spec) => {
      calls.push(`install ${spec}`);
      return 1;
    },
    jobGet: async () => jobs.shift() ?? job('done', 1),
    nodeUninstall: async (version) => {
      calls.push(`uninstall ${version}`);
    },
    nodeDefault: async (version) => {
      calls.push(`default ${version}`);
    },
    ...over,
  };
  return api;
}

describe('Node store', () => {
  test('installed versions come first in their major, with a newer build offered', async () => {
    const store = createNodeStore(fakeApi());
    await store.load();
    expect(store.rows.map((r) => [r.major, r.installedVersion, r.version])).toEqual([
      [22, '22.12.0', '22.12.0'],
      [21, null, '21.7.3'],
      [20, null, '20.19.5'],
    ]);
    const r22 = store.rows[0];
    expect([r22.isDefault, r22.update, r22.lts]).toEqual([true, '22.21.1', 'Jod']);
    expect(store.rows[2].update).toBe(null);
  });

  test('an install shows its progress on the major and ends installed', async () => {
    const api = fakeApi();
    api.jobs.push(job('running', 0.4));
    const store = createNodeStore(api);
    await store.load();
    await store.install(20);
    expect(api.calls).toEqual(['install 20']);
    expect(store.rows.find((r) => r.major === 20)!.job).toEqual({ progress: 0.4, message: 'Downloading Node 20.19.5' });
    expect(store.busy).toBe(true);
    api.setList({
      default: '22.12.0',
      installed: [
        { version: '22.12.0', major: 22, lts: 'Jod' },
        { version: '20.19.5', major: 20, lts: 'Iron' },
      ],
    });
    await store.poll();
    const r20 = store.rows.find((r) => r.major === 20)!;
    expect([r20.job, r20.installedVersion]).toEqual([null, '20.19.5']);
    expect(store.busy).toBe(false);
  });

  test('a failed install shows the error on its row', async () => {
    const api = fakeApi();
    api.jobs.push(job('failed', 0, 'The download of Node 20.19.5 is damaged'));
    const store = createNodeStore(api);
    await store.load();
    await store.install(20);
    expect(store.rows.find((r) => r.major === 20)!.error).toBe('The download of Node 20.19.5 is damaged');
  });

  test('default and remove act on the installed version', async () => {
    const api = fakeApi();
    api.setList({
      default: '22.12.0',
      installed: [
        { version: '22.12.0', major: 22, lts: 'Jod' },
        { version: '20.19.5', major: 20, lts: 'Iron' },
      ],
    });
    const store = createNodeStore(api);
    await store.load();
    await store.makeDefault('20.19.5');
    expect(store.rows.find((r) => r.major === 20)!.isDefault).toBe(true);
    api.setList({ default: '22.12.0', installed: [{ version: '22.12.0', major: 22, lts: 'Jod' }] });
    await store.uninstall('20.19.5');
    expect(api.calls).toEqual(['default 20.19.5', 'uninstall 20.19.5']);
    expect(store.rows.find((r) => r.major === 20)!.installedVersion).toBe(null);
  });

  test('a running install is picked up again when the page opens', async () => {
    const api = fakeApi({ jobsList: async () => [job('running', 0.7)] });
    const store = createNodeStore(api);
    await store.load();
    expect(store.rows.find((r) => r.major === 20)!.job?.progress).toBe(0.7);
  });

  test('without the catalog the installed versions still show', async () => {
    const api = fakeApi({
      nodeAvailable: async () => {
        throw new Error('offline');
      },
    });
    const store = createNodeStore(api);
    await store.load();
    expect(store.catalogError).toBe('offline');
    expect(store.rows.map((r) => r.version)).toEqual(['22.12.0']);
  });
});
