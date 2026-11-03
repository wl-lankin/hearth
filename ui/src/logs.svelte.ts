import type { LogEntry, LogRead, SiteLogs } from './api';

export type LogsApi = {
  logsSites(): Promise<SiteLogs[]>;
  /** With `size`: just `unchanged` when the file still has that size. */
  logsRead(site: string, file: string, size?: number): Promise<LogRead>;
  logsClear(site: string, file: string): Promise<void>;
};

export type LevelFilter = 'all' | 'error' | 'warning' | 'info';

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** Monolog's levels, least to most severe. */
const RANK: Record<string, number> = { debug: 0, info: 1, notice: 2, warning: 3, error: 4, critical: 5, alert: 6, emergency: 7 };
const FLOOR: Record<LevelFilter, number> = { all: 0, info: 1, warning: 3, error: 4 };

export function createLogsStore(api: LogsApi) {
  let sites = $state<SiteLogs[]>([]);
  let site = $state<string | null>(null);
  let file = $state<string | null>(null);
  let all = $state<LogEntry[]>([]);
  let size = $state<number | undefined>(undefined);
  let level = $state<LevelFilter>('all');
  let search = $state('');
  let confirming = $state(false);
  let error = $state<string | null>(null);

  async function read(known?: number) {
    if (!site || !file) return;
    const got = await api.logsRead(site, file, known);
    size = got.size;
    if (!got.unchanged) all = got.entries ?? [];
  }

  async function attempt(f: () => Promise<void>) {
    error = null;
    try {
      await f();
    } catch (e) {
      error = asMessage(e);
    }
  }

  return {
    get sites() {
      return sites;
    },
    get site() {
      return site;
    },
    get file() {
      return file;
    },
    get files() {
      return sites.find((s) => s.site === site)?.files ?? [];
    },
    get level() {
      return level;
    },
    set level(value: LevelFilter) {
      level = value;
    },
    get search() {
      return search;
    },
    set search(value: string) {
      search = value;
    },
    get confirming() {
      return confirming;
    },
    get error() {
      return error;
    },
    /** The entries at or above the level filter that match the search, newest first. */
    get entries() {
      const floor = FLOOR[level];
      const needle = search.trim().toLowerCase();
      return all.filter(
        (e) =>
          (RANK[e.level] ?? 0) >= floor &&
          (!needle || e.message.toLowerCase().includes(needle) || e.detail.toLowerCase().includes(needle)),
      );
    },
    async load() {
      await attempt(async () => {
        sites = await api.logsSites();
        if (!sites.some((s) => s.site === site)) {
          site = sites[0]?.site ?? null;
          file = sites[0]?.files[0]?.name ?? null;
        }
        size = undefined;
        await read();
      });
    },
    /** Opens a site's newest log, or the given one. */
    async choose(nextSite: string, nextFile?: string) {
      await attempt(async () => {
        site = nextSite;
        file = nextFile ?? sites.find((s) => s.site === nextSite)?.files[0]?.name ?? null;
        all = [];
        size = undefined;
        confirming = false;
        await read();
      });
    },
    /** The background poll: reads again only when the file changed. */
    async poll() {
      try {
        await read(size);
      } catch (e) {
        error = asMessage(e);
      }
    },
    askClear() {
      confirming = true;
    },
    cancelClear() {
      confirming = false;
    },
    async confirmClear() {
      await attempt(async () => {
        if (site && file) await api.logsClear(site, file);
        confirming = false;
        await read();
      });
    },
  };
}

export type LogsStore = ReturnType<typeof createLogsStore>;
