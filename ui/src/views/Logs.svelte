<script lang="ts">
  import EmptyState from '../EmptyState.svelte';
  import Kindling from '../Kindling.svelte';
  import { art } from '../illustrations';
  import Select from '../Select.svelte';
  import * as api from '../api';
  import { formatSize } from '../format';
  import { createLogsStore, type LevelFilter } from '../logs.svelte';

  const store = createLogsStore(api);
  // Opened entries, by their key (positions shift as new lines arrive on top).
  let open = $state<Record<string, boolean>>({});

  store.load();

  // New lines show up while the page is open; the core only re-reads a file that changed.
  $effect(() => {
    const timer = setInterval(() => store.poll(), 2000);
    return () => clearInterval(timer);
  });

  async function pickSite(value: string) {
    open = {};
    await store.choose(value);
  }

  async function pickFile(value: string) {
    open = {};
    if (store.site) await store.choose(store.site, value);
  }

  const levels: { id: LevelFilter; label: string }[] = [
    { id: 'all', label: 'All' },
    { id: 'info', label: 'Info and up' },
    { id: 'warning', label: 'Warnings and up' },
    { id: 'error', label: 'Errors' },
  ];
  const severity = (level: string) =>
    ['error', 'critical', 'alert', 'emergency'].includes(level) ? 'bad' : level === 'warning' ? 'warn' : level === 'debug' ? 'quiet' : '';
</script>

<h1>Logs</h1>

{#if !store.loaded}
  <section class="card"><Kindling rows={3} label="Loading logs" /></section>
{:else if store.sites.length === 0}
  <EmptyState art={art.logs} title="No logs yet" line="Sites with a storage/logs folder show their Laravel logs here" />
{:else}
  <section class="card bar">
    <span class="pick">
      <Select label="Site" value={store.site ?? ''} onchange={pickSite} options={store.sites.map((s) => ({ value: s.site, label: `${s.site}.test` }))} />
    </span>
    <span class="pick">
      <Select
        label="Log file"
        value={store.file ?? ''}
        onchange={pickFile}
        options={store.files.map((f) => ({ value: f.name, label: `${f.name} (${formatSize(f.size)})` }))}
      />
    </span>
    <div class="levels" role="group" aria-label="Level">
      {#each levels as l (l.id)}
        <button class:on={store.level === l.id} aria-pressed={store.level === l.id} onclick={() => (store.level = l.id)}>{l.label}</button>
      {/each}
    </div>
    <input type="search" placeholder="Search" aria-label="Search" bind:value={store.search} />
    <button onclick={() => store.askClear()} disabled={!store.file}>Empty file</button>
    {#if store.confirming}
      <div class="confirm" role="alert">
        <span>Empty {store.file}? Its entries are gone for good.</span>
        <button onclick={() => store.cancelClear()}>Cancel</button>
        <button class="danger" onclick={() => store.confirmClear()}>Empty</button>
      </div>
    {/if}
  </section>

  {#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

  <section class="card list" aria-label="Log entries">
    {#if store.entries.length === 0}
      {#if store.search || store.level !== 'all'}
        <p class="empty">No entries match.</p>
      {:else}
        <EmptyState art={art.logs} title="All quiet" line="No errors in your logs" />
      {/if}
    {/if}
    {#each store.entries as e (e.key)}
      <article class="entry">
        <button class="head" onclick={() => (open[e.key] = !open[e.key])} aria-expanded={!!open[e.key]} disabled={!e.detail}>
          <span class="level {severity(e.level)}">{e.level}</span>
          <span class="time">{e.time}</span>
          <span class="message">{e.message}</span>
        </button>
        {#if open[e.key] && e.detail}
          <pre>{e.detail}</pre>
        {/if}
        {#if open[e.key] && e.truncated}<p class="hint">This entry was cut at 64 KB.</p>{/if}
      </article>
    {/each}
  </section>
{/if}

<style>
  .pick {
    min-width: 180px;
    max-width: 280px;
  }
  h1 {
    margin: 0 0 16px;
    font-size: 22px;
    font-weight: 650;
  }
  .card {
    max-width: 1100px;
    margin-bottom: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
  }
  .levels {
    display: flex;
    gap: 2px;
  }
  .levels button.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
    font-weight: 600;
  }
  input[type='search'] {
    flex: 1;
    min-width: 140px;
  }
  .confirm {
    display: flex;
    flex-basis: 100%;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    font-size: 13px;
  }
  .list {
    overflow: hidden;
  }
  .entry + .entry {
    border-top: 1px solid var(--border);
  }
  button.head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    padding: 8px 14px;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--text);
    text-align: left;
    white-space: normal;
  }
  button.head:not(:disabled):hover {
    background: var(--surface-2);
  }
  button.head:disabled {
    cursor: default;
    opacity: 1;
  }
  .level {
    flex: none;
    width: 70px;
    padding: 0 6px;
    border-radius: 6px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 700;
    text-align: center;
    text-transform: uppercase;
  }
  .level.bad {
    background: var(--accent-soft);
    color: var(--danger);
  }
  .level.warn {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .level.quiet {
    background: transparent;
  }
  .time {
    flex: none;
    color: var(--muted);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
  }
  .message {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    overflow-wrap: anywhere;
  }
  pre {
    max-height: 420px;
    margin: 0 14px 10px;
    padding: 10px 12px;
    overflow: auto;
    border-radius: 8px;
    background: var(--surface-2);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    font-variant-ligatures: none;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .hint {
    margin: 0 14px 10px;
    color: var(--muted);
    font-size: 12px;
  }
  .empty {
    margin: 0;
    padding: 18px 16px;
    color: var(--muted);
  }
  .error {
    color: var(--danger);
  }
  .banner {
    margin: 0 0 12px;
  }
  input {
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  button.danger {
    border-color: var(--danger);
    background: var(--danger);
    color: var(--accent-text);
    font-weight: 600;
  }
</style>
