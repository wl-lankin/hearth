import type { LogEntry, LogRead, SiteLogs } from './api';

export type LogsApi = {
  logsSites(): Promise<SiteLogs[]>;
  /** With `size`: just `unchanged` when the file still has that size. */
  logsRead(site: string, file: string, size?: number): Promise<LogRead>;
  logsClear(site: string, file: string): Promise<void>;
};

export type LevelFilter = 'all' | 'error' | 'warning' | 'info';

/** An entry with a key that stays the same while newer lines arrive. */
export type KeyedEntry = LogEntry & { key: string };

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** Monolog's levels, least to most severe. */
const RANK: Record<string, number> = { debug: 0, info: 1, notice: 2, warning: 3, error: 4, critical: 5, alert: 6, emergency: 7 };
const FLOOR: Record<LevelFilter, number> = { all: 0, info: 1, warning: 3, error: 4 };
/** Every this many polls (2 s each) the list of files is read again. */
const RESYNC_EVERY = 5;

/** Keys from the entry itself, counted from the oldest for repeats: new lines on top change none. */
function keyed(entries: LogEntry[]): KeyedEntry[] {
  const seen = new Map<string, number>();
  const out: KeyedEntry[] = new Array(entries.length);
  for (let i = entries.length - 1; i >= 0; i--) {
    const e = entries[i];
    const base = `${e.time}|${e.level}|${e.message}`;
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out[i] = { ...e, key: `${base}#${n}` };
  }
  return out;
}

export function createLogsStore(api: LogsApi) {
  let sites = $state<SiteLogs[]>([]);
  let site = $state<string | null>(null);
  let file = $state<string | null>(null);
  // True until a file is picked by hand: then the page keeps showing that one.
  let followNewest = $state(true);
  let all = $state<KeyedEntry[]>([]);
  let size = $state<number | undefined>(undefined);
  let level = $state<LevelFilter>('all');
  let search = $state('');
  let confirming = $state(false);
  let error = $state<string | null>(null);
  let polls = 0;

  async function read(known?: number) {
    if (!site || !file) return;
    const got = await api.logsRead(site, file, known);
    size = got.size;
    if (!got.unchanged) all = keyed(got.entries ?? []);
  }

  /** Reads the files again; follows the site's newest file, or any file when the chosen one is gone. */
  async function resync() {
    sites = await api.logsSites();
    const current = sites.find((s) => s.site === site) ?? sites[0];
    const files = current?.files ?? [];
    const keep = current?.site === site && !followNewest && files.some((f) => f.name === file);
    const next = keep ? file : (files[0]?.name ?? null);
    if (current?.site !== site || next !== file) {
      site = current?.site ?? null;
      file = next;
      all = [];
      size = undefined;
      confirming = false;
    }
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
    get entries(): KeyedEntry[] {
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
        await resync();
        await read();
      });
    },
    /** Opens a site's newest log (and follows it), or the given one (and keeps it). */
    async choose(nextSite: string, nextFile?: string) {
      await attempt(async () => {
        site = nextSite;
        followNewest = nextFile === undefined;
        file = nextFile ?? sites.find((s) => s.site === nextSite)?.files[0]?.name ?? null;
        all = [];
        size = undefined;
        confirming = false;
        await read();
      });
    },
    /**
     * The background poll: reads again only when the file changed. Every few polls, and after a
     * failed read, the files are listed again (a new daily file, a first log, a deleted file).
     */
    async poll() {
      polls += 1;
      try {
        if (polls % RESYNC_EVERY === 0 || !file) await resync();
        await read(size);
        error = null;
      } catch {
        try {
          await resync();
          await read(size);
          error = null;
        } catch (e) {
          error = asMessage(e);
        }
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
