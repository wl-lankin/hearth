<script lang="ts">
  import * as api from '../api';
  import type { CoreStatus } from '../api';
  import EmptyState from '../EmptyState.svelte';
  import { formatUptime } from '../format';
  import { greeting, litWindows, rack, windows, type Win } from '../house';
  import { art } from '../illustrations';
  import type { View } from '../nav';

  type Source = Pick<typeof api, 'coreStatus' | 'webStatus' | 'servicesList' | 'sitesList' | 'phpList' | 'moveoverState'>;

  /** source: where the numbers come from (the core; tests pass their own). */
  let {
    onnavigate,
    source = api,
  }: { onnavigate: (view: View, opts?: { link?: boolean }) => void; source?: Source } = $props();

  let status = $state<CoreStatus | null>(null);
  let failed = $state<string | null>(null);
  // null until the core answered: the welcome is only for a site list that is really empty.
  let sites = $state<number | null>(null);
  let running = $state(0);
  let webRunning = $state(false);
  let php = $state<string | null>(null);
  let hour = $state(new Date().getHours());
  // Herd is there and nothing moved yet: offer the move.
  let offerMove = $state(false);

  async function refresh() {
    hour = new Date().getHours();
    const [core, list, services, web, runtimes] = await Promise.allSettled([
      source.coreStatus(),
      source.sitesList(),
      source.servicesList(),
      source.webStatus(),
      source.phpList(),
    ]);
    source
      .moveoverState()
      .then((m) => (offerMove = m.herd && !m.moved && !m.uninstalled))
      .catch(() => (offerMove = false));
    if (core.status === 'fulfilled') {
      status = core.value;
      failed = null;
    } else {
      failed = String(core.reason);
    }
    if (list.status === 'fulfilled') sites = list.value.sites.length;
    if (services.status === 'fulfilled') running = services.value.filter((s) => s.running).length;
    if (web.status === 'fulfilled') webRunning = web.value.running;
    if (runtimes.status === 'fulfilled') php = runtimes.value.default;
  }

  $effect(() => {
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  });

  const lit = $derived(litWindows(webRunning, running));
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

  /** A window's rectangle as a clip on the whole picture. */
  const clip = (w: Win) => `inset(${w.y}% ${100 - w.x - w.w}% ${100 - w.y - w.h}% ${w.x}%)`;
</script>

<div class="home">
{#if offerMove}
  <section class="move glass">
    <div>
      <h2>Herd is still installed</h2>
      <p>Move your sites, PHP, services and tools over in one go. You see everything first, and can go back.</p>
    </div>
    <button class="primary" onclick={() => onnavigate('moveover')}>Move over from Herd</button>
  </section>
{/if}

{#if sites === 0}
  <div class="welcome">
    <EmptyState
      art={art.welcome}
      title="Welcome to Stoke"
      line="Your sites, services and tools, warm and ready"
      action={{ label: 'Get started', onclick: () => onnavigate('sites', { link: true }) }}
    />
  </div>
{:else}
  <section class="hero">
    <div class="text">
      <h1>{greeting(hour)}</h1>
      {#if sites !== null}
        <p class="line">{plural(sites, 'site')}, {plural(running, 'service')} running</p>
      {/if}
      <div class="stats">
        <div class="stat glass"><span class="num">{sites ?? 0}</span> <span class="lbl">Sites</span></div>
        <div class="stat glass"><span class="num">{running}</span> <span class="lbl">Services running</span></div>
        <div class="stat glass"><span class="num">{php ?? 'None'}</span> <span class="lbl">PHP</span></div>
      </div>
    </div>
    <div class="house" aria-hidden="true">
      <img src={art.homeDark} alt="" />
      {#each windows as w (w.id)}
        <img class="light" class:on={lit.windows.includes(w.id)} src={art.homeLit} alt="" style:clip-path={clip(w)} />
      {/each}
      {#each rack as w (w.id)}
        <img class="light" class:on={lit.rack} src={art.homeLit} alt="" style:clip-path={clip(w)} />
      {/each}
    </div>
  </section>
{/if}

<section class="card glass">
  <header>
    <h2>Core</h2>
    {#if status && !failed}
      <span class="pill ok">Running</span>
    {:else if failed}
      <span class="pill bad">Not responding</span>
    {/if}
  </header>
  {#if status}
    <dl>
      <dt>Version</dt>
      <dd>{status.version}</dd>
      <dt>Uptime</dt>
      <dd>{formatUptime(status.uptimeSecs)}</dd>
      <dt>Home</dt>
      <dd class="path" title={status.home}>{status.home}</dd>
    </dl>
  {/if}
  {#if failed}
    <p class="error">{failed}</p>
  {/if}
</section>
</div>

<style>
  .hero {
    position: relative;
    display: flex;
    align-items: center;
    min-height: 440px;
    margin-bottom: 10px;
    container-type: inline-size;
    /* The house reaches past the edge; it must not make the page scroll sideways. */
    overflow-x: clip;
  }

  .text {
    position: relative;
    z-index: 1;
    max-width: 58%;
  }

  h1 {
    margin: 0;
    font-size: 30px;
  }

  .line {
    margin: 6px 0 22px;
    color: var(--muted);
  }

  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .stat {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 12px 16px;
  }

  .num {
    font-size: 24px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .lbl {
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  /* As tall as the hero and no taller: the chimney and its sparks never reach up into the title
     strip, at any window size. */
  .house {
    position: absolute;
    right: -56px;
    top: 0;
    height: 100%;
    aspect-ratio: 2048 / 1360;
  }

  .house img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  /* A narrow window: the house steps back so it never covers the tiles. */
  @container (max-width: 640px) {
    .house {
      right: -20px;
      top: 50%;
      width: 46%;
      height: auto;
      transform: translateY(-50%);
    }
  }

  .light {
    opacity: 0;
    transition: opacity 400ms var(--ease);
  }

  .light.on {
    opacity: 1;
  }

  .move {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px 20px;
    max-width: 960px;
    margin: 0 auto 16px;
    padding: 14px 18px;
  }

  .move p {
    margin: 4px 0 0;
    color: var(--muted);
  }

  .welcome {
    margin: 10px 0 24px;
  }

  .welcome :global(img) {
    height: 300px;
  }

  /* The page fills the window, so the core box always sits at the bottom. */
  .home {
    display: flex;
    flex-direction: column;
    min-height: 100%;
  }

  .card {
    max-width: 720px;
    width: 100%;
    margin: auto auto 0;
    padding: 18px 20px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  h2 {
    margin: 0;
    font-size: 15px;
  }

  .pill {
    padding: 2px 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
  }

  .ok {
    background: var(--ok-soft);
    color: var(--ok);
  }

  .bad {
    background: var(--accent-soft);
    color: var(--danger);
  }

  dl {
    display: grid;
    grid-template-columns: 90px 1fr;
    gap: 6px 16px;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    min-width: 0;
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace;
    font-size: 13px;
  }

  .error {
    margin: 10px 0 0;
    color: var(--danger);
  }
</style>
