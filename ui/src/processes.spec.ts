// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { ProcessList, ProcessOutput, SitesList } from './api';
import { createProcessesStore, type ProcessesApi } from './processes.svelte';

const row = (name: string, running: boolean) => ({ name, command: `run ${name}`, autostart: false, running, pid: running ? 1 : null, restarts: 0, lastError: null });

function fakeApi(over: Partial<ProcessesApi> = {}) {
  const calls: string[] = [];
  let list: ProcessList = { sites: [{ site: 'shop', processes: [row('vite', true), row('queue', false)] }] };
  let outputs: ProcessOutput[] = [];
  const api: ProcessesApi & { calls: string[]; setList(l: ProcessList): void; queue(...o: ProcessOutput[]): void } = {
    calls,
    setList: (l) => (list = l),
    queue: (...o) => outputs.push(...o),
    processesList: async () => structuredClone(list),
    processesOutput: async (site, name, offset) => {
      calls.push(`output ${site} ${name} ${offset ?? '-'}`);
      return outputs.shift() ?? { offset: offset ?? 0, text: '', reset: false };
    },
    processesStart: async (site, name) => void calls.push(`start ${site} ${name}`),
    processesStop: async (site, name) => void calls.push(`stop ${site} ${name}`),
    processesRestart: async (site, name) => void calls.push(`restart ${site} ${name}`),
    processesAdd: async (site, name, command, autostart) => void calls.push(`add ${site} ${name} ${command} ${autostart}`),
    processesRemove: async (site, name) => void calls.push(`remove ${site} ${name}`),
    processesSuggest: async () => [{ name: 'vite', command: 'npm run dev', autostart: false }],
    sitesList: async (): Promise<SitesList> => ({ sites: [{ name: 'shop', kind: 'linked', path: '/x/shop', upstream: null, docroot: '/x/shop/public', php: null, secured: false, debug: false, profile: false, problem: null }], parked: [], httpPort: 8080, httpsPort: 8443 }),
    ...over,
  };
  return api;
}

describe('processes store', () => {
  test('picking a process shows its output; later polls append only new text, a reset replaces it', async () => {
    const api = fakeApi();
    const store = createProcessesStore(api);
    await store.load();
    api.queue({ offset: 10, text: 'ready\n', reset: false }, { offset: 15, text: 'hmr\n', reset: false }, { offset: 4, text: 'new\n', reset: true });
    await store.select('shop', 'vite');
    expect(store.output).toBe('ready\n');
    await store.poll();
    expect(store.output).toBe('ready\nhmr\n');
    expect(api.calls).toContain('output shop vite 10');
    await store.poll();
    expect(store.output).toBe('new\n');
  });

  test('the output keeps its last 2000 lines', async () => {
    const api = fakeApi();
    const store = createProcessesStore(api);
    await store.load();
    api.queue({ offset: 1, text: Array.from({ length: 2500 }, (_, i) => `line ${i}`).join('\n') + '\n', reset: false });
    await store.select('shop', 'vite');
    const lines = store.output.split('\n').filter(Boolean);
    expect(lines.length).toBe(2000);
    expect(lines[0]).toBe('line 500');
  });

  test('start, stop and restart refresh the list; a refusal shows on that row', async () => {
    const api = fakeApi();
    const store = createProcessesStore(api);
    await store.load();
    api.setList({ sites: [{ site: 'shop', processes: [row('vite', true), row('queue', true)] }] });
    await store.act('start', 'shop', 'queue');
    expect(api.calls).toContain('start shop queue');
    expect(store.sites[0].processes[1].running).toBe(true);
    const refusing = fakeApi({ processesStop: async () => { throw 'shop has no process named queue'; } });
    const other = createProcessesStore(refusing);
    await other.load();
    await other.act('stop', 'shop', 'queue');
    expect(other.rowError('shop', 'queue')).toBe('shop has no process named queue');
  });

  test('adding takes a suggestion; a refused add keeps the form filled', async () => {
    const api = fakeApi();
    const store = createProcessesStore(api);
    await store.load();
    await store.suggest('shop');
    expect(store.suggestions.map((s) => s.name)).toEqual(['vite']);
    store.useSuggestion(store.suggestions[0]);
    expect(store.form).toEqual({ site: 'shop', name: 'vite', command: 'npm run dev', autostart: false });
    store.form.autostart = true;
    expect(await store.add()).toBe(true);
    expect(api.calls).toContain('add shop vite npm run dev true');
    expect(store.form.name).toBe('');
    const refusing = fakeApi({ processesAdd: async () => { throw 'shop already has a process named vite'; } });
    const other = createProcessesStore(refusing);
    await other.load();
    other.form.site = 'shop';
    other.form.name = 'vite';
    other.form.command = 'npm run dev';
    expect(await other.add()).toBe(false);
    expect(other.formError).toBe('shop already has a process named vite');
    expect(other.form.command).toBe('npm run dev');
  });

  test('removing the shown process clears the output', async () => {
    const api = fakeApi();
    const store = createProcessesStore(api);
    await store.load();
    api.queue({ offset: 3, text: 'hi\n', reset: false });
    await store.select('shop', 'vite');
    await store.remove('shop', 'vite');
    expect(api.calls).toContain('remove shop vite');
    expect(store.selected).toBeNull();
    expect(store.output).toBe('');
  });
});
