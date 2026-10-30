<script lang="ts">
  import * as api from '../api';
  import { createSitesStore } from '../sites.svelte';

  const store = createSitesStore(api);
  let adding = $state<'link' | 'proxy' | null>(null);
  let name = $state('');
  let target = $state('');
  let menu = $state<string | null>(null);

  store.load();

  // The web server follows changes in the background; show its state as it settles.
  $effect(() => {
    const timer = setInterval(() => store.refreshStatus(), 2000);
    return () => clearInterval(timer);
  });

  async function parkFolder() {
    const dir = await api.pickFolder();
    if (dir) await store.park(dir);
  }

  async function chooseLinkFolder() {
    const dir = await api.pickFolder();
    if (dir) {
      target = dir;
      if (!name) name = dir.split(/[\\/]/).pop()?.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') ?? '';
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (adding === 'link') await store.link(name, target);
    else if (adding === 'proxy') await store.proxy(name, target);
    if (!store.error) {
      adding = null;
      name = '';
      target = '';
    }
  }

  const kinds = { parked: 'Parked', linked: 'Linked', proxy: 'Proxy' } as const;
</script>

<h1>Sites</h1>

<section class="server" aria-live="polite">
  <span class="dot" class:ok={store.server.kind === 'running'} class:bad={store.server.kind === 'error'} aria-hidden="true"></span>
  <span class:error={store.server.kind === 'error'} class:muted={store.server.kind !== 'running' && store.server.kind !== 'error'}>
    {store.server.text}
  </span>
  {#if store.portSuggestion}
    <button onclick={() => store.setPort(store.portSuggestion!)}>Use port {store.portSuggestion}</button>
  {/if}
  <span class="actions">
    <button onclick={parkFolder}>Park a folder</button>
    <button onclick={() => (adding = adding === 'link' ? null : 'link')}>Link a folder</button>
    <button onclick={() => (adding = adding === 'proxy' ? null : 'proxy')}>Add a proxy</button>
  </span>
</section>

{#if store.setup === 'dns'}
  <section class="card setup" aria-live="polite">
    <div class="setup-text">
      <strong>Set up .test names</strong>
      <span class="muted">So site names like shop.test open in the browser. The system asks for admin rights once.</span>
    </div>
    <button class="primary" onclick={() => store.setupDns()}>Set up</button>
  </section>
{:else if store.setup === 'trust'}
  <section class="card setup" aria-live="polite">
    <div class="setup-text">
      <strong>Browsers do not trust Hearth yet</strong>
      <span class="muted">Trust Hearth's certificates once, then HTTPS sites open without warnings.</span>
    </div>
    <button class="primary" onclick={() => store.trust()}>Trust</button>
  </section>
{/if}

{#if adding}
  <form class="card add" onsubmit={submit}>
    <label for="site-name">Name</label>
    <span class="name-field"><input id="site-name" bind:value={name} placeholder="shop" required /><span class="suffix">.test</span></span>
    {#if adding === 'link'}
      <label for="site-folder">Folder</label>
      <span class="folder-field">
        <input id="site-folder" bind:value={target} placeholder="C:\Projects\shop" required />
        <button type="button" onclick={chooseLinkFolder}>Choose</button>
      </span>
    {:else}
      <label for="site-upstream">Forward to</label>
      <input id="site-upstream" bind:value={target} placeholder="http://localhost:5173" required />
    {/if}
    <span class="form-actions">
      <button type="button" onclick={() => (adding = null)}>Cancel</button>
      <button class="primary" type="submit">{adding === 'link' ? 'Link' : 'Add proxy'}</button>
    </span>
  </form>
{/if}

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="card list" aria-label="Sites">
  {#if store.sites.length === 0}
    <p class="empty">No sites yet. Park a folder to serve every project in it, or link a single project.</p>
  {/if}
  {#each store.sites as site (site.name)}
    <div class="row">
      <div class="name">
        {#if site.problem}
          <strong>{site.name}.test</strong>
        {:else}
          <a href={store.url(site)} onclick={(e) => { e.preventDefault(); api.openUrl(store.url(site)); }}>{site.name}.test</a>
        {/if}
        <span class="tag">{kinds[site.kind]}</span>
      </div>
      <div class="where">
        <span class="path" title={site.path ?? site.upstream ?? ''}>{site.path ?? site.upstream}</span>
        {#if site.problem}<span class="error">{site.problem}</span>{/if}
        {#if store.note(site.name)}<span class="muted">{store.note(site.name)}</span>{/if}
        {#if store.node(site.name)}
          {@const node = store.node(site.name)!}
          <span class={node.problem ? 'error' : 'muted'} title={node.problem ?? ''}>{node.text}</span>
        {/if}
        {#if store.errors[site.name]}<span class="error" role="alert">{store.errors[site.name]}</span>{/if}
      </div>
      <div class="php">
        {#if site.kind !== 'proxy'}
          <label class="sr-only" for="php-{site.name}">PHP for {site.name}</label>
          <select id="php-{site.name}" value={site.php ?? ''} onchange={(e) => store.isolate(site.name, e.currentTarget.value || null)}>
            <option value="">Default{store.defaultPhp ? ` (${store.defaultPhp})` : ''}</option>
            {#each store.phpChoices as minor (minor)}<option value={minor}>PHP {minor}</option>{/each}
            {#if site.php && !store.phpChoices.includes(site.php)}
              <option value={site.php}>PHP {site.php} (not installed)</option>
            {/if}
          </select>
        {/if}
      </div>
      <div class="more">
        <button
          class="secure"
          class:on={site.secured}
          aria-pressed={site.secured}
          title={site.secured ? 'Served over HTTPS; click to use plain HTTP' : 'Click to serve over HTTPS'}
          onclick={() => store.secure(site.name, !site.secured)}>HTTPS</button
        >
        {#if site.kind === 'linked'}
          <button onclick={() => store.unlink(site.name)}>Unlink</button>
        {:else if site.kind === 'proxy'}
          <button onclick={() => store.unproxy(site.name)}>Remove</button>
        {/if}
      </div>
    </div>
  {/each}
</section>

{#if store.parked.length > 0}
  <section class="card parked">
    <h2>Parked folders</h2>
    {#each store.parked as dir (dir)}
      <div class="parked-row">
        <span class="path" title={dir}>{dir}</span>
        <button onclick={() => store.unpark(dir)}>Stop parking</button>
      </div>
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

  .server {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 10px;
    max-width: 960px;
    margin-bottom: 16px;
  }

  .actions {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }

  .dot {
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

  .name-field,
  .folder-field {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .suffix {
    color: var(--muted);
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

  .php select {
    width: 100%;
  }

  .folder-field input,
  .name-field input,
  #site-upstream {
    flex: 1;
  }

  .list {
    padding: 4px 0;
  }

  .setup {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 20px;
  }

  .setup-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .more {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }

  button.secure {
    color: var(--muted);
  }

  button.secure.on {
    border-color: var(--ok);
    background: var(--ok);
    color: var(--accent-text);
    font-weight: 600;
  }

  .row {
    display: grid;
    grid-template-columns: minmax(150px, 220px) minmax(0, 1fr) 190px 150px;
    gap: 6px 16px;
    align-items: center;
    padding: 10px 20px;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  .name {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .name a {
    color: var(--text);
    font-weight: 600;
    text-decoration: none;
  }

  .name a:hover {
    color: var(--accent);
    text-decoration: underline;
  }

  .where {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    font-size: 13px;
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12.5px;
  }

  .tag {
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
  }

  .parked {
    padding: 16px 20px;
  }

  .parked-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 36px;
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

  .error {
    color: var(--danger);
  }

  .banner {
    max-width: 960px;
    margin: 0 0 12px;
  }

  .muted {
    color: var(--muted);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }

  @media (max-width: 860px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .where {
      grid-column: 1 / -1;
      grid-row: 2;
    }

    .more {
      grid-column: 1 / -1;
    }
  }
</style>
