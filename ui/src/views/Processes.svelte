<script lang="ts">
  import Select from '../Select.svelte';
  import { tick } from 'svelte';
  import * as api from '../api';
  import { createProcessesStore } from '../processes.svelte';

  const store = createProcessesStore(api);
  let adding = $state(false);
  let pane = $state<HTMLPreElement | null>(null);
  // The output follows new lines unless the user scrolled up to read.
  let follow = $state(true);

  store.load().then(() => {
    const first = store.sites[0]?.processes[0];
    if (first) store.select(store.sites[0].site, first.name);
  });

  $effect(() => {
    const timer = setInterval(() => store.poll(), 1000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    void store.output;
    if (follow && pane) tick().then(() => pane && (pane.scrollTop = pane.scrollHeight));
  });

  function scrolled() {
    if (pane) follow = pane.scrollHeight - pane.scrollTop - pane.clientHeight < 24;
  }

  async function openForm() {
    adding = true;
    if (store.form.site) await store.suggest(store.form.site);
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const site = store.form.site;
    const name = store.form.name.trim();
    if (await store.add()) {
      adding = false;
      await store.select(site, name);
    }
  }

  const current = $derived(
    store.selected ? store.sites.find((s) => s.site === store.selected?.site)?.processes.find((p) => p.name === store.selected?.name) : undefined,
  );
</script>

<h1>Processes</h1>

{#if store.error}<p class="error banner" role="alert">{store.error}</p>{/if}

<section class="split">
  <div class="card list" aria-label="Processes">
    {#if store.sites.length === 0 && !adding}
      <p class="empty">No processes yet. Add Vite, a queue worker or Pail for a site, and start them here.</p>
    {/if}
    <ul>
      {#each store.sites as group (group.site)}
        <li class="site">{group.site}.test</li>
        {#each group.processes as p (p.name)}
          <li>
            <button
              class="row"
              class:selected={store.selected?.site === group.site && store.selected?.name === p.name}
              onclick={() => store.select(group.site, p.name)}
            >
              <span class="line">
                <span class="dot" class:ok={p.running} class:bad={!p.running && !!p.lastError} aria-hidden="true"></span>
                <span class="name">{p.name}</span>
                <span class="state">{store.state(p)}</span>
              </span>
              <span class="command">{p.command}</span>
              {#if store.rowError(group.site, p.name)}<span class="error small">{store.rowError(group.site, p.name)}</span>{/if}
            </button>
          </li>
        {/each}
      {/each}
    </ul>
    <div class="add">
      {#if adding}
        <form onsubmit={submit}>
          <div class="field">
            <span>Site</span>
            <Select
              label="Site"
              value={store.form.site}
              onchange={(site) => {
                store.form.site = site;
                store.suggest(site);
              }}
              options={store.siteNames.map((name) => ({ value: name, label: `${name}.test` }))}
            />
          </div>
          {#if store.suggestions.length}
            <div class="field">
              <span>Suggested for this project</span>
              <div class="chips">
                {#each store.suggestions as s (s.name)}
                  <button
                    type="button"
                    class="chip"
                    class:on={store.form.name === s.name && store.form.command === s.command}
                    title={s.command}
                    onclick={() => store.useSuggestion(s)}>{s.name}</button
                  >
                {/each}
              </div>
            </div>
          {/if}
          <label class="field">
            <span>Name</span>
            <input bind:value={store.form.name} placeholder="vite" autocomplete="off" spellcheck="false" />
          </label>
          <label class="field">
            <span>Command</span>
            <input class="mono" bind:value={store.form.command} placeholder="npm run dev" autocomplete="off" spellcheck="false" />
          </label>
          <label class="check"><input type="checkbox" bind:checked={store.form.autostart} /> Start with Hearth</label>
          {#if store.formError}<p class="error small">{store.formError}</p>{/if}
          <div class="buttons">
            <button type="button" onclick={() => (adding = false)}>Cancel</button>
            <button type="submit" class="primary" disabled={!store.form.site || !store.form.name.trim() || !store.form.command.trim()}>Add</button>
          </div>
        </form>
      {:else}
        <button onclick={openForm} disabled={store.siteNames.length === 0} title={store.siteNames.length ? '' : 'Park or link a site first'}>Add a process</button>
      {/if}
    </div>
  </div>

  <div class="card detail">
    {#if store.selected && current}
      {@const sel = store.selected}
      <header>
        <div class="title">
          <h2>{sel.name} <span class="muted">{sel.site}.test</span></h2>
          {#if current.running}
            <button onclick={() => store.act('restart', sel.site, sel.name)}>Restart</button>
          {/if}
          {#if store.canStop(current)}
            <button onclick={() => store.act('stop', sel.site, sel.name)}>Stop</button>
          {:else}
            <button class="primary" onclick={() => store.act('start', sel.site, sel.name)}>Start</button>
          {/if}
        </div>
        <p class="facts">
          <code>{current.command}</code>
          {#if current.pid}<span>PID {current.pid}</span>{/if}
          {#if current.restarts}<span>{current.restarts} {current.restarts === 1 ? 'restart' : 'restarts'}</span>{/if}
          {#if current.autostart}<span>starts with Hearth</span>{/if}
        </p>
        {#if current.lastError && !current.running}<p class="error small">{current.lastError}</p>{/if}
      </header>
      <pre class="output" bind:this={pane} onscroll={scrolled}>{store.output || 'No output yet.'}</pre>
      <footer>
        <button class="link danger" onclick={() => store.remove(sel.site, sel.name)}>Remove {sel.name}</button>
      </footer>
    {:else}
      <p class="empty">Pick a process to see its output.</p>
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
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
  .split {
    display: grid;
    grid-template-columns: minmax(240px, 320px) minmax(0, 1fr);
    gap: 16px;
    height: max(440px, calc(100vh - 140px));
  }
  .list,
  .detail {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  ul {
    flex: 1;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }
  .site {
    padding: 10px 12px 4px;
    color: var(--muted);
    font-size: 12px;
    font-weight: 650;
  }
  button.row {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 100%;
    padding: 8px 12px;
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
  }
  .name {
    flex: 1;
    font-size: 13px;
    font-weight: 600;
  }
  .state {
    color: var(--muted);
    font-size: 12px;
  }
  .command,
  .mono,
  code {
    overflow: hidden;
    color: var(--muted);
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    font-variant-ligatures: none;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    color: inherit;
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
  .add {
    padding: 10px 12px;
    border-top: 1px solid var(--border);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 13px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field > span {
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }
  .field input {
    width: 100%;
  }
  label.check {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  label.check input {
    accent-color: var(--accent);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    padding: 2px 11px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }
  .chip:hover {
    border-color: var(--accent);
    background: var(--surface);
    color: var(--accent);
  }
  .chip.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
  }
  .buttons {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }
  .buttons button {
    flex: 1;
  }
  .add > button {
    width: 100%;
  }
  header {
    padding: 14px 16px 10px;
    border-bottom: 1px solid var(--border);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 8px 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  .output {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 12px 16px;
    overflow: auto;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 12px;
    font-variant-ligatures: none;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  footer {
    padding: 8px 16px;
    border-top: 1px solid var(--border);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
    font-size: 12px;
  }
  .link.danger:hover {
    color: var(--danger);
  }
  .empty {
    margin: 0;
    padding: 24px 16px;
    color: var(--muted);
    font-size: 13px;
  }
  .muted {
    color: var(--muted);
    font-weight: 400;
  }
  .error.small {
    margin: 0;
    font-size: 12px;
  }
  .error.banner {
    margin: 0 0 16px;
  }
  @media (max-width: 900px) {
    .split {
      grid-template-columns: 1fr;
      height: auto;
    }
    .detail {
      min-height: 420px;
    }
  }
</style>
