import type { AvailableBuild, JobStatus, PhpList } from './api';

export type PhpApi = {
  phpList(): Promise<PhpList>;
  phpAvailable(): Promise<AvailableBuild[]>;
  phpInstall(minor: string): Promise<number>;
  jobGet(id: number): Promise<JobStatus>;
  phpUninstall(minor: string): Promise<string | null>;
  phpUse(minor: string): Promise<void>;
  phpExtension(minor: string, name: string, on: boolean): Promise<void>;
  /** Optional: running installs to pick up when the view opens again. */
  jobsList?(): Promise<JobStatus[]>;
};

/** One PHP minor as the PHP view shows it. */
export interface PhpRow {
  minor: string;
  /** The catalog's newest version, or the installed one when the catalog is unavailable. */
  version: string;
  channel: 'stable' | 'rc';
  installedVersion: string | null;
  isDefault: boolean;
  updateAvailable: boolean;
  extensions: Record<string, boolean>;
  job: { progress: number; message: string } | null;
  error: string | null;
}

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));
const minorNumber = (m: string) => m.split('.').map(Number);
const newerFirst = (a: string, b: string) => {
  const [x, y] = [minorNumber(a), minorNumber(b)];
  return y[0] - x[0] || y[1] - x[1];
};

export function createPhpStore(api: PhpApi) {
  let list = $state<PhpList>({ default: null, installed: [] });
  let available = $state<AvailableBuild[]>([]);
  let catalogError = $state<string | null>(null);
  let jobs = $state<Record<string, { id: number; progress: number; message: string }>>({});
  let errors = $state<Record<string, string | null>>({});

  async function refresh() {
    list = await api.phpList();
    try {
      available = await api.phpAvailable();
      catalogError = null;
    } catch (e) {
      catalogError = asMessage(e);
    }
  }

  async function apply(minor: string, status: JobStatus) {
    if (status.state === 'running') {
      jobs[minor] = { id: status.id, progress: status.progress, message: status.message };
      return;
    }
    delete jobs[minor];
    if (status.state === 'failed') errors[minor] = status.error;
    else await refresh();
  }

  return {
    get rows(): PhpRow[] {
      const minors = new Set([...available.map((b) => b.minor), ...list.installed.map((i) => i.minor)]);
      const rows = [...minors].map((minor): PhpRow => {
        const build = available.find((b) => b.minor === minor);
        const mine = list.installed.find((i) => i.minor === minor);
        return {
          minor,
          version: build?.version ?? mine?.version ?? '',
          channel: build?.channel ?? 'stable',
          installedVersion: mine?.version ?? null,
          isDefault: list.default === minor && !!mine,
          updateAvailable: !!build?.updateAvailable && !!mine,
          extensions: mine?.extensions ?? {},
          job: jobs[minor] ? { progress: jobs[minor].progress, message: jobs[minor].message } : null,
          error: errors[minor] ?? null,
        };
      });
      return rows.sort((a, b) => Number(a.channel === 'rc') - Number(b.channel === 'rc') || newerFirst(a.minor, b.minor));
    },
    get catalogError() {
      return catalogError;
    },
    /** True while any install runs (the view polls then). */
    get busy() {
      return Object.keys(jobs).length > 0;
    },
    async load() {
      await refresh();
      // Installs keep running while the view is closed; show them again.
      const running = (await api.jobsList?.().catch(() => [])) ?? [];
      for (const job of running) {
        const minor = job.key.startsWith('php.install:') ? job.key.slice('php.install:'.length) : null;
        if (minor && job.state === 'running') jobs[minor] = { id: job.id, progress: job.progress, message: job.message };
      }
    },
    async install(minor: string) {
      errors[minor] = null;
      try {
        const id = await api.phpInstall(minor);
        await apply(minor, await api.jobGet(id));
      } catch (e) {
        errors[minor] = asMessage(e);
      }
    },
    /** Refreshes every running install once. */
    async poll() {
      for (const [minor, job] of Object.entries(jobs)) {
        try {
          await apply(minor, await api.jobGet(job.id));
        } catch (e) {
          delete jobs[minor];
          errors[minor] = asMessage(e);
        }
      }
    },
    async uninstall(minor: string) {
      errors[minor] = null;
      try {
        await api.phpUninstall(minor);
        await refresh();
      } catch (e) {
        errors[minor] = asMessage(e);
      }
    },
    async use(minor: string) {
      errors[minor] = null;
      try {
        await api.phpUse(minor);
        list.default = minor;
      } catch (e) {
        errors[minor] = asMessage(e);
      }
    },
    async toggle(minor: string, name: string) {
      const php = list.installed.find((i) => i.minor === minor);
      if (!php) return;
      const on = !php.extensions[name];
      php.extensions[name] = on;
      errors[minor] = null;
      try {
        await api.phpExtension(minor, name, on);
      } catch (e) {
        php.extensions[name] = !on;
        errors[minor] = asMessage(e);
      }
    },
  };
}

export type PhpStore = ReturnType<typeof createPhpStore>;
