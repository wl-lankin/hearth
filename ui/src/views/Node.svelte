<script lang="ts">
  import * as api from '../api';
  import { createNodeStore, type NodeRow } from '../node.svelte';

  const node = createNodeStore(api);
  let confirming = $state<string | null>(null);
  let spec = $state('');

  node.load();

  // Follow running installs; stop polling once none runs.
  $effect(() => {
    if (!node.busy) return;
    const timer = setInterval(() => node.poll(), 500);
    return () => clearInterval(timer);
  });

  function stateLine(row: NodeRow): string {
    if (row.job) return `${row.job.message} (${Math.round(row.job.progress * 100)} %)`;
    if (!row.installedVersion) return `${row.version} available`;
    if (row.update) return `Installed ${row.installedVersion}, ${row.update} available`;
    return `Installed ${row.installedVersion}`;
  }
</script>

<p class="intro">
  <code>node</code>, <code>npm</code>, <code>npx</code> and <code>corepack</code> run the version a project's
  <code>.nvmrc</code> or <code>.node-version</code> asks for, and the default everywhere else.
</p>

{#if node.catalogError}
  <p class="notice">Could not load the list of Node versions: {node.catalogError}</p>
{/if}

<section class="card list" aria-label="Node versions">
  {#each node.rows as row (row.key)}
    <div class="row" class:installed={!!row.installedVersion}>
      <div class="name">
        <strong>Node {row.major}</strong>
        {#if row.lts}<span class="tag">LTS {row.lts}</span>{/if}
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
      <div class="actions">
        {#if row.job}
          <span class="muted">Installing</span>
        {:else if row.installedVersion && confirming === row.installedVersion}
          <span class="confirm">Remove Node {row.installedVersion} and its global npm packages?</span>
          <button class="danger" onclick={() => { const v = confirming!; confirming = null; node.uninstall(v); }}>Remove</button>
          <button onclick={() => (confirming = null)}>Keep</button>
        {:else if !row.installedVersion}
          <button onclick={() => node.install(row.major)}>Install</button>
        {:else}
          {#if row.update}
            <button class="primary" onclick={() => node.install(row.major)}>Install {row.update}</button>
          {/if}
          {#if !row.isDefault}
            <button onclick={() => node.makeDefault(row.installedVersion!)}>Make default</button>
          {/if}
          <button onclick={() => (confirming = row.installedVersion)}>Remove</button>
        {/if}
      </div>
    </div>
  {/each}
</section>

<form
  class="card other"
  onsubmit={(e) => {
    e.preventDefault();
    node.installSpec(spec);
  }}
>
  <label for="node-spec">Install another version</label>
  <div class="field">
    <input id="node-spec" bind:value={spec} placeholder="20.11.1 or lts/hydrogen" spellcheck="false" autocomplete="off" disabled={!!node.typed.job} />
    <button class="primary" type="submit" disabled={!!node.typed.job}>Install</button>
  </div>
  {#if node.typed.job}
    <span>{node.typed.job.message} ({Math.round(node.typed.job.progress * 100)} %)</span>
    <span class="bar" role="progressbar" aria-valuenow={Math.round(node.typed.job.progress * 100)} aria-valuemin="0" aria-valuemax="100">
      <span style:width="{node.typed.job.progress * 100}%"></span>
    </span>
  {:else if node.typed.error}
    <span class="error" role="alert">{node.typed.error}</span>
  {:else}
    <span class="muted">Versions the list above does not offer come from nodejs.org, checked against its published checksums.</span>
  {/if}
</form>

<style>

  .intro {
    max-width: 900px;
    margin: 0 0 20px;
    color: var(--muted);
    font-size: 13px;
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

  .other {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px 20px;
    font-size: 13px;
  }

  .other label {
    font-weight: 600;
  }

  .field {
    display: flex;
    gap: 8px;
    max-width: 420px;
  }

  .field input {
    flex: 1;
    min-width: 0;
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: inherit;
    font: inherit;
  }

  .field input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .row {
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr) auto;
    gap: 2px 16px;
    align-items: center;
    padding: 12px 20px;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  .name {
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
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    font-size: 13px;
  }

  .actions {
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

  code {
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
  }

  @media (max-width: 900px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .state {
      grid-column: 1 / -1;
      grid-row: 2;
    }
  }
</style>
