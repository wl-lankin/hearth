<script lang="ts">
  import logo from '../../assets/logo.svg?url';
  import * as api from './api';
  import { createSettingsStore } from './settings.svelte';
  import TitleBar, { platformOf } from './TitleBar.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import Dumps from './views/Dumps.svelte';
  import Logs from './views/Logs.svelte';
  import Mail from './views/Mail.svelte';
  import NodePage from './views/Node.svelte';
  import Php from './views/Php.svelte';
  import Processes from './views/Processes.svelte';
  import Profiles from './views/Profiles.svelte';
  import Requests from './views/Requests.svelte';
  import Services from './views/Services.svelte';
  import Sites from './views/Sites.svelte';
  import Workspaces from './views/Workspaces.svelte';
  import Settings from './views/Settings.svelte';

  type View = 'dashboard' | 'sites' | 'workspaces' | 'services' | 'processes' | 'php' | 'node' | 'mail' | 'dumps' | 'requests' | 'profiles' | 'logs' | 'settings';

  const settings = createSettingsStore(api);
  // `?view=settings` opens a section directly (used for screenshots in the browser demo).
  const requested = new URLSearchParams(location.search).get('view');
  const views: View[] = ['dashboard', 'sites', 'workspaces', 'services', 'processes', 'php', 'node', 'mail', 'dumps', 'requests', 'profiles', 'logs', 'settings'];
  let view = $state<View>(views.includes(requested as View) ? (requested as View) : 'dashboard');

  settings.load();

  const nav: { id: View; label: string }[] = [
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'sites', label: 'Sites' },
    { id: 'workspaces', label: 'Workspaces' },
    { id: 'services', label: 'Services' },
    { id: 'processes', label: 'Processes' },
    { id: 'php', label: 'PHP' },
    { id: 'node', label: 'Node' },
    { id: 'mail', label: 'Mail' },
    { id: 'dumps', label: 'Dumps' },
    { id: 'requests', label: 'Requests' },
    { id: 'profiles', label: 'Profiles' },
    { id: 'logs', label: 'Logs' },
    { id: 'settings', label: 'Settings' },
  ];
</script>

<div class="glow" aria-hidden="true"></div>
<TitleBar platform={platformOf(navigator.userAgent)} />
<div class="shell">
  <aside>
    <div class="brand">
      <img class="mark" src={logo} alt="" width="26" height="26" />
      <span class="name">Hearth</span>
    </div>
    <nav aria-label="Sections">
      {#each nav as item (item.id)}
        <button class:active={view === item.id} aria-current={view === item.id ? 'page' : undefined} onclick={() => (view = item.id)}>
          {item.label}
        </button>
      {/each}
    </nav>
    <div class="version">v{__HEARTH_VERSION__}</div>
  </aside>
  <main>
    {#if view === 'dashboard'}
      <Dashboard />
    {:else if view === 'sites'}
      <Sites />
    {:else if view === 'workspaces'}
      <Workspaces />
    {:else if view === 'services'}
      <Services />
    {:else if view === 'processes'}
      <Processes />
    {:else if view === 'php'}
      <Php />
    {:else if view === 'node'}
      <NodePage />
    {:else if view === 'mail'}
      <Mail />
    {:else if view === 'dumps'}
      <Dumps />
    {:else if view === 'requests'}
      <Requests />
    {:else if view === 'profiles'}
      <Profiles />
    {:else if view === 'logs'}
      <Logs />
    {:else}
      <Settings {settings} />
    {/if}
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

  aside {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: calc(var(--titlebar-height) + 6px) 12px 14px;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px;
  }

  .mark {
    display: block;
  }

  .name {
    font-size: 16px;
    font-weight: 650;
    letter-spacing: 0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  nav button {
    text-align: left;
    padding: 7px 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  nav button:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  nav button.active {
    background: var(--nav-active);
    color: var(--text);
    font-weight: 600;
  }

  .version {
    margin-top: auto;
    padding: 0 10px;
    color: var(--muted);
    font-size: 12px;
  }

  main {
    overflow: auto;
    padding: calc(var(--titlebar-height) + 4px) 32px 28px;
  }
</style>
