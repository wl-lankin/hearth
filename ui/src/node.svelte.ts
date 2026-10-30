import type { JobStatus, NodeBuild, NodeList } from './api';

export type NodeApi = {
  nodeList(): Promise<NodeList>;
  nodeAvailable(): Promise<NodeBuild[]>;
  /** Starts installing the newest build of a major ("22"); returns the job id. */
  nodeInstall(spec: string): Promise<number>;
  jobGet(id: number): Promise<JobStatus>;
  nodeUninstall(version: string): Promise<void>;
  nodeDefault(version: string): Promise<void>;
  /** Optional: running installs to pick up when the page opens again. */
  jobsList?(): Promise<JobStatus[]>;
};

/** One row of the Node page: an installed version, or a major the catalog offers. */
export interface NodeRow {
  key: string;
  major: number;
  version: string;
  lts: string | null;
  installedVersion: string | null;
  isDefault: boolean;
  /** A newer build of this major that is not installed yet. */
  update: string | null;
  job: { progress: number; message: string } | null;
  error: string | null;
}

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));
const parts = (v: string) => v.split('.').map(Number);
/** Sorts newer versions first: positive when `b` is newer than `a`. */
const newerFirst = (a: string, b: string) => {
  const [x, y] = [parts(a), parts(b)];
  return y[0] - x[0] || y[1] - x[1] || y[2] - x[2];
};
const majorOf = (version: string) => parts(version)[0];

export function createNodeStore(api: NodeApi) {
  let list = $state<NodeList>({ default: null, installed: [] });
  let available = $state<NodeBuild[]>([]);
  let catalogError = $state<string | null>(null);
  // Installs and their errors, by major.
  let jobs = $state<Record<number, { id: number; progress: number; message: string }>>({});
  let errors = $state<Record<number, string | null>>({});

  async function refresh() {
    list = await api.nodeList();
    try {
      available = await api.nodeAvailable();
      catalogError = null;
    } catch (e) {
      catalogError = asMessage(e);
    }
  }

  async function apply(major: number, status: JobStatus) {
    if (status.state === 'running') {
      jobs[major] = { id: status.id, progress: status.progress, message: status.message };
      return;
    }
    delete jobs[major];
    if (status.state === 'failed') errors[major] = status.error;
    else await refresh();
  }

  const progress = (major: number) => (jobs[major] ? { progress: jobs[major].progress, message: jobs[major].message } : null);

  return {
    get rows(): NodeRow[] {
      const installed = list.installed.map((i): NodeRow => {
        const build = available.find((b) => b.major === i.major);
        const newer = build && !build.installed && newerFirst(i.version, build.version) > 0 ? build.version : null;
        const newestOfMajor = !list.installed.some((o) => o.major === i.major && newerFirst(o.version, i.version) < 0);
        return {
          key: i.version,
          major: i.major,
          version: i.version,
          lts: i.lts,
          installedVersion: i.version,
          isDefault: list.default === i.version,
          update: newestOfMajor ? newer : null,
          job: newestOfMajor ? progress(i.major) : null,
          error: newestOfMajor ? (errors[i.major] ?? null) : null,
        };
      });
      const offered = available
        .filter((b) => !list.installed.some((i) => i.major === b.major))
        .map((b): NodeRow => ({
          key: String(b.major),
          major: b.major,
          version: b.version,
          lts: b.lts,
          installedVersion: null,
          isDefault: false,
          update: null,
          job: progress(b.major),
          error: errors[b.major] ?? null,
        }));
      return [...installed, ...offered].sort((a, b) => b.major - a.major || newerFirst(a.version, b.version));
    },
    get catalogError() {
      return catalogError;
    },
    /** True while any install runs (the page polls then). */
    get busy() {
      return Object.keys(jobs).length > 0;
    },
    async load() {
      await refresh();
      // Installs keep running while the page is closed; show them again.
      const running = (await api.jobsList?.().catch(() => [])) ?? [];
      for (const job of running) {
        if (job.state === 'running' && job.key.startsWith('node.install:')) {
          jobs[majorOf(job.key.slice('node.install:'.length))] = { id: job.id, progress: job.progress, message: job.message };
        }
      }
    },
    async install(major: number) {
      errors[major] = null;
      try {
        const id = await api.nodeInstall(String(major));
        await apply(major, await api.jobGet(id));
      } catch (e) {
        errors[major] = asMessage(e);
      }
    },
    /** Refreshes every running install once. */
    async poll() {
      for (const [major, job] of Object.entries(jobs)) {
        try {
          await apply(Number(major), await api.jobGet(job.id));
        } catch (e) {
          delete jobs[Number(major)];
          errors[Number(major)] = asMessage(e);
        }
      }
    },
    async uninstall(version: string) {
      const major = majorOf(version);
      errors[major] = null;
      try {
        await api.nodeUninstall(version);
        await refresh();
      } catch (e) {
        errors[major] = asMessage(e);
      }
    },
    async makeDefault(version: string) {
      const major = majorOf(version);
      errors[major] = null;
      try {
        await api.nodeDefault(version);
        list.default = version;
      } catch (e) {
        errors[major] = asMessage(e);
      }
    },
  };
}

export type NodeStore = ReturnType<typeof createNodeStore>;
