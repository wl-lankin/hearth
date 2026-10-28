<script lang="ts">
  import * as api from '../api';
  import { createServicesStore } from '../services.svelte';

  const store = createServicesStore(api);
  let adding = $state(false);
  let logsFor = $state<string | null>(null);
  let logText = $state('');
  let copied = $state<string | null>(null);

  store.load();

  // Starting and stopping happen in the background; show the state as it settles.
  $effect(() => {
    const timer = setInterval(() => store.refresh(), 2000);
    return () => clearInterval(timer);
  });

  async function pick(event: Event) {
    const kind = (event.currentTarget as HTMLSelectElement).value;
    if (kind) await store.begin(kind);
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    await store.create();
    if (!store.draft) adding = false;
  }

  async function showLog(id: string) {
    logsFor = logsFor === id ? null : id;
    if (logsFor) logText = await store.log(id);
  }

  async function copyEnv(id: string) {
    await store.copyEnv(id);
    copied = id;
    setTimeout(() => (copied = null), 1500);
  }
</script>

<h1>Services</h1>

<section class="bar">
  <span class="muted">Databases, Redis, search and S3 storage, on this computer only.</span>
  <button class="primary" onclick={() => { adding = !adding; store.cancel(); }}>Add a service</button>
</section>

{#if adding}
  <form class="card add" onsubmit={submit}>
    <label for="svc-kind">Service</label>
    <select id="svc-kind" value={store.draft?.kind ?? ''} onchange={pick} required>
      <option value="" disabled>Choose a service</option>
      {#each store.kinds as k (k.kind)}<option value={k.kind}>{k.name}</option>{/each}
    </select>
    {#if store.draft}
      <label for="svc-line">Version</label>
      <select id="svc-line" bind:value={store.draft.line}>
        {#each store.lines(store.draft.kind) as line (line)}<option value={line}>{line === 'current' ? 'Current' : line}</option>{/each}
      </select>
      <label for="svc-port">Port</label>
      <input id="svc-port" type="number" min="1" max="65535" bind:value={store.draft.port} required />
      <label for="svc-name">Name</label>
      <input id="svc-name" bind:value={store.draft.name} placeholder="Optional" />
    {/if}
    {#if store.createError}
      <p class="error wide" role="alert">
        {store.createError}
        {#if store.freePort}<button type="button" onclick={() => store.useFreePort()}>Use port {store.freePort}</button>{/if}
      </p>
    {/if}
    <span class="form-actions">
      <button type="button" onclick={() => { adding = false; store.cancel(); }}>Cancel</button>
      <button class="primary" type="submit" disabled={!store.draft}>Create</button>
    </span>
  </form>
{/if}

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="card list" aria-label="Services">
  {#if store.services.length === 0}
    <p class="empty">No services yet. Add a database, Redis or search to use them from your sites.</p>
  {/if}
  {#each store.services as s (s.id)}
    <div class="row">
      <div class="name">
        <span class="dot" class:ok={s.running} class:bad={!s.running && !!(store.errors[s.id] ?? s.error)} aria-hidden="true"></span>
        <strong>{s.name}</strong>
        <span class="tag">port {s.port}</span>
      </div>
      <div class="state">
        {#if store.errors[s.id] ?? s.error}
          <span class="error">{store.errors[s.id] ?? s.error}</span>
        {:else}
          <span class="muted">{store.busy[s.id] ? 'Working...' : s.running ? 'Running' : 'Stopped'}</span>
        {/if}
      </div>
      <div class="actions">
        {#if s.running}
          <button onclick={() => store.stop(s.id)} disabled={store.busy[s.id]}>Stop</button>
        {:else}
          <button class="primary" onclick={() => store.start(s.id)} disabled={store.busy[s.id]}>Start</button>
        {/if}
        <button class="toggle" class:on={s.autostart} aria-pressed={s.autostart} title="Start with Hearth" onclick={() => store.setAutostart(s.id, !s.autostart)}>Autostart</button>
        <button onclick={() => copyEnv(s.id)}>{copied === s.id ? 'Copied' : 'Copy .env'}</button>
        <button onclick={() => showLog(s.id)} aria-expanded={logsFor === s.id}>Log</button>
        <button onclick={() => store.askDelete(s.id)}>Delete</button>
      </div>
      {#if store.confirming === s.id}
        <div class="confirm wide" role="alert">
          <span>Delete {s.name} and all its data?</span>
          <button onclick={() => store.cancelDelete()}>Cancel</button>
          <button class="danger" onclick={() => store.confirmDelete()}>Delete</button>
        </div>
      {/if}
      {#if logsFor === s.id}
        <pre class="log wide">{logText || 'Nothing logged yet.'}</pre>
      {/if}
    </div>
  {/each}
</section>

{#if store.unavailable.length > 0}
  <section class="card unavailable">
    <h2>Not available here</h2>
    {#each store.unavailable as u (u.kind)}
      <p><strong>{u.name}</strong> <span class="muted">{u.reason}</span></p>
    {/each}
  </section>
{/if}

<style>
  h1 {
    margin: 0 0 16px;
    font-size: 22px;
    font-weight: 650;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    max-width: 960px;
    margin-bottom: 16px;
  }

  .card {
    max-width: 960px;
    margin-bottom: 16px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  .add {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 8px 12px;
    align-items: center;
    padding: 16px 20px;
  }

  .wide {
    grid-column: 1 / -1;
  }

  .form-actions {
    grid-column: 1 / -1;
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }

  input,
  select {
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .list {
    padding: 4px 0;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(180px, 260px) minmax(0, 1fr) auto;
    gap: 8px 16px;
    align-items: center;
    padding: 10px 20px;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .state {
    min-width: 0;
    font-size: 13px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
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

  .tag {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }

  .confirm {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    font-size: 13px;
  }

  .log {
    max-height: 220px;
    margin: 0;
    padding: 10px 12px;
    overflow: auto;
    border-radius: 8px;
    background: var(--surface-2);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    white-space: pre-wrap;
  }

  .unavailable {
    padding: 14px 20px;
  }

  .unavailable p {
    margin: 4px 0;
  }

  .empty {
    margin: 0;
    padding: 18px 20px;
    color: var(--muted);
  }

  button {
    padding: 5px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
    cursor: pointer;
    white-space: nowrap;
  }

  button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 600;
  }

  button.danger {
    border-color: var(--danger);
    background: var(--danger);
    color: var(--accent-text);
    font-weight: 600;
  }

  button.toggle {
    color: var(--muted);
  }

  button.toggle.on {
    border-color: var(--ok);
    background: var(--ok);
    color: var(--accent-text);
    font-weight: 600;
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .error {
    color: var(--danger);
  }

  .error button {
    margin-left: 8px;
  }

  .banner {
    max-width: 960px;
    margin: 0 0 12px;
  }

  .muted {
    color: var(--muted);
  }

  @media (max-width: 860px) {
    .row {
      grid-template-columns: minmax(0, 1fr);
    }

    .actions {
      justify-content: flex-start;
    }
  }
</style>
