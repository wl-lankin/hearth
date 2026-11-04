<script lang="ts">
  import Select from '../Select.svelte';
  import * as api from '../api';
  import { createDumpsStore, fold, shortPath } from '../dumps.svelte';
  import { formatReceived } from '../format';

  const store = createDumpsStore(api);
  let port = $state<number | null>(null);
  let open = $state<Record<number, boolean>>({});

  store.load().then(() => (port = store.status?.port ?? null));

  // New dumps show up while the page is open.
  $effect(() => {
    const timer = setInterval(() => store.refresh(), 1000);
    return () => clearInterval(timer);
  });

  function pickSite(value: string) {
    store.filter = value === '*' ? undefined : value === '' ? null : value;
  }

  async function changePort(event: SubmitEvent) {
    event.preventDefault();
    if (port) await store.setPort(port);
  }

  const siteValue = $derived(store.filter === undefined ? '*' : (store.filter ?? ''));
</script>

<section class="card strip">
  <div class="status">
    <span class="dot" class:ok={store.status?.listening && store.status?.enabled} class:bad={!!store.status?.error} aria-hidden="true"></span>
    {#if store.status?.error}
      <span class="error">{store.status.error}</span>
    {:else if !store.status?.enabled}
      <span class="muted">Dumps are off: PHP runs without Hearth's dump script.</span>
    {:else if store.status?.listening}
      <span>Every <code>dump()</code> and <code>dd()</code> shows here too, from sites and the command line.</span>
    {:else}
      <span class="muted">The dump collector is not running.</span>
    {/if}
  </div>
  <div class="tools">
    <form class="port" onsubmit={changePort}>
      <label for="dump-port">Port</label>
      <input id="dump-port" type="number" min="1" max="65535" bind:value={port} required />
      <button type="submit" disabled={!port || (port === store.status?.port && store.status?.listening)}>Use</button>
    </form>
    <button
      class="toggle"
      class:on={store.status?.enabled}
      aria-pressed={!!store.status?.enabled}
      onclick={() => store.setEnabled(!store.status?.enabled)}
    >
      {store.status?.enabled ? 'On' : 'Off'}
    </button>
  </div>
  {#if store.portError}<p class="error wide" role="alert">{store.portError}</p>{/if}
</section>

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="bar">
  <span class="pick">
    <Select
      label="Site"
      value={siteValue}
      onchange={pickSite}
      options={[
        { value: '*', label: `All sites (${store.sites.reduce((n, s) => n + s.count, 0)})` },
        ...store.sites.map((s) => ({ value: s.site ?? '', label: `${s.label} (${s.count})` })),
      ]}
    />
  </span>
  <button onclick={() => store.clear()} disabled={store.dumps.length === 0}>Clear</button>
</section>

{#if store.dumps.length === 0}
  <p class="card empty">No dumps yet. Call <code>dump($value)</code> in a site or a command and it shows up here.</p>
{/if}

{#each store.dumps as d (d.id)}
  {@const folded = fold(d.text)}
  <article class="card dump">
    <header>
      {#if d.site}<span class="tag">{d.site}</span>{/if}
      <span class="where" title={d.file ?? ''}>{shortPath(d.file, d.site)}{d.line ? `:${d.line}` : ''}</span>
      <span class="muted from">{d.uri ?? (d.sapi === 'cli' ? 'Command line' : '')}</span>
      <span class="muted when">{formatReceived(Math.floor(d.time / 1000))}</span>
    </header>
    <pre>{open[d.id] ? d.text : folded.text}</pre>
    {#if folded.hidden > 0}
      <button class="more" onclick={() => (open[d.id] = !open[d.id])}>
        {open[d.id] ? 'Show less' : `Show all (${folded.hidden} more lines)`}
      </button>
    {/if}
    {#if d.truncated}<p class="hint">This dump was cut at 256 KB.</p>{/if}
  </article>
{/each}

<style>
  .card {
    max-width: 960px;
    margin-bottom: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  .strip {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px 16px;
    margin-bottom: 16px;
    padding: 12px 16px;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 13px;
  }
  .tools,
  .port {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .port label {
    color: var(--muted);
    font-size: 13px;
  }
  .port input {
    width: 84px;
  }
  .wide {
    flex-basis: 100%;
    margin: 0;
  }
  .bar {
    display: flex;
    gap: 6px;
    max-width: 960px;
    margin-bottom: 12px;
  }
  .bar .pick {
    flex: 1;
    min-width: 0;
    max-width: 320px;
  }
  .dump {
    overflow: hidden;
  }
  .dump header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
    padding: 9px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  .where {
    min-width: 0;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .from {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when {
    flex: none;
    font-size: 12px;
  }
  pre {
    margin: 0;
    padding: 12px 14px;
    overflow-x: auto;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.45;
    /* A dump shows the characters as they are: "=>" stays "=>". */
    font-variant-ligatures: none;
  }
  .more {
    margin: 0 14px 12px;
  }
  .hint {
    margin: 0 14px 12px;
    color: var(--muted);
    font-size: 12px;
  }
  .empty {
    padding: 18px 16px;
    color: var(--muted);
  }
  .tag {
    padding: 0 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.bad {
    background: var(--danger);
  }
  .error {
    color: var(--danger);
  }
  .banner {
    margin: 0 0 12px;
  }
  .muted {
    color: var(--muted);
  }
  code {
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
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
  button.toggle.on {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }
</style>
