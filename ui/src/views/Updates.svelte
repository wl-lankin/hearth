<script lang="ts">
  import { checkForUpdates, installUpdateNow, onUpdateState, updateState, type UpdateState } from '../api';
  import { describeUpdate } from '../updates';

  let state = $state<UpdateState>({ phase: 'idle' });

  $effect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    updateState().then((s) => (state = s));
    onUpdateState((s) => (state = s)).then((unlisten) => (gone ? unlisten() : (stop = unlisten)));
    return () => {
      gone = true;
      stop?.();
    };
  });

  const busy = $derived(state.phase === 'checking' || state.phase === 'downloading' || state.phase === 'installing');
</script>

<section class="card">
  <h2>Updates</h2>
  <div class="row">
    <div class="status">
      <span>Version {__HEARTH_VERSION__}</span>
      <span class="line" class:error={state.phase === 'error'} aria-live="polite">{describeUpdate(state)}</span>
    </div>
    {#if state.phase === 'ready'}
      <button class="primary" onclick={() => installUpdateNow()}>Restart to update</button>
    {:else if state.phase !== 'disabled'}
      <button onclick={() => checkForUpdates()} disabled={busy}>Check now</button>
    {/if}
  </div>
</section>

<style>
  .card {
    max-width: 720px;
    margin-bottom: 16px;
    padding: 16px 20px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 40px;
  }

  .status {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .line {
    color: var(--muted);
    font-size: 13px;
  }

  .line.error {
    color: var(--danger);
  }

  button {
    padding: 6px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
    cursor: pointer;
    white-space: nowrap;
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }
</style>
