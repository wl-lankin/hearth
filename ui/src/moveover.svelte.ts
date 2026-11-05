import type { JobStatus, MoveCheck, MoveItem, MoveScan, MoveStatus, MoveUninstall, SiteCheck } from './api';

export type MoveApi = {
  moveoverState(): Promise<MoveStatus>;
  moveoverScan(): Promise<MoveScan>;
  moveoverApply(): Promise<{ jobId: number }>;
  moveoverCheck(): Promise<MoveCheck>;
  moveoverAccept(key: string): Promise<{ accepted: string[] }>;
  moveoverUndo(): Promise<{ jobId: number }>;
  moveoverUninstall(): Promise<MoveUninstall>;
  jobGet(id: number): Promise<JobStatus>;
};

export type Step = 'loading' | 'dry' | 'partial' | 'moving' | 'check' | 'uninstalling' | 'done' | 'none';

/** `https://shop.test`: how a check row is named, and accepted. */
export const keyOf = (c: SiteCheck) => `${c.scheme}://${c.host}`;

const message = (e: unknown) => String(e).replace(/^Error: /, '');

/** Every row the uninstall waits for: the sites, then the services and Node (as the core keeps them). */
export function rowsOf(check: MoveCheck): SiteCheck[] {
  const services: SiteCheck[] = check.services.map((s) => ({
    host: s.name,
    scheme: 'service',
    verdict: s.running ? { kind: 'same' } : { kind: 'broken', detail: 'not running' },
  }));
  const node: SiteCheck = {
    host: 'node',
    scheme: 'node',
    verdict: check.node.fromHearth ? { kind: 'same' } : { kind: 'changed', detail: "still Herd's Node" },
  };
  return [...check.sites, ...services, node];
}

/** A row that needs a look: changed or broken, and not accepted. */
export const isOpen = (check: MoveCheck, row: SiteCheck) =>
  (row.verdict.kind === 'changed' || row.verdict.kind === 'broken') && !check.accepted.includes(keyOf(row));

/** The move-over wizard: dry run, move, check, uninstall, done. */
export function createMoveStore(api: MoveApi, opts: { pollMs?: number } = {}) {
  const pollMs = opts.pollMs ?? 500;
  let step = $state<Step>('loading');
  let items = $state<MoveItem[]>([]);
  let startedHerd = $state(false);
  let check = $state<MoveCheck | null>(null);
  let progress = $state(0);
  let current = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);
  // After the uninstall: the check that ran then, and a trust problem if there was one.
  let after = $state<MoveCheck | null>(null);
  let trustError = $state<string | null>(null);

  async function waitFor(jobId: number) {
    for (;;) {
      const job = await api.jobGet(jobId);
      progress = job.progress;
      current = job.message;
      if (job.state === 'done') return;
      if (job.state === 'failed') throw new Error(job.error ?? 'The step failed');
      await new Promise((r) => setTimeout(r, pollMs));
    }
  }

  async function runCheck() {
    check = await api.moveoverCheck();
    step = 'check';
  }

  async function scan() {
    step = 'loading';
    const result = await api.moveoverScan();
    items = result.plan.items;
    startedHerd = result.startedHerd;
    step = 'dry';
  }

  const guarded = async (work: () => Promise<void>) => {
    error = null;
    busy = true;
    try {
      await work();
    } catch (e) {
      error = message(e);
    } finally {
      busy = false;
    }
  };

  return {
    get step() {
      return step;
    },
    get items() {
      return items;
    },
    get startedHerd() {
      return startedHerd;
    },
    get check() {
      return check;
    },
    get progress() {
      return progress;
    },
    get current() {
      return current;
    },
    get error() {
      return error;
    },
    get busy() {
      return busy;
    },
    get after() {
      return after;
    },
    get trustError() {
      return trustError;
    },
    /** Every red row (changed or broken) either accepted or gone. */
    get canUninstall() {
      const current = check;
      return !!current && !rowsOf(current).some((row) => isOpen(current, row));
    },
    load: () =>
      guarded(async () => {
        const state = await api.moveoverState();
        if (state.uninstalled) step = 'done';
        else if (state.moved) await runCheck();
        else if (state.recorded) step = 'partial';
        else if (state.herd) await scan();
        else step = 'none';
      }),
    move: () =>
      guarded(async () => {
        step = 'moving';
        progress = 0;
        try {
          await waitFor((await api.moveoverApply()).jobId);
        } catch (e) {
          // A failed move can run again (it continues where it stopped), or go back.
          step = 'partial';
          throw e;
        }
        await runCheck();
      }),
    recheck: () => guarded(runCheck),
    accept: (key: string) =>
      guarded(async () => {
        const { accepted } = await api.moveoverAccept(key);
        if (check) check = { ...check, accepted };
      }),
    undo: () =>
      guarded(async () => {
        const back = check ? 'check' : 'partial';
        step = 'moving';
        try {
          await waitFor((await api.moveoverUndo()).jobId);
        } catch (e) {
          // Undo continues where it stopped the next time.
          step = back;
          throw e;
        }
        check = null;
        await scan();
      }),
    uninstall: () =>
      guarded(async () => {
        step = 'uninstalling';
        const result = await api.moveoverUninstall();
        check = result.check;
        if (result.exitCode === 0) {
          after = result.check;
          trustError = result.trustError ?? null;
        }
        step = result.exitCode === 0 ? 'done' : 'check';
        if (result.exitCode !== 0) throw new Error("Herd's uninstaller stopped without uninstalling. Herd is still there.");
      }),
  };
}

export type MoveStore = ReturnType<typeof createMoveStore>;
