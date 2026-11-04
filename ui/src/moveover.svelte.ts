import type { JobStatus, MoveCheck, MoveItem, MoveScan, MoveStatus, SiteCheck } from './api';

export type MoveApi = {
  moveoverState(): Promise<MoveStatus>;
  moveoverScan(): Promise<MoveScan>;
  moveoverApply(): Promise<{ jobId: number }>;
  moveoverCheck(): Promise<MoveCheck>;
  moveoverAccept(key: string): Promise<{ accepted: string[] }>;
  moveoverUndo(): Promise<{ jobId: number }>;
  moveoverUninstall(): Promise<{ exitCode: number; check: MoveCheck }>;
  jobGet(id: number): Promise<JobStatus>;
};

export type Step = 'loading' | 'dry' | 'moving' | 'check' | 'uninstalling' | 'done' | 'none';

/** `https://shop.test`: how a check row is named, and accepted. */
export const keyOf = (c: SiteCheck) => `${c.scheme}://${c.host}`;

const message = (e: unknown) => String(e).replace(/^Error: /, '');

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
    /** Every red row (changed or broken) either accepted or gone. */
    get canUninstall() {
      return (
        !!check &&
        check.sites.every((c) => c.verdict.kind === 'same' || c.verdict.kind === 'better' || check!.accepted.includes(keyOf(c)))
      );
    },
    load: () =>
      guarded(async () => {
        const state = await api.moveoverState();
        if (state.uninstalled) step = 'done';
        else if (state.moved) await runCheck();
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
          // A failed move can run again: it continues where it stopped.
          step = 'dry';
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
        step = 'moving';
        await waitFor((await api.moveoverUndo()).jobId);
        await scan();
      }),
    uninstall: () =>
      guarded(async () => {
        step = 'uninstalling';
        const result = await api.moveoverUninstall();
        check = result.check;
        step = result.exitCode === 0 ? 'done' : 'check';
        if (result.exitCode !== 0) throw new Error("Herd's uninstaller stopped without uninstalling. Herd is still there.");
      }),
  };
}

export type MoveStore = ReturnType<typeof createMoveStore>;
