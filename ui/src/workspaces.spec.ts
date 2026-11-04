// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { SitesList, Workspace } from './api';
import { createWorkspacesStore, type WorkspacesApi } from './workspaces.svelte';

const ws = (site: string, parentSite: string, over: Partial<Workspace> = {}): Workspace => ({
  kind: 'worktree', name: site.split('.')[0], site, url: `http://${site}.test:8080`, path: `/h/data/workspaces/${parentSite}/${site.split('.')[0]}`,
  appDir: '', parentSite, parentPath: `/x/${parentSite}`, branch: site.split('.')[0], baseBranch: 'main', baseCommit: 'abc',
  secured: false, phpVersion: null, databaseMode: 'shared', databaseName: null, databaseDriver: null, createdBy: 'app', createdAt: 1, owned: true,
  ...over,
});

const site = (name: string, kind: 'linked' | 'parked' | 'proxy' = 'linked') => ({ name, kind, path: `/x/${name}`, upstream: null, docroot: `/x/${name}/public`, php: null, secured: false, debug: false, profile: false, problem: null });

function fakeApi(over: Partial<WorkspacesApi> = {}) {
  const calls: string[] = [];
  let list: Workspace[] = [ws('feature-x.shop', 'shop'), ws('hotfix.blog', 'blog'), ws('ui.shop', 'shop')];
  const api: WorkspacesApi & { calls: string[]; setList(l: Workspace[]): void } = {
    calls,
    setList: (l) => (list = l),
    workspacesList: async () => ({ workspaces: structuredClone(list) }),
    workspacesCreate: async (site, name, database, secure) => {
      calls.push(`create ${site} ${name} ${database} ${secure}`);
      return { workspace: ws(`${name}.${site}`, site), notes: ['composer.lock differs from the parent\'s: run `composer install` in the workspace'] };
    },
    workspacesDelete: async (name, keepDatabase, keepFiles) => {
      calls.push(`delete ${name} ${keepDatabase} ${keepFiles}`);
      return { notes: [] };
    },
    workspacesMerge: async (name, push, del) => {
      calls.push(`merge ${name} ${push} ${del}`);
      return { message: `Merged ${name} into main of shop`, notes: [] };
    },
    sitesList: async (): Promise<SitesList> => ({ sites: [site('shop'), site('blog', 'parked'), site('vite', 'proxy'), site('feature-x.shop')], parked: [], httpPort: 8080, httpsPort: 8443 }),
    openUrl: async (url) => void calls.push(`open ${url}`),
    ...over,
  };
  return api;
}

describe('workspaces store', () => {
  test('workspaces are grouped by their parent site; proxies cannot have any', async () => {
    const store = createWorkspacesStore(fakeApi());
    await store.load();
    expect(store.groups.map((g) => [g.site, g.workspaces.map((w) => w.site)])).toEqual([
      ['blog', ['hotfix.blog']],
      ['shop', ['feature-x.shop', 'ui.shop']],
    ]);
    // No proxies, no workspaces.
    expect(store.parents).toEqual(['shop', 'blog']);
  });

  test('a row action while a workspace is being made keeps the form busy', async () => {
    let finish: () => void = () => {};
    const api = fakeApi({
      workspacesCreate: (site, name) => new Promise((resolve) => (finish = () => resolve({ workspace: ws(`${name}.${site}`, site), notes: [] }))),
    });
    const store = createWorkspacesStore(api);
    await store.load();
    store.form.site = 'shop';
    store.form.name = 'slow';
    const making = store.create();
    await store.merge('feature-x.shop');
    expect(store.creating).toBe(true);
    finish();
    await making;
    expect(store.creating).toBe(false);
  });

  test('a new workspace sends site, branch and database and shows what to do next', async () => {
    const api = fakeApi();
    const store = createWorkspacesStore(api);
    await store.load();
    store.form.site = 'shop';
    store.form.name = 'feature-y';
    store.form.database = 'clone';
    expect(await store.create()).toBe(true);
    expect(api.calls).toContain('create shop feature-y clone undefined');
    expect(store.notes).toEqual(['composer.lock differs from the parent\'s: run `composer install` in the workspace']);
    expect(store.form.name).toBe('');
    const refusing = fakeApi({ workspacesCreate: async () => { throw 'Workspaces need a git repository. Run `git init` and commit first.'; } });
    const other = createWorkspacesStore(refusing);
    await other.load();
    other.form.site = 'blog';
    other.form.name = 'x';
    expect(await other.create()).toBe(false);
    expect(other.formError).toBe('Workspaces need a git repository. Run `git init` and commit first.');
    expect(other.form.name).toBe('x');
  });

  test('delete asks first and passes what to keep', async () => {
    const api = fakeApi();
    const store = createWorkspacesStore(api);
    await store.load();
    store.askDelete('feature-x.shop');
    expect(store.deleting).toBe('feature-x.shop');
    expect(api.calls).toEqual([]);
    store.cancelDelete();
    expect(store.deleting).toBeNull();
    store.askDelete('feature-x.shop');
    await store.confirmDelete({ keepDatabase: true, keepFiles: false });
    expect(api.calls).toContain('delete feature-x.shop true false');
    expect(store.deleting).toBeNull();
  });

  test('a merge shows its message, or its error on the row', async () => {
    const api = fakeApi();
    const store = createWorkspacesStore(api);
    await store.load();
    await store.merge('feature-x.shop');
    expect(store.message).toBe('Merged feature-x.shop into main of shop');
    const refusing = fakeApi({ workspacesMerge: async () => { throw 'The workspace feature-x has uncommitted changes: commit them first'; } });
    const other = createWorkspacesStore(refusing);
    await other.load();
    await other.merge('feature-x.shop');
    expect(other.rowError('feature-x.shop')).toBe('The workspace feature-x has uncommitted changes: commit them first');
  });
});
