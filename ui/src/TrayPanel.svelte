<script lang="ts">
  import logo from '../../assets/logo.svg?url';
  import * as api from './api';
  import type { InstalledPhp, ServiceRow, SitesList, UpdateState } from './api';
  import { shownSites, siteUrl } from './tray';

  type Source = Pick<
    typeof api,
    | 'phpList'
    | 'phpUse'
    | 'servicesList'
    | 'servicesStart'
    | 'servicesStop'
    | 'sitesList'
    | 'webStatus'
    | 'openUrl'
    | 'updateState'
    | 'onUpdateState'
    | 'installUpdateNow'
  >;
  type Actions = { open(view?: string, tab?: string): void; quit(): void; fit(w: number, h: number): void; hide(): void };

  const appActions: Actions = {
    open: (view, tab) => void (view ? api.appOpen(view, tab) : api.appOpen()),
    quit: () => void api.appQuit(),
    fit: (w, h) => void api.panelFit(w, h),
    hide: () => void api.panelHide(),
  };

  /** The tray panel. source and actions: the core and the app (tests pass their own). */
  let { source = api, actions = appActions }: { source?: Source; actions?: Actions } = $props();

  /** The panel card's width, the flyout's, and the gap and margin around them (room for the shadow). */
  const PANEL = 300;
  const FLYOUT = 200;
  const GAP = 12;
  // The same as MARGIN in src-tauri/src/panel.rs: the card is placed, the margin hangs over.
  const MARGIN = 32;

  let php = $state<{ default: string | null; installed: InstalledPhp[] }>({ default: null, installed: [] });
  let services = $state<ServiceRow[]>([]);
  let sites = $state<SitesList | null>(null);
  let webRunning = $state(false);
  let webError = $state<string | null>(null);
  let coreDown = $state(false);
  let update = $state<UpdateState>({ phase: 'idle' });
  let flyout = $state(false);
  let busy = $state<Record<string, boolean>>({});
  let errors = $state<Record<string, string | null>>({});
  let card = $state<HTMLElement | null>(null);
  let phpRow = $state<HTMLElement | null>(null);
  let flyoutTop = $state(0);

  async function refresh() {
    const [p, s, l, w, u] = await Promise.allSettled([
      source.phpList(),
      source.servicesList(),
      source.sitesList(),
      source.webStatus(),
      source.updateState(),
    ]);
    if (p.status === 'fulfilled') php = p.value;
    if (s.status === 'fulfilled') services = s.value;
    if (l.status === 'fulfilled') sites = l.value;
    if (w.status === 'fulfilled') {
      webRunning = w.value.running;
      webError = w.value.error;
    }
    if (u.status === 'fulfilled') update = u.value;
    coreDown = l.status === 'rejected' && w.status === 'rejected';
  }

  $effect(() => {
    refresh();
    const timer = setInterval(() => document.visibilityState !== 'hidden' && refresh(), 4000);
    const onFocus = () => refresh();
    window.addEventListener('focus', onFocus);
    let stop: (() => void) | undefined;
    let cancelled = false;
    source.onUpdateState((next) => (update = next)).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      clearInterval(timer);
      window.removeEventListener('focus', onFocus);
      stop?.();
    };
  });

  // The window is as big as the panel (and the flyout when it is open).
  $effect(() => {
    if (!card) return;
    const measure = () => {
      const flyoutBottom = flyout ? flyoutTop + flyoutHeight() : 0;
      const height = Math.max(card!.offsetHeight, flyoutBottom) + MARGIN * 2;
      const width = PANEL + MARGIN * 2 + (flyout ? FLYOUT + GAP : 0);
      actions.fit(width, height);
    };
    measure();
    // Without ResizeObserver (old WebViews, tests) the first size is kept.
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(card);
    return () => observer.disconnect();
  });

  function flyoutHeight() {
    return (php.installed.length + 1) * 34 + 14;
  }

  function toggleFlyout() {
    flyoutTop = phpRow?.offsetTop ?? 0;
    flyout = !flyout;
  }

  async function usePhp(minor: string) {
    flyout = false;
    await source.phpUse(minor);
    php = { ...php, default: minor };
    refresh();
  }

  async function toggleService(s: ServiceRow) {
    busy = { ...busy, [s.id]: true };
    errors = { ...errors, [s.id]: null };
    try {
      await (s.running ? source.servicesStop(s.id) : source.servicesStart(s.id));
    } catch (e) {
      errors = { ...errors, [s.id]: String(e).replace(/^Error: /, '') };
    }
    busy = { ...busy, [s.id]: false };
    refresh();
  }

  function openSite(site: { name: string; secured: boolean }) {
    if (!sites) return;
    source.openUrl(siteUrl(site, sites.httpPort, sites.httpsPort));
    actions.hide();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    if (flyout) flyout = false;
    else actions.hide();
  }

  const list = $derived(shownSites(sites?.sites ?? []));
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
  const status = $derived(
    coreDown
      ? { kind: 'off', text: 'Hearth core is not running' }
      : webError
        ? { kind: 'bad', text: webError }
        : webRunning
          ? { kind: 'ok', text: `Serving ${plural(sites?.sites.length ?? 0, 'site')}` }
          : { kind: 'off', text: 'The web server is stopped' },
  );
</script>

<svelte:window onkeydown={keydown} />

<div class="frame" style:padding="{MARGIN}px">
  {#if flyout}
    <div class="flyout" role="menu" aria-label="Default PHP version" style:top="{MARGIN + flyoutTop}px" style:right="{MARGIN + PANEL + GAP}px">
      {#each php.installed as version (version.minor)}
        <button role="menuitemradio" aria-checked={php.default === version.minor} class="row" onclick={() => usePhp(version.minor)}>
          <span class="check">{php.default === version.minor ? '✓' : ''}</span>
          <span class="grow">PHP {version.minor}</span>
          <span class="muted">{version.version}</span>
        </button>
      {/each}
      <button role="menuitem" class="row more" onclick={() => actions.open('runtimes', 'php')}>
        <span class="check"></span>
        <span class="grow muted">Install another...</span>
      </button>
    </div>
  {/if}

  <div class="panel" bind:this={card}>
    <header>
      <img src={logo} alt="" width="26" height="26" />
      <div>
        <div class="title">Hearth</div>
        <div class="status"><span class="dot {status.kind}"></span>{status.text}</div>
      </div>
    </header>

    <section bind:this={phpRow}>
      <div class="label">PHP</div>
      <button class="row" class:hl={flyout} aria-expanded={flyout} aria-haspopup="menu" onclick={toggleFlyout}>
        <span class="grow">Default version</span>
        <span class="muted">{php.default ?? 'None'}</span>
        <span class="chev">{flyout ? '‹' : '›'}</span>
      </button>
    </section>

    {#if services.length > 0}
      <section>
        <div class="label">Services</div>
        <div class="services">
          {#each services as s (s.id)}
            <div class="row">
              <span class="sdot" class:on={s.running}></span>
              <span class="grow">{s.name}</span>
              <button
                class="switch"
                class:on={s.running}
                role="switch"
                aria-checked={s.running}
                aria-label="{s.running ? 'Stop' : 'Start'} {s.name}"
                disabled={busy[s.id]}
                onclick={() => toggleService(s)}
              ></button>
            </div>
            {#if errors[s.id]}<p class="error">{errors[s.id]}</p>{/if}
          {/each}
        </div>
      </section>
    {/if}

    {#if list.shown.length > 0}
      <section>
        <div class="label">Sites</div>
        {#each list.shown as site (site.name)}
          <button class="row" onclick={() => openSite(site)}>
            <span class="grow name">{site.name}.test</span>
            {#if site.secured}<span class="pill">HTTPS</span>{/if}
            <span class="chev">↗</span>
          </button>
        {/each}
        {#if list.more > 0}
          <button class="row" onclick={() => actions.open('sites')}><span class="grow muted">All {list.more} sites...</span></button>
        {/if}
      </section>
    {/if}

    {#if update.phase === 'ready'}
      <div class="update">
        <span class="grow">Hearth {update.version} is ready</span>
        <button class="primary small" onclick={() => source.installUpdateNow()}>Restart to update</button>
      </div>
    {/if}

    <footer>
      <button onclick={() => actions.open()}>Open Hearth</button>
      <button onclick={() => actions.quit()}>Quit Hearth</button>
    </footer>
  </div>
</div>

<style>
  /* The window itself is invisible: only the cards show. */
  :global(html),
  :global(body.panel-page) {
    background: transparent;
    overflow: hidden;
  }

  .frame {
    position: relative;
    display: flex;
    justify-content: flex-end;
    align-items: flex-start;
  }

  .panel,
  .flyout {
    color: var(--text);
    border: 1px solid rgb(255 255 255 / 0.1);
    /* Fits inside the window margin (offset + blur stay under 32 px), so nothing cuts it. */
    box-shadow:
      0 8px 22px rgb(0 0 0 / 0.5),
      0 1px 3px rgb(0 0 0 / 0.4);
    animation: open 120ms var(--ease);
  }

  @keyframes open {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  .panel {
    position: relative;
    width: 300px;
    overflow: hidden;
    border-radius: 14px;
    background: rgb(24 19 16 / 0.97);
    font-size: 12.5px;
  }

  /* The ember glow from the bottom, as in the app. */
  .panel::before {
    content: '';
    position: absolute;
    inset: 0;
    background: radial-gradient(80% 50% at 80% 110%, rgb(240 122 64 / 0.3), transparent 70%);
    pointer-events: none;
  }

  header {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
  }

  .title {
    font-size: 13.5px;
    font-weight: 700;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: 11px;
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #5a4a40;
  }

  .dot.ok {
    background: var(--ok);
  }

  .dot.bad {
    background: var(--danger);
  }

  section {
    position: relative;
    padding: 8px 8px 6px;
    border-bottom: 1px solid rgb(255 255 255 / 0.07);
  }

  .label {
    padding: 2px 8px 5px;
    color: var(--muted);
    font-size: 9.5px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 6px 8px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font-weight: 400;
    text-align: left;
  }

  button.row:hover:not(:disabled),
  .row.hl {
    background: rgb(255 255 255 / 0.06);
    border-color: transparent;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .name {
    overflow-wrap: anywhere;
  }

  .muted {
    color: var(--muted);
    font-size: 11px;
  }

  .chev {
    color: var(--muted);
  }

  .services {
    max-height: calc(6 * 31px);
    overflow-y: auto;
  }

  .sdot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #5a4a40;
  }

  .sdot.on {
    background: var(--ok);
    box-shadow: 0 0 6px var(--ok);
  }

  .switch {
    position: relative;
    flex: none;
    width: 28px;
    height: 16px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.14);
  }

  .switch::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #d9cbc1;
    transition: left 150ms var(--ease);
  }

  .switch.on {
    background: linear-gradient(180deg, #f6904f, #e8642c);
  }

  .switch.on::after {
    left: 14px;
    background: var(--accent-text);
  }

  .switch:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.2);
  }

  .switch.on:hover:not(:disabled) {
    background: linear-gradient(180deg, #f6904f, #e8642c);
    filter: brightness(1.08);
  }

  .error {
    margin: 0 8px 4px 24px;
    color: var(--danger);
    font-size: 11px;
  }

  .pill {
    padding: 1px 7px;
    border-radius: 999px;
    background: rgb(240 122 64 / 0.18);
    color: #f6a66f;
    font-size: 10px;
  }

  .update {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px;
    padding: 8px 10px;
    border: 1px solid rgb(240 122 64 / 0.35);
    border-radius: 9px;
    background: linear-gradient(180deg, rgb(240 122 64 / 0.28), rgb(240 122 64 / 0.12));
  }

  .small {
    padding: 3px 10px;
    font-size: 11px;
  }

  footer {
    position: relative;
    display: flex;
    gap: 6px;
    padding: 8px;
  }

  footer button {
    flex: 1;
    padding: 7px 0;
    font-size: 12px;
  }

  .flyout {
    position: absolute;
    width: 200px;
    padding: 6px;
    border-radius: 12px;
    background: rgb(28 22 19 / 0.97);
    font-size: 12.5px;
  }

  .check {
    width: 14px;
    color: var(--accent);
    font-weight: 700;
  }

  .more {
    margin-top: 4px;
    border-top: 1px solid rgb(255 255 255 / 0.07);
    border-radius: 0 0 8px 8px;
  }
</style>
