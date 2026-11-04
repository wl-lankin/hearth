import type { SitesList, Workspace } from './api';

export type WorkspacesApi = {
  workspacesList(): Promise<{ workspaces: Workspace[] }>;
  workspacesCreate(site: string, name: string, database: string, secure?: boolean): Promise<{ workspace: Workspace; notes: string[] }>;
  workspacesDelete(name: string, keepDatabase: boolean, keepFiles: boolean): Promise<{ notes: string[] }>;
  workspacesMerge(name: string, push: boolean, del: boolean): Promise<{ message: string; notes: string[] }>;
  sitesList(): Promise<SitesList>;
  openUrl(url: string): Promise<void>;
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export function createWorkspacesStore(api: WorkspacesApi) {
  let workspaces = $state<Workspace[]>([]);
  let parents = $state<string[]>([]);
  let error = $state<string | null>(null);
  let errors = $state<Record<string, string | null>>({});
  let busy = $state<string | null>(null);
  let message = $state<string | null>(null);
  let notes = $state<string[]>([]);
  let deleting = $state<string | null>(null);
  let form = $state({ site: '', name: '', database: 'shared' });
  let formError = $state<string | null>(null);

  async function refresh() {
    try {
      workspaces = (await api.workspacesList()).workspaces;
      error = null;
    } catch (e) {
      error = asMessage(e);
    }
  }

  /** Runs a row action with a busy marker; a failure shows on that row. */
  async function rowAction(site: string, f: () => Promise<void>) {
    busy = site;
    errors[site] = null;
    message = null;
    notes = [];
    try {
      await f();
    } catch (e) {
      errors[site] = asMessage(e);
    } finally {
      busy = null;
    }
    await refresh();
  }

  return {
    get groups() {
      const by = new Map<string, Workspace[]>();
      for (const w of workspaces) by.set(w.parentSite, [...(by.get(w.parentSite) ?? []), w]);
      return [...by.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([site, list]) => ({ site, workspaces: list }));
    },
    /** The sites a workspace can branch from: every site with a folder. */
    get parents() {
      return parents;
    },
    get error() {
      return error;
    },
    get busy() {
      return busy;
    },
    get message() {
      return message;
    },
    get notes() {
      return notes;
    },
    get deleting() {
      return deleting;
    },
    get form() {
      return form;
    },
    get formError() {
      return formError;
    },
    rowError: (site: string) => errors[site] ?? null,
    async load() {
      await refresh();
      try {
        parents = (await api.sitesList()).sites.filter((s) => s.kind !== 'proxy').map((s) => s.name);
      } catch {
        parents = [];
      }
      if (!form.site) form.site = parents[0] ?? '';
    },
    /** Makes the form's workspace; true when made (the form then empties). */
    async create(): Promise<boolean> {
      formError = null;
      message = null;
      notes = [];
      busy = 'new';
      try {
        const made = await api.workspacesCreate(form.site, form.name.trim(), form.database);
        message = `${made.workspace.name} is ready at ${made.workspace.url}`;
        notes = made.notes;
        form.name = '';
        await refresh();
        return true;
      } catch (e) {
        formError = asMessage(e);
        return false;
      } finally {
        busy = null;
      }
    },
    open: (w: Workspace) => api.openUrl(w.url),
    merge: (site: string) =>
      rowAction(site, async () => {
        const done = await api.workspacesMerge(site, false, false);
        message = done.message;
        notes = done.notes;
      }),
    askDelete(site: string) {
      deleting = site;
    },
    cancelDelete() {
      deleting = null;
    },
    async confirmDelete(keep: { keepDatabase: boolean; keepFiles: boolean }) {
      const site = deleting;
      deleting = null;
      if (!site) return;
      await rowAction(site, async () => {
        notes = (await api.workspacesDelete(site, keep.keepDatabase, keep.keepFiles)).notes;
      });
    },
  };
}
