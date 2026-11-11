import type { DumpsStatus, RequestDetail, RequestFinding, RequestQuery, RequestSummary } from './api';

export type RequestsApi = {
  dumpsStatus(): Promise<DumpsStatus>;
  /** With `after`: only the requests newer than that id. */
  requestsList(after?: number): Promise<RequestSummary[]>;
  requestsGet(id: number): Promise<RequestDetail>;
  requestsClear(): Promise<void>;
  requestsEnable(on: boolean): Promise<DumpsStatus>;
};

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** How many requests the core keeps, and so the page. */
const KEEP = 200;
/** The thinnest a query shows on the timeline, in percent. */
const HAIRLINE = 0.4;

/**
 * The query rows to show: an N+1 group folds into its first query, with the count of the others,
 * unless that group (named by its first query's index) is in `open`.
 */
export function rows(count: number, findings: RequestFinding[], open: Set<number>): { index: number; more: number }[] {
  const folded = new Map<number, number>();
  const hidden = new Set<number>();
  for (const f of findings) {
    if (f.kind !== 'n+1' || f.queries.length < 2 || open.has(f.queries[0])) continue;
    folded.set(f.queries[0], f.queries.length - 1);
    for (const i of f.queries.slice(1)) hidden.add(i);
  }
  const out: { index: number; more: number }[] = [];
  for (let index = 0; index < count; index++) {
    if (!hidden.has(index)) out.push({ index, more: folded.get(index) ?? 0 });
  }
  return out;
}

/** Where each query sits on the request's timeline: left edge and width in percent. */
export function timeline(queries: RequestQuery[], requestMs: number): { left: number; width: number }[] {
  const end = Math.max(requestMs, ...queries.map((q) => q.startMs + q.ms), 1);
  const pct = (ms: number) => Math.round((ms / end) * 1000) / 10;
  return queries.map((q) => ({ left: pct(q.startMs), width: Math.max(pct(q.ms), HAIRLINE) }));
}

export function createRequestsStore(api: RequestsApi) {
  let status = $state<DumpsStatus | null>(null);
  let all = $state<RequestSummary[]>([]);
  // undefined: every site; null: requests without one.
  let filter = $state<string | null | undefined>(undefined);
  let current = $state<RequestDetail | null>(null);
  let error = $state<string | null>(null);

  // False until the first answer (or failure): the page shows its loader instead of its empty state.
  let loaded = $state(false);

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
    get error() {
      return error;
    },
    get current() {
      return current;
    },
    get filter() {
      return filter;
    },
    set filter(value: string | null | undefined) {
      filter = value;
    },
    get sites() {
      const by = new Map<string | null, { site: string | null; label: string; count: number }>();
      for (const r of all) {
        const s = by.get(r.site) ?? { site: r.site, label: r.site ?? 'Unknown', count: 0 };
        s.count += 1;
        by.set(r.site, s);
      }
      return [...by.values()].sort((a, b) => (a.site === null ? 1 : b.site === null ? -1 : a.label.localeCompare(b.label)));
    },
    /** The requests of the chosen site, newest first. */
    get requests() {
      return filter === undefined ? all : all.filter((r) => r.site === filter);
    },
    get loaded() {
      return loaded;
    },
    async load() {
      try {
        await attempt(async () => {
          status = await api.dumpsStatus();
          all = await api.requestsList();
        });
      } finally {
        loaded = true;
      }
    },
    /** The background poll: a failure shows, but an error from an action stays until the next one. */
    async refresh() {
      try {
        const newer = await api.requestsList(all[0]?.id);
        if (newer.length > 0) all = [...newer, ...all].slice(0, KEEP);
      } catch (e) {
        error = asMessage(e);
      }
    },
    async open(id: number) {
      await attempt(async () => {
        current = await api.requestsGet(id);
      });
    },
    close() {
      current = null;
    },
    async clear() {
      await attempt(async () => {
        await api.requestsClear();
        all = [];
        current = null;
      });
    },
    async setEnabled(on: boolean) {
      await attempt(async () => {
        status = await api.requestsEnable(on);
      });
    },
  };
}

export type RequestsStore = ReturnType<typeof createRequestsStore>;
