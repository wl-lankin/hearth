<script lang="ts">
  import Select from '../Select.svelte';
  import * as api from '../api';
  import type { Workspace } from '../api';
  import { createWorkspacesStore } from '../workspaces.svelte';

  const store = createWorkspacesStore(api);
  store.load();

  let keepDatabase = $state(false);
  let keepFiles = $state(false);

  const databases = [
    { value: 'shared', label: "The parent's database" },
    { value: 'clone', label: 'Its own copy' },
    { value: 'empty', label: 'Its own, empty (migrated)' },
  ];

  const dbText = (w: Workspace) =>
    w.databaseMode === 'clone' ? `own copy ${w.databaseName ?? ''}` : w.databaseMode === 'empty' ? `own, empty ${w.databaseName ?? ''}` : 'parent database';

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    await store.create();
  }

  function ask(site: string) {
    keepDatabase = false;
    keepFiles = false;
    store.askDelete(site);
  }
</script>

<h1>Workspaces</h1>

<form class="card new" onsubmit={submit}>
  <div class="field site">
    <span>Branch from</span>
    <Select
      label="Site to branch from"
      value={store.form.site}
      onchange={(v) => (store.form.site = v)}
      options={store.parents.map((p) => ({ value: p, label: `${p}.test` }))}
      disabled={store.parents.length === 0}
    />
  </div>
  <label class="field name">
    <span>Branch</span>
    <input bind:value={store.form.name} placeholder="Empty: a random name" autocomplete="off" spellcheck="false" />
  </label>
  <div class="field db">
    <span>Database</span>
    <Select label="Database" value={store.form.database} onchange={(v) => (store.form.database = v)} options={databases} />
  </div>
  <button class="primary" type="submit" disabled={!store.form.site || store.creating}>
    {store.creating ? 'Making...' : 'New workspace'}
  </button>
  {#if store.formError}<p class="error wide">{store.formError}</p>{/if}
  <p class="hint wide">
    A workspace is the branch in its own git worktree, served at <code>branch.site.test</code> with its own <code>.env</code>. Installed
    dependencies are copied when the lockfiles match.
  </p>
</form>

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}
{#if store.message}
  <div class="card done" role="status">
    <strong>{store.message}</strong>
    {#each store.notes as note (note)}<span class="muted">{note}</span>{/each}
  </div>
{/if}

{#if store.groups.length === 0}
  <p class="empty">No workspaces yet. Make one to work on a branch next to the main checkout.</p>
{/if}

{#each store.groups as group (group.site)}
  <section class="card group" aria-label="Workspaces of {group.site}.test">
    <h2>{group.site}.test</h2>
    {#each group.workspaces as w (w.site)}
      <div class="row">
        <div class="what">
          <span class="line">
            <a href={w.url} onclick={(e) => { e.preventDefault(); store.open(w); }}>{w.site}.test</a>
            <span class="tag">{w.branch}</span>
            <span class="tag db" class:own={w.databaseMode !== 'shared'}>{dbText(w)}</span>
          </span>
          <span class="path" title={w.path}>{w.path}</span>
          {#if store.rowError(w.site)}<span class="error" role="alert">{store.rowError(w.site)}</span>{/if}
        </div>
        <div class="actions">
          <button onclick={() => store.open(w)}>Open</button>
          <button onclick={() => store.merge(w.site)} disabled={store.busy === w.site} title="Merge {w.branch} into {w.baseBranch}">
            Merge into {w.baseBranch}
          </button>
          <button class="danger" onclick={() => ask(w.site)} disabled={store.busy === w.site}>Delete</button>
        </div>
        {#if store.deleting === w.site}
          <div class="confirm" role="alertdialog" aria-label="Delete {w.site}.test">
            <span>Delete <strong>{w.site}.test</strong>? Uncommitted changes in it are lost; the branch <code>{w.branch}</code> stays.</span>
            {#if w.databaseMode !== 'shared' && w.databaseDriver !== 'sqlite'}
              <label class="check"><input type="checkbox" bind:checked={keepDatabase} /> Keep its database {w.databaseName}</label>
            {/if}
            {#if !w.owned}
              <label class="check"><input type="checkbox" bind:checked={keepFiles} /> Keep the worktree folder</label>
            {/if}
            <span class="buttons">
              <button onclick={() => store.cancelDelete()}>Cancel</button>
              <button class="danger solid" onclick={() => store.confirmDelete({ keepDatabase, keepFiles })}>Delete</button>
            </span>
          </div>
        {/if}
      </div>
    {/each}
  </section>
{/each}

<style>
  h1 {
    margin: 0 0 16px;
    font-size: 22px;
    font-weight: 650;
  }
  h2 {
    margin: 0;
    padding: 14px 20px 8px;
    color: var(--muted);
    font-size: 13px;
    font-weight: 650;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  .new {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr) 220px auto;
    gap: 10px 12px;
    align-items: end;
    margin-bottom: 16px;
    padding: 14px 16px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .field > span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }
  .field input {
    width: 100%;
  }
  .wide {
    grid-column: 1 / -1;
    margin: 0;
  }
  .hint {
    color: var(--muted);
    font-size: 12px;
  }
  .done {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 16px;
    padding: 12px 16px;
    border-color: var(--ok);
    font-size: 13px;
  }
  .group {
    margin-bottom: 16px;
    padding-bottom: 4px;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px 16px;
    align-items: center;
    padding: 10px 20px;
    border-top: 1px solid var(--border);
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .line a {
    color: var(--text);
    font-weight: 600;
    text-decoration: none;
  }
  .line a:hover {
    color: var(--accent);
    text-decoration: underline;
  }
  .tag {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }
  .tag.db.own {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .path {
    color: var(--muted);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  button.danger {
    color: var(--danger);
  }
  button.danger.solid {
    border-color: var(--danger);
    background: var(--danger);
    color: var(--accent-text);
    font-weight: 600;
  }
  .confirm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
    grid-column: 1 / -1;
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 8px;
    font-size: 13px;
  }
  .confirm .buttons {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .check input {
    accent-color: var(--accent);
  }
  code {
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
  }
  .empty {
    color: var(--muted);
    font-size: 13px;
  }
  .muted {
    color: var(--muted);
  }
  .error {
    color: var(--danger);
    font-size: 13px;
  }
  .banner {
    margin: 0 0 16px;
  }
  @media (max-width: 900px) {
    .new {
      grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    }
    .row {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
