<script lang="ts">
  import { terminalGet, terminalSet, type TerminalStatus } from '../api';

  let status = $state<TerminalStatus | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);

  $effect(() => {
    terminalGet()
      .then((s) => (status = s))
      .catch((e) => (error = String(e)));
  });

  async function toggle(on: boolean) {
    saving = true;
    error = null;
    try {
      status = await terminalSet(on);
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<section class="card">
  <h2>Terminal</h2>
  <label class="row">
    <span>
      Use Hearth's PHP in terminals
      <small>Puts <code>php</code>, <code>composer</code> and <code>laravel</code> from Hearth first on your PATH. Takes effect in new terminals.</small>
    </span>
    <input
      type="checkbox"
      checked={status?.enabled ?? false}
      disabled={!status || saving}
      onchange={(e) => toggle(e.currentTarget.checked)}
    />
  </label>
  {#if status && status.others.length > 0}
    <p class="note">
      {#if status.enabled}
        Other PHP folders are also on your PATH ({status.others.join(', ')}). New terminals use Hearth's PHP first.
      {:else}
        Terminals currently use PHP from {status.others.join(', ')}.
      {/if}
    </p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
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
    cursor: pointer;
  }

  .row span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  small,
  .note {
    color: var(--muted);
    font-size: 12.5px;
  }

  .note {
    margin: 8px 0 0;
    overflow-wrap: anywhere;
  }

  input[type='checkbox'] {
    width: 18px;
    height: 18px;
    flex: none;
    accent-color: var(--accent);
  }

  code {
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
  }

  .error {
    margin: 10px 0 0;
    color: var(--danger);
  }
</style>
