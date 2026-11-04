<script lang="ts">
  import * as api from './api';
  import { resolveView, type View } from './nav';
  import Rail from './Rail.svelte';
  import { createSettingsStore } from './settings.svelte';
  import TitleBar, { platformOf } from './TitleBar.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import Inspect from './views/Inspect.svelte';
  import Logs from './views/Logs.svelte';
  import Mail from './views/Mail.svelte';
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

  settings.load();

  function navigate(next: View, opts: { tab?: string } = {}) {
    view = next;
    tab = opts.tab ?? null;
  }
</script>

<div class="glow" aria-hidden="true"></div>
<TitleBar platform={platformOf(navigator.userAgent)} />
<div class="shell">
  <Rail {view} onnavigate={(next) => navigate(next)} />
  <main>
    {#key view}
      <div class="page">
        {#if view === 'home'}
          <Dashboard />
        {:else if view === 'sites'}
          <Sites />
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
        {:else}
          <Settings {settings} />
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
    grid-template-columns: 208px 1fr;
    height: 100%;
  }

  main {
    overflow: auto;
    padding: calc(var(--titlebar-height) + 4px) 32px 28px;
  }

  .page {
    animation: page-in 180ms var(--ease);
  }

  @keyframes page-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }
</style>
