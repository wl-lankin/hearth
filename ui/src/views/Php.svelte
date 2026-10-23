<script lang="ts">
  import * as api from '../api';
  import { createPhpStore, type PhpRow } from '../php.svelte';
  import PhpIni from './PhpIni.svelte';

  const php = createPhpStore(api);
  let confirming = $state<string | null>(null);
  let editing = $state<string | null>(null);
  let laravel = $state<string | null>(null);

  php.load();

  // Follow running installs; stop polling once none runs.
  $effect(() => {
    if (!php.busy) return;
    const timer = setInterval(() => php.poll(), 500);
    return () => clearInterval(timer);
  });

  function stateLine(row: PhpRow): string {
    if (row.job) return `${row.job.message} (${Math.round(row.job.progress * 100)} %)`;
    if (!row.installedVersion) return 'Not installed';
    if (row.updateAvailable) return `Installed ${row.installedVersion}, ${row.version} available`;
    return `Installed ${row.installedVersion}`;
  }

  async function installLaravel() {
    laravel = 'Installing the Laravel installer';
    try {
      const { jobId } = await api.phpInstallLaravel();
      let job = await api.jobGet(jobId);
      while (job.state === 'running') {
        await new Promise((r) => setTimeout(r, 1000));
        job = await api.jobGet(jobId);
      }
      laravel = job.state === 'done' ? 'The laravel command is ready' : job.error;
    } catch (e) {
      laravel = String(e);
    }
  }

  const label: Record<string, string> = { redis: 'Redis', xdebug: 'Xdebug' };
</script>

<h1>PHP</h1>

{#if php.catalogError}
  <p class="notice">Could not load the list of PHP versions: {php.catalogError}</p>
{/if}

<section class="card list" aria-label="PHP versions">
  {#each php.rows as row (row.minor)}
    <div class="row" class:installed={!!row.installedVersion}>
      <div class="name">
        <strong>PHP {row.minor}</strong>
        {#if row.channel === 'rc'}<span class="tag">Preview</span>{/if}
        {#if row.isDefault}<span class="tag default">Default</span>{/if}
      </div>
      <div class="state">
        <span>{stateLine(row)}</span>
        {#if row.job}
          <span class="bar" role="progressbar" aria-valuenow={Math.round(row.job.progress * 100)} aria-valuemin="0" aria-valuemax="100">
            <span style:width="{row.job.progress * 100}%"></span>
          </span>
        {/if}
        {#if row.error}<span class="error" role="alert">{row.error}</span>{/if}
      </div>
      <div class="extensions">
        {#if row.installedVersion}
          {#each Object.keys(row.extensions) as name (name)}
            <label class="switch">
              <input
                type="checkbox"
                checked={row.extensions[name]}
                disabled={!!row.job}
                onchange={() => php.toggle(row.minor, name)}
              />
              {label[name] ?? name}
            </label>
          {/each}
        {/if}
      </div>
      <div class="actions">
        {#if row.job}
          <span class="muted">Installing</span>
        {:else if confirming === row.minor}
          <span class="confirm">Remove PHP {row.minor}? Your user.ini is kept.</span>
          <button class="danger" onclick={() => { confirming = null; php.uninstall(row.minor); }}>Remove</button>
          <button onclick={() => (confirming = null)}>Keep</button>
        {:else if !row.installedVersion}
          <button onclick={() => php.install(row.minor)}>Install</button>
        {:else}
          {#if row.updateAvailable}
            <button class="primary" onclick={() => php.install(row.minor)}>Update</button>
          {/if}
          {#if !row.isDefault}
            <button onclick={() => php.use(row.minor)}>Make default</button>
          {/if}
          <button onclick={() => (editing = editing === row.minor ? null : row.minor)}>php.ini</button>
          <button onclick={() => (confirming = row.minor)}>Remove</button>
        {/if}
      </div>
    </div>
    {#if editing === row.minor}
      <PhpIni minor={row.minor} onclose={() => (editing = null)} />
    {/if}
  {/each}
</section>

<section class="card tools">
  <h2>Tools</h2>
  <div class="tool">
    <span>Composer comes with every PHP version. The Laravel installer adds the <code>laravel new</code> command.</span>
    <button onclick={installLaravel} disabled={laravel === 'Installing the Laravel installer'}>Install the Laravel installer</button>
  </div>
  {#if laravel}<p class="muted" aria-live="polite">{laravel}</p>{/if}
</section>

<style>
  h1 {
    margin: 0 0 20px;
    font-size: 22px;
    font-weight: 650;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
  }

  .card {
    max-width: 900px;
    margin-bottom: 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  .list {
    padding: 4px 0;
  }

  .row {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr) auto;
    grid-template-areas: 'name state actions' 'name extensions actions';
    gap: 2px 16px;
    align-items: center;
    padding: 12px 20px;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  .name {
    grid-area: name;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .row:not(.installed) .name strong {
    font-weight: 500;
    color: var(--muted);
  }

  .state {
    grid-area: state;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    font-size: 13px;
  }

  .extensions {
    grid-area: extensions;
    display: flex;
    gap: 14px;
  }

  .actions {
    grid-area: actions;
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
  }

  .tag {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }

  .tag.default {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .bar {
    display: block;
    height: 4px;
    max-width: 280px;
    border-radius: 2px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 200ms ease-out;
  }

  .switch {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    cursor: pointer;
  }

  .switch input {
    accent-color: var(--accent);
  }

  button {
    padding: 5px 12px;
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

  button.danger {
    border-color: var(--danger);
    color: var(--danger);
  }

  .confirm {
    font-size: 13px;
  }

  .error {
    color: var(--danger);
  }

  .muted {
    color: var(--muted);
    font-size: 13px;
  }

  .notice {
    max-width: 900px;
    margin: 0 0 16px;
    padding: 10px 14px;
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .tools {
    padding: 16px 20px;
  }

  .tool {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    font-size: 13px;
  }

  code {
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
  }

  @media (max-width: 900px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
      grid-template-areas: 'name actions' 'state state' 'extensions extensions';
    }
  }
</style>
