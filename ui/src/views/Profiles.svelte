<script lang="ts">
  import * as api from '../api';
  import type { ProfileReport } from '../api';
  import { formatReceived, formatSize } from '../format';
  import { createProfilesStore } from '../profiles.svelte';

  const store = createProfilesStore(api);
  store.load();

  // New profiles show up while the page is open.
  $effect(() => {
    const timer = setInterval(() => store.load(), 3000);
    return () => clearInterval(timer);
  });

  const label = (r: ProfileReport) => (r.cli ? r.uri : `${r.method} ${r.uri}`.trim());
  const ms = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(2)} s` : `${n} ms`);
</script>

<h1>Profiles</h1>

<section class="card strip">
  <span class="muted">
    Switch <strong>Profile</strong> on for a site on the Sites page: SPX then profiles each of its requests. Open a profile to see its flame graph and timeline in SPX.
  </span>
  <button onclick={() => store.clear()} disabled={store.reports.length === 0}>Clear</button>
</section>

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="card">
  {#if store.unavailable}
    <p class="empty">{store.unavailable}</p>
  {:else if store.reports.length === 0}
    <p class="empty">No profiles yet. Switch Profile on for a site, then open one of its pages.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Request</th>
          <th>Site</th>
          <th class="num">Time</th>
          <th class="num">Memory</th>
          <th class="num">Functions</th>
          <th>When</th>
          <th><span class="sr">Open</span></th>
        </tr>
      </thead>
      <tbody>
        {#each store.reports as r (r.key)}
          <tr>
            <td class="what" title={label(r)}>{label(r)}</td>
            <td class="muted">{r.cli ? 'command' : r.host}</td>
            <td class="num">{ms(r.wallMs)}</td>
            <td class="num">{formatSize(r.peakMemory)}</td>
            <td class="num">{r.functions.toLocaleString('en')}</td>
            <td class="muted">{formatReceived(r.time)}</td>
            <td class="open">
              <button onclick={() => store.open(r)} disabled={!store.reportUrl(r)} title={store.reportUrl(r) ? 'Open in SPX' : 'No site to open it through'}>Open</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  h1 {
    margin: 0 0 16px;
    font-size: 22px;
    font-weight: 650;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: auto;
  }
  .strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
    padding: 12px 16px;
    font-size: 13px;
  }
  .empty {
    margin: 0;
    padding: 24px 16px;
    color: var(--muted);
    font-size: 13px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    text-align: left;
    white-space: nowrap;
  }
  td {
    padding: 8px 12px;
    border-top: 1px solid var(--border);
    white-space: nowrap;
  }
  tbody tr:first-child td {
    border-top: 0;
  }
  tbody tr:hover {
    background: var(--surface-2);
  }
  .what {
    max-width: 420px;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    font-variant-ligatures: none;
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .open {
    text-align: right;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  .muted {
    color: var(--muted);
  }
  .error.banner {
    margin: 0 0 16px;
  }
</style>
