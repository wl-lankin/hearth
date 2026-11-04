<script lang="ts">
  import Select from '../Select.svelte';
  import * as api from '../api';
  import type { RequestFinding, RequestSummary } from '../api';
  import { shortPath } from '../dumps.svelte';
  import { formatReceived, formatSize } from '../format';
  import { createRequestsStore, rows, timeline } from '../requests.svelte';

  const store = createRequestsStore(api);
  let showAll = $state(false);
  // N+1 groups opened, by their first query's index.
  let opened = $state<number[]>([]);

  // `?request=<id>` opens a request directly (used for screenshots in the browser demo).
  const requested = Number(new URLSearchParams(location.search).get('request'));
  store.load().then(() => {
    if (requested) open(requested);
  });

  // New requests show up while the page is open.
  $effect(() => {
    const timer = setInterval(() => store.refresh(), 1000);
    return () => clearInterval(timer);
  });

  async function open(id: number) {
    showAll = false;
    opened = [];
    await store.open(id);
  }

  function pickSite(value: string) {
    store.filter = value === '*' ? undefined : value === '' ? null : value;
  }

  const label = (r: { method: string | null; uri: string | null; command: string | null }) =>
    r.command !== null ? `artisan ${r.command}`.trim() : `${r.method ?? ''} ${r.uri ?? ''}`.trim();
  const ms = (n: number) => (n >= 100 ? `${Math.round(n)} ms` : `${n.toFixed(1)} ms`);
  const kindLabel: Record<string, string> = { 'n+1': 'N+1', duplicate: 'Duplicate', slow: 'Slow' };

  const bars = $derived(store.current ? timeline(store.current.queries, store.current.ms) : []);
  /** For each query, the kind of the first finding it is part of. */
  const flagged = $derived.by(() => {
    const kinds: (RequestFinding['kind'] | null)[] = store.current ? store.current.queries.map(() => null) : [];
    for (const f of store.current?.findings ?? []) for (const i of f.queries) kinds[i] ??= f.kind;
    return kinds;
  });
  // N+1 groups fold into their first query until opened; long lists show 50 rows first.
  const allRows = $derived(store.current ? rows(store.current.queries.length, store.current.findings, new Set(opened)) : []);
  const shown = $derived(showAll ? allRows : allRows.slice(0, 50));
  const statusClass = (s: RequestSummary['status']) => (s === null ? '' : s >= 500 ? 'bad' : s >= 400 ? 'warn' : 'ok');
  const siteValue = $derived(store.filter === undefined ? '*' : (store.filter ?? ''));
</script>

<h1>Requests</h1>

<section class="card strip">
  <div class="status">
    <span class="dot" class:ok={store.status?.listening && store.status?.requests} class:bad={!!store.status?.error} aria-hidden="true"></span>
    {#if store.status?.error}
      <span class="error">{store.status.error}</span>
    {:else if !store.status?.requests}
      <span class="muted">Recording is off: Laravel apps run without Hearth watching their queries.</span>
    {:else}
      <span>Laravel requests and short artisan commands, with their queries. N+1 loops, duplicates and slow queries are pointed out.</span>
    {/if}
  </div>
  <button
    class="toggle"
    class:on={store.status?.requests}
    aria-pressed={!!store.status?.requests}
    onclick={() => store.setEnabled(!store.status?.requests)}
  >
    {store.status?.requests ? 'On' : 'Off'}
  </button>
</section>

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="split" class:reading={!!store.current}>
  <div class="card list" aria-label="Requests">
    <div class="list-head">
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
      <button onclick={() => store.clear()} disabled={store.requests.length === 0}>Clear</button>
    </div>
    {#if store.requests.length === 0}
      <p class="empty">No requests yet. Open a page of a Laravel site, or run an artisan command.</p>
    {/if}
    <ul>
      {#each store.requests as r (r.id)}
        <li>
          <button class="row" class:selected={store.current?.id === r.id} onclick={() => open(r.id)}>
            <span class="line">
              {#if r.status !== null}<span class="code {statusClass(r.status)}">{r.status}</span>{/if}
              <span class="what">{label(r)}</span>
              <span class="when">{formatReceived(Math.floor(r.time / 1000))}</span>
            </span>
            <span class="line small">
              <span class="muted">{ms(r.ms)}</span>
              <span class="muted">{r.queryCount} {r.queryCount === 1 ? 'query' : 'queries'}</span>
              {#each r.findings as kind (kind)}<span class="badge {kind === 'n+1' ? 'nplus' : kind}">{kindLabel[kind]}</span>{/each}
              {#if store.filter === undefined && r.site}<span class="tag">{r.site}</span>{/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </div>

  <div class="card detail">
    {#if store.current}
      {@const r = store.current}
      <header>
        <div class="title">
          <button class="back" onclick={() => store.close()}>Back</button>
          {#if r.status !== null}<span class="code {statusClass(r.status)}">{r.status}</span>{/if}
          <h2>{label(r)}</h2>
        </div>
        <p class="facts">
          <span>{ms(r.ms)}</span>
          <span>{r.queries.length} {r.queries.length === 1 ? 'query' : 'queries'} in {ms(r.queries.reduce((n, q) => n + q.ms, 0))}</span>
          <span>{formatSize(r.memory)} memory</span>
          {#if r.site}<span>{r.site}</span>{/if}
          {#if r.dropped > 0}<span class="warn-text">{r.dropped} more queries not kept</span>{/if}
        </p>
      </header>
      <div class="body">
        {#if r.findings.length > 0}
          <ul class="findings">
            {#each r.findings as f, i (i)}
              <li><span class="badge {f.kind === 'n+1' ? 'nplus' : f.kind}">{kindLabel[f.kind]}</span> <span>{f.text}</span></li>
            {/each}
          </ul>
        {/if}
        {#if r.queries.length > 0}
          <div class="track" aria-label="Timeline">
            {#each bars as bar, i (i)}
              <span
                class="mark {flagged[i] === 'n+1' ? 'nplus' : (flagged[i] ?? '')}"
                style="left: {bar.left}%; width: {bar.width}%"
                title="{ms(r.queries[i].ms)}: {r.queries[i].sql}"
              ></span>
            {/each}
          </div>
          <p class="scale"><span>0 ms</span><span>{ms(Math.max(r.ms, ...r.queries.map((q) => q.startMs + q.ms)))}</span></p>
          <ol class="queries">
            {#each shown as row (row.index)}
              {@const q = r.queries[row.index]}
              {@const i = row.index}
              <li class:flag={flagged[i] !== null}>
                <div class="q-head">
                  <span class="muted">#{i + 1}</span>
                  <span class="q-ms" class:slow={q.ms >= 100}>{ms(q.ms)}</span>
                  {#if flagged[i]}<span class="badge {flagged[i] === 'n+1' ? 'nplus' : flagged[i]}">{kindLabel[flagged[i] ?? '']}</span>{/if}
                  <span class="where" title={q.file ?? ''}>{shortPath(q.file, r.site)}{q.line ? `:${q.line}` : ''}</span>
                </div>
                <code class="sql">{q.sql}</code>
                {#if q.bindings.length > 0}<code class="bindings">{q.bindings.join(', ')}</code>{/if}
                {#if row.more > 0}
                  <button class="more" onclick={() => (opened = [...opened, i])}>
                    {row.more} more like this
                  </button>
                {/if}
              </li>
            {/each}
          </ol>
          {#if allRows.length > shown.length}
            <button class="more" onclick={() => (showAll = true)}>Show all {allRows.length} rows</button>
          {/if}
        {:else}
          <p class="empty">No queries.</p>
        {/if}
      </div>
    {:else}
      <p class="empty">Pick a request to see its queries.</p>
    {/if}
  </div>
</section>

<style>
  h1 {
    margin: 0 0 16px;
    font-size: 22px;
    font-weight: 650;
  }
  h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 15px;
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  .strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
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
  .split {
    display: grid;
    grid-template-columns: minmax(260px, 360px) minmax(0, 1fr);
    gap: 16px;
    height: max(440px, calc(100vh - 200px));
  }
  .list,
  .detail {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .list-head {
    display: flex;
    gap: 6px;
    padding: 10px;
    border-bottom: 1px solid var(--border);
  }
  .list-head .pick {
    flex: 1;
    min-width: 0;
  }
  ul,
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .list ul {
    overflow: auto;
  }
  .list li + li {
    border-top: 1px solid var(--border);
  }
  button.row {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 100%;
    padding: 9px 12px;
    border: 0;
    border-radius: 0;
    background: transparent;
    text-align: left;
    white-space: normal;
  }
  button.row:hover {
    background: var(--surface-2);
  }
  button.row.selected {
    background: var(--nav-active);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .line.small {
    flex-wrap: wrap;
    font-size: 12px;
  }
  .what {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when {
    flex: none;
    color: var(--muted);
    font-size: 12px;
  }
  .code {
    flex: none;
    padding: 0 6px;
    border-radius: 6px;
    background: var(--surface-2);
    font-size: 11px;
    font-weight: 700;
  }
  .code.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }
  .code.warn {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .code.bad {
    background: var(--accent-soft);
    color: var(--danger);
  }
  .badge {
    flex: none;
    padding: 0 7px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
  }
  .badge.nplus {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .badge.duplicate {
    background: var(--surface-2);
    color: var(--text);
  }
  .badge.slow {
    background: var(--accent-soft);
    color: var(--danger);
  }
  .tag {
    padding: 0 7px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  header {
    padding: 14px 16px 10px;
    border-bottom: 1px solid var(--border);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .back {
    display: none;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 8px 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  .warn-text {
    color: var(--danger);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px 16px 16px;
  }
  .findings {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 14px;
    font-size: 13px;
  }
  .findings li {
    overflow-wrap: anywhere;
  }
  .track {
    position: relative;
    height: 22px;
    border-radius: 6px;
    background: var(--surface-2);
  }
  .mark {
    position: absolute;
    top: 4px;
    bottom: 4px;
    border-radius: 2px;
    background: var(--muted);
  }
  .mark.nplus {
    background: var(--accent);
  }
  .mark.duplicate {
    background: var(--text);
  }
  .mark.slow {
    background: var(--danger);
  }
  .scale {
    display: flex;
    justify-content: space-between;
    margin: 4px 0 14px;
    color: var(--muted);
    font-size: 11px;
  }
  .queries li {
    padding: 8px 0;
    border-top: 1px solid var(--border);
  }
  .q-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 10px;
    font-size: 12px;
  }
  .q-ms.slow {
    color: var(--danger);
    font-weight: 700;
  }
  .where {
    margin-left: auto;
    color: var(--muted);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    overflow-wrap: anywhere;
  }
  .sql,
  .bindings {
    display: block;
    margin-top: 4px;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    font-variant-ligatures: none;
    overflow-wrap: anywhere;
  }
  .bindings {
    color: var(--muted);
  }
  .more {
    margin-top: 8px;
  }
  .empty {
    margin: 0;
    padding: 18px 16px;
    color: var(--muted);
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
  /* Narrow windows: the list, or the open request with a way back. */
  @media (max-width: 1000px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
    }
    .split.reading .list,
    .split:not(.reading) .detail {
      display: none;
    }
    .back {
      display: inline-block;
    }
  }
</style>
