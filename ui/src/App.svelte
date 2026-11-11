<script lang="ts">
  import * as api from './api';
  import { resolveView, type View } from './nav';
  import Rail from './Rail.svelte';
  import { createSettingsStore } from './settings.svelte';
  import TitleBar, { platformOf } from './TitleBar.svelte';
  import Home from './views/Home.svelte';
  import Inspect from './views/Inspect.svelte';
  import Logs from './views/Logs.svelte';
  import Mail from './views/Mail.svelte';
  import MoveOver from './views/MoveOver.svelte';
  import Processes from './views/Processes.svelte';
  import Runtimes from './views/Runtimes.svelte';
  import Services from './views/Services.svelte';
  import Settings from './views/Settings.svelte';
  import Sites from './views/Sites.svelte';
  import Workspaces from './views/Workspaces.svelte';

  const settings = createSettingsStore(api);
  // `?view=` opens a page directly (deep links, screenshots of the browser demo); old page names
  // open the merged page on their tab.
  const start = resolveView(new URLSearchParams(location.search).get('view'));
  let view = $state<View>(start.view);
  let tab = $state<string | null>(start.tab);
  // Home's Get started opens Sites with its link form.
  let link = $state(false);

  settings.load();

  const platform = platformOf(navigator.userAgent);
  // For styles that differ per platform (the logo steps aside for the macOS traffic lights).
  document.documentElement.dataset.platform = platform;

  function navigate(next: View, opts: { tab?: string; link?: boolean } = {}) {
    view = next;
    tab = opts.tab ?? null;
    link = opts.link ?? false;
  }

  // The tray panel opens the window on a page ("Install another...", "All sites...").
  $effect(() => {
    if (!('__TAURI_INTERNALS__' in window)) return;
    let stop: (() => void) | undefined;
    let cancelled = false;
    import('@tauri-apps/api/event').then(({ listen }) =>
      listen<{ view: string | null; tab: string | null }>('stoke://navigate', ({ payload }) => {
        const target = resolveView(payload.view);
        navigate(target.view, { tab: payload.tab ?? target.tab ?? undefined });
      }).then((unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      }),
    );
    return () => {
      cancelled = true;
      stop?.();
    };
  });
</script>

<div class="glow" aria-hidden="true"></div>
<TitleBar {platform} />
<div class="shell">
  <Rail {view} onnavigate={(next) => navigate(next)} />
  <main>
    {#key view}
      <div class="page">
        {#if view === 'home'}
          <Home onnavigate={navigate} />
        {:else if view === 'sites'}
          <Sites startLinking={link} />
        {:else if view === 'workspaces'}
          <Workspaces />
        {:else if view === 'services'}
          <Services />
        {:else if view === 'processes'}
          <Processes />
        {:else if view === 'runtimes'}
          <Runtimes tab={tab === 'node' ? 'node' : 'php'} />
        {:else if view === 'mail'}
          <Mail />
        {:else if view === 'inspect'}
          <Inspect tab={tab === 'requests' || tab === 'profiles' ? tab : 'dumps'} />
        {:else if view === 'logs'}
          <Logs />
        {:else if view === 'moveover'}
          <MoveOver />
        {:else}
          <Settings {settings} onnavigate={(next) => navigate(next)} />
        {/if}
      </div>
    {/key}
  </main>
</div>

<style>
  /* The ember glow behind everything: warm from the bottom right, a faint gold at the top left.
     It breathes slowly. */
  .glow {
    position: fixed;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    background:
      radial-gradient(65% 55% at 78% 108%, rgb(240 122 64 / 0.34), transparent 70%),
      radial-gradient(45% 40% at 40% 115%, rgb(194 54 31 / 0.22), transparent 70%),
      radial-gradient(40% 32% at 8% -4%, rgb(255 210 122 / 0.08), transparent 70%);
    animation: breathe 8s ease-in-out infinite alternate;
  }

  @keyframes breathe {
    from {
      opacity: 0.85;
    }
    to {
      opacity: 1;
    }
  }

  .shell {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: var(--rail-width) 1fr;
    height: 100%;
  }

  /* The page scrolls below the title strip, never under its buttons. */
  main {
    height: calc(100% - var(--titlebar-height));
    margin-top: var(--titlebar-height);
    overflow: auto;
    padding: 4px 32px 0;
  }

  /* At least as tall as the content area, so a page can place something at its bottom (Home's
     core box); a longer page grows and `main` scrolls. The space below the last card is the
     page's own padding: WebKit (the Mac's window) leaves a scroll container's bottom padding out
     when content overflows a fixed-height child, so it used to end flush with the window. */
  .page {
    min-height: 100%;
    padding-bottom: 28px;
    animation: page-in 180ms var(--ease);
  }

  @keyframes page-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
</style>
