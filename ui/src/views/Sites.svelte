<script lang="ts">
  import { onMount } from 'svelte';
  import EmptyState from '../EmptyState.svelte';
  import { art } from '../illustrations';
  import Select from '../Select.svelte';
  import * as api from '../api';
  import { createSitesStore } from '../sites.svelte';

  /** startLinking: open with the link form, as Home's Get started does. */
  let { startLinking = false }: { startLinking?: boolean } = $props();

  const store = createSitesStore(api);
  let adding = $state<'link' | 'proxy' | null>(null);
  onMount(() => {
    if (startLinking) adding = 'link';
  });
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

{#if store.ide}
  <p class="ide"><span class="ide-dot" class:on={store.ideListening} aria-hidden="true"></span>{store.ide}</p>
{/if}

<section class="card list" aria-label="Sites">
  {#if store.sites.length === 0}
    <!-- A wrapper of this page: the list's grid rules below reach it, so it spans the card. -->
    <div class="empty">
      <EmptyState
        art={art.sites}
        title="No sites yet"
        line="Link a folder and Hearth serves it at name.test"
        action={{ label: 'Link a folder', onclick: () => (adding = 'link') }}
      />
    </div>
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
          <Select
            id="php-{site.name}"
            label="PHP for {site.name}"
            value={site.php ?? ''}
            onchange={(minor) => store.isolate(site.name, minor || null)}
            options={[
              { value: '', label: `Default${store.defaultPhp ? ` (${store.defaultPhp})` : ''}` },
              ...store.phpChoices.map((minor) => ({ value: minor, label: `PHP ${minor}` })),
              ...(site.php && !store.phpChoices.includes(site.php) ? [{ value: site.php, label: `PHP ${site.php} (not installed)` }] : []),
            ]}
          />
        {/if}
      </div>
      <div class="more">
        {#if site.kind !== 'proxy'}
          <button
            class="secure"
            class:on={site.debug}
            aria-pressed={site.debug}
            title={site.debug ? 'Every request starts an Xdebug session; click to stop' : 'Click to start an Xdebug session with every request'}
            onclick={() => store.debug(site.name, !site.debug)}>Debug</button
          >
        {/if}
        {#if store.canProfile(site)}
          <button
            class="secure"
            class:on={site.profile}
            aria-pressed={site.profile}
            title={site.profile ? 'SPX profiles every request; click to stop' : 'Click to let SPX profile every request (see Profiles)'}
            onclick={() => store.profile(site.name, !site.profile)}>Profile</button
          >
        {/if}
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

  input {
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
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

  /* On: a soft ember light, not a solid block. */
  button.secure.on {
    border-color: rgb(240 122 64 / 0.5);
    background: linear-gradient(180deg, rgb(240 122 64 / 0.3), rgb(240 122 64 / 0.16));
    color: var(--text);
    font-weight: 600;
  }

  /* Name and controls on top, the full path and its notes below: nothing is cut off. */
  /* One grid for the whole list (rows are subgrids): the PHP pickers and buttons line up, and the
     picker column is as wide as its longest choice, so "PHP 8.3 (not installed)" is never cut. */
  .list.card {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(200px, max-content) auto;
  }

  .list.card > :not(.row) {
    grid-column: 1 / -1;
  }

  .empty {
    display: flex;
    justify-content: center;
  }

  .row {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    grid-template-areas:
      'name php more'
      'where where where';
    gap: 6px 14px;
    align-items: center;
    padding: 12px 20px;
  }

  .name {
    grid-area: name;
  }

  .php {
    grid-area: php;
  }

  .more {
    grid-area: more;
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
    grid-area: where;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px 14px;
    min-width: 0;
    font-size: 13px;
  }

  /* A note moves to the next line as a whole rather than breaking inside. */
  .where > :not(.path) {
    white-space: nowrap;
  }

  .path {
    min-width: 0;
    overflow-wrap: anywhere;
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




  .error {
    color: var(--danger);
  }

  .banner {
    max-width: 960px;
    margin: 0 0 12px;
  }

  .ide {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 960px;
    margin: 0 0 10px;
    color: var(--muted);
    font-size: 13px;
  }
  .ide-dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .ide-dot.on {
    background: var(--ok);
  }
  .muted {
    color: var(--muted);
  }

  @media (max-width: 860px) {
    .list.card {
      grid-template-columns: minmax(0, 1fr) minmax(180px, max-content);
    }

    .row {
      grid-template-areas:
        'name php'
        'where where'
        'more more';
    }
  }
</style>
