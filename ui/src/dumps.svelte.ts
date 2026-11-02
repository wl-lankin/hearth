import type { Dump, DumpsStatus } from './api';

export type DumpsApi = {
  dumpsStatus(): Promise<DumpsStatus>;
  dumpsList(): Promise<Dump[]>;
  dumpsClear(): Promise<void>;
  dumpsEnable(on: boolean): Promise<DumpsStatus>;
  dumpsPort(port: number): Promise<DumpsStatus>;
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** How many lines of a dump show before "Show all". */
export const FOLD_LINES = 30;

/** The first `FOLD_LINES` lines of a dump, and how many more there are. */
export function fold(text: string): { text: string; hidden: number } {
  const lines = text.split('\n');
  if (lines.length <= FOLD_LINES) return { text, hidden: 0 };
  return { text: lines.slice(0, FOLD_LINES).join('\n'), hidden: lines.length - FOLD_LINES };
}

/** Folders a Laravel (or Composer) project has at its top. */
const PROJECT_FOLDERS = ['app', 'bootstrap', 'config', 'database', 'public', 'resources', 'routes', 'src', 'storage', 'tests', 'vendor'];

/** A file path from the project's top folder on ("app/Http/..."), or the whole path. */
export function shortPath(file: string | null): string {
  if (!file) return '';
  const parts = file.replaceAll('\\', '/').split('/');
  const at = parts.findIndex((p, i) => i > 0 && PROJECT_FOLDERS.includes(p));
  return at > 0 ? parts.slice(at).join('/') : file;
}

export function createDumpsStore(api: DumpsApi) {
  let status = $state<DumpsStatus | null>(null);
  let all = $state<Dump[]>([]);
  // undefined: every site; null: dumps without one.
  let filter = $state<string | null | undefined>(undefined);
  let portError = $state<string | null>(null);
  let error = $state<string | null>(null);

  async function attempt(f: () => Promise<void>) {
    error = null;
    try {
      await f();
    } catch (e) {
      error = asMessage(e);
    }
  }

  return {
    get status() {
      return status;
    },
    get portError() {
      return portError;
    },
    get error() {
      return error;
    },
    get filter() {
      return filter;
    },
    set filter(value: string | null | undefined) {
      filter = value;
    },
    get sites() {
      const by = new Map<string | null, { site: string | null; label: string; count: number }>();
      for (const d of all) {
        const s = by.get(d.site) ?? { site: d.site, label: d.site ?? 'Unknown', count: 0 };
        s.count += 1;
        by.set(d.site, s);
      }
      return [...by.values()].sort((a, b) => (a.site === null ? 1 : b.site === null ? -1 : a.label.localeCompare(b.label)));
    },
    /** The dumps of the chosen site, newest first. */
    get dumps() {
      return filter === undefined ? all : all.filter((d) => d.site === filter);
    },
    async load() {
      await attempt(async () => {
        status = await api.dumpsStatus();
        all = await api.dumpsList();
      });
    },
    /** The background poll: a failure shows, but an error from an action stays until the next one. */
    async refresh() {
      try {
        all = await api.dumpsList();
      } catch (e) {
        error = asMessage(e);
      }
    },
    async clear() {
      await attempt(async () => {
        await api.dumpsClear();
        all = [];
      });
    },
    async setEnabled(on: boolean) {
      await attempt(async () => {
        status = await api.dumpsEnable(on);
      });
    },
    async setPort(port: number) {
      portError = null;
      try {
        status = await api.dumpsPort(port);
      } catch (e) {
        portError = asMessage(e);
      }
    },
  };
}

export type DumpsStore = ReturnType<typeof createDumpsStore>;
