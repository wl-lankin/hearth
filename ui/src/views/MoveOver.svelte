<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../api';
  import type { MoveItem } from '../api';
  import { createMoveStore, keyOf, type MoveStore } from '../moveover.svelte';

  /** store: the wizard's state (tests pass their own). */
  let { store = createMoveStore(api) }: { store?: MoveStore } = $props();

  onMount(() => {
    store.load();
  });

  const areas: [string, string][] = [
    ['sites', 'Sites'],
    ['php', 'PHP'],
    ['https', 'HTTPS'],
    ['redis', 'Redis'],
    ['mail', 'Mail'],
    ['node', 'Node'],
    ['path', 'PATH'],
    ['mcp', 'Claude and other agents'],
    ['autostart', 'Starting with Windows'],
    ['mariaDb', 'MariaDB'],
    ['hosts', 'Other names'],
  ];
  const grouped = $derived(
    areas
      .map(([id, label]) => ({ label, items: store.items.filter((i: MoveItem) => i.area === id) }))
      .filter((g) => g.items.length > 0),
  );
  const steps = ['Dry run', 'Move', 'Check', 'Uninstall', 'Done'];
  const at = $derived(
    { loading: 0, none: 0, dry: 0, moving: 1, check: 2, uninstalling: 3, done: 4 }[store.step],
  );
  const verdictText = { same: 'Same as under Herd', better: 'Answers now', changed: 'Changed', broken: 'Not answering' };
</script>

<h1>Move over from Herd</h1>

<ol class="steps" aria-label="Steps">
  {#each steps as label, i (label)}
    <li class:done={i < at} class:now={i === at}><span class="n">{i + 1}</span>{label}</li>
  {/each}
</ol>

{#if store.error}
  <p class="error glass" role="alert">{store.error}</p>
{/if}

{#if store.step === 'loading'}
  <p class="muted">Reading Herd's setup and recording how your sites answer. This takes up to a minute.</p>
{:else if store.step === 'none'}
  <p class="muted">Herd isn't installed on this computer: there is nothing to move over.</p>
{:else if store.step === 'dry'}
  {#if store.startedHerd}
    <p class="note glass">Herd was started once to record how your sites answer. Nothing has changed yet.</p>
  {/if}
  <p class="lead">Nothing has changed yet. This is what moving over does:</p>
  {#each grouped as group (group.label)}
    <section class="card glass">
      <h2>{group.label}</h2>
      {#each group.items as item (item.what)}
        <div class="item" class:stays={!item.moved}>
          <span class="what">{item.what}</span>
          <span class="action">{item.moved ? item.action : item.reason}</span>
        </div>
      {/each}
    </section>
  {/each}
  <div class="actions">
    <button class="primary" disabled={store.busy} onclick={() => store.move()}>Move over</button>
  </div>
{:else if store.step === 'moving'}
  <section class="card glass">
    <h2>{store.current || 'Starting'}</h2>
    <div class="bar"><div class="fill" style:width="{Math.round(store.progress * 100)}%"></div></div>
    <p class="muted">Installing PHP and Node versions can take a few minutes.</p>
  </section>
{:else if store.step === 'check' && store.check}
  {@const check = store.check}
  <p class="lead">Every site, asked again through Hearth and compared with how it answered under Herd.</p>
  <section class="card glass">
    <h2>Sites</h2>
    {#each check.sites as site (keyOf(site))}
      {@const accepted = check.accepted.includes(keyOf(site))}
      <div class="item">
        <span class="what">{keyOf(site)}</span>
        <span class="verdict {site.verdict.kind}" class:accepted>
          {verdictText[site.verdict.kind]}{'detail' in site.verdict ? `: ${site.verdict.detail}` : ''}
        </span>
        {#if (site.verdict.kind === 'changed' || site.verdict.kind === 'broken') && !accepted}
          <button disabled={store.busy} onclick={() => store.accept(keyOf(site))}>Accept</button>
        {/if}
      </div>
    {/each}
  </section>
  <section class="card glass">
    <h2>Services and Node</h2>
    {#each check.services as s (s.name)}
      <div class="item"><span class="what">{s.name}</span><span class="verdict {s.running ? 'same' : 'broken'}">{s.running ? 'Running' : 'Not running'}</span></div>
    {/each}
    <div class="item">
      <span class="what">node</span>
      <span class="verdict {check.node.fromHearth ? 'same' : 'changed'}">{check.node.fromHearth ? "Hearth's Node" : "Still Herd's Node"}</span>
    </div>
  </section>
  <div class="actions">
    <button disabled={store.busy} onclick={() => store.recheck()}>Check again</button>
    <button disabled={store.busy} onclick={() => store.undo()}>Undo</button>
    <button class="primary" disabled={store.busy || !store.canUninstall} onclick={() => store.uninstall()}>Uninstall Herd</button>
  </div>
{:else if store.step === 'uninstalling'}
  <section class="card glass">
    <h2>Herd's uninstaller is running</h2>
    <p class="muted">Windows may ask for permission. Hearth checks every site again afterwards.</p>
  </section>
{:else if store.step === 'done'}
  <section class="card glass">
    <h2>Herd is gone</h2>
    <p>Everything runs on Hearth now. One step left: Claude updates your CLAUDE.md and skills to Hearth's paths in a session.</p>
  </section>
{/if}

<style>
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin: 0 0 20px;
    padding: 0;
    list-style: none;
    color: var(--muted);
    font-size: 12.5px;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .n {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border: 1px solid var(--border);
    border-radius: 50%;
    font-size: 11px;
  }

  .steps li.now {
    color: var(--text);
    font-weight: 600;
  }

  .steps li.now .n {
    border-color: transparent;
    background: linear-gradient(180deg, #f6904f, #e8642c);
    color: var(--accent-text);
  }

  .steps li.done .n {
    border-color: rgb(92 197 138 / 0.5);
    color: var(--ok);
  }

  .lead {
    margin: 0 0 14px;
  }

  .muted {
    color: var(--muted);
  }

  .note,
  .error {
    margin: 0 0 14px;
    padding: 10px 14px;
  }

  .error {
    color: var(--danger);
  }

  .card {
    max-width: 960px;
    margin-bottom: 14px;
    padding: 14px 18px;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 14px;
  }

  .item {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 14px;
    padding: 7px 0;
    border-top: 1px solid var(--border);
  }

  .item:first-of-type {
    border-top: 0;
  }

  .what {
    flex: 1 1 260px;
    min-width: 0;
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .action {
    flex: 2 1 320px;
    min-width: 0;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .item.stays .what,
  .item.stays .action {
    color: var(--muted);
    opacity: 0.75;
  }

  .verdict {
    flex: 2 1 320px;
    font-size: 12.5px;
  }

  .verdict.same,
  .verdict.better {
    color: var(--ok);
  }

  .verdict.changed {
    color: var(--gold);
  }

  .verdict.broken {
    color: var(--danger);
  }

  .verdict.accepted {
    color: var(--muted);
  }

  .bar {
    height: 8px;
    margin: 10px 0;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.08);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: linear-gradient(90deg, #e8642c, #ffd27a);
    transition: width 300ms var(--ease);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
    max-width: 960px;
    margin-top: 4px;
  }
</style>
