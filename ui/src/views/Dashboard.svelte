<script lang="ts">
  import { coreStatus, type CoreStatus } from '../api';
  import { formatUptime } from '../format';

  let status = $state<CoreStatus | null>(null);
  let failed = $state<string | null>(null);

  async function refresh() {
    try {
      status = await coreStatus();
      failed = null;
    } catch (e) {
      failed = String(e);
    }
  }

  $effect(() => {
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  });
</script>

<h1>Dashboard</h1>

<section class="card">
  <header>
    <h2>Core</h2>
    {#if status && !failed}
      <span class="pill ok">Running</span>
    {:else if failed}
      <span class="pill bad">Not responding</span>
    {/if}
  </header>
  {#if status}
    <dl>
      <dt>Version</dt>
      <dd>{status.version}</dd>
      <dt>Uptime</dt>
      <dd>{formatUptime(status.uptimeSecs)}</dd>
      <dt>Home</dt>
      <dd class="path" title={status.home}>{status.home}</dd>
    </dl>
  {/if}
  {#if failed}
    <p class="error">{failed}</p>
  {/if}
</section>

<section class="card empty">
  <h2>Coming next</h2>
  <p>Sites, PHP and services arrive in the next milestones.</p>
</section>

<style>
  h1 {
    margin: 0 0 20px;
    font-size: 22px;
    font-weight: 650;
  }

  .card {
    max-width: 720px;
    margin-bottom: 16px;
    padding: 18px 20px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .pill {
    padding: 2px 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
  }

  .ok {
    background: var(--ok-soft);
    color: var(--ok);
  }

  .bad {
    background: var(--accent-soft);
    color: var(--danger);
  }

  dl {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: 6px 16px;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    min-width: 0;
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 13px;
  }

  .empty p {
    margin: 6px 0 0;
    color: var(--muted);
  }

  .error {
    margin: 10px 0 0;
    color: var(--danger);
  }
</style>
