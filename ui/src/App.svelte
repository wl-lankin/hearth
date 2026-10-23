<script lang="ts">
  import logo from '../../assets/logo.svg?url';
  import * as api from './api';
  import { createSettingsStore } from './settings.svelte';
  import { applyTheme, resolveTheme } from './theme';
  import Dashboard from './views/Dashboard.svelte';
  import Settings from './views/Settings.svelte';

  type View = 'dashboard' | 'settings';

  const settings = createSettingsStore(api);
  // `?view=settings` opens a section directly (used for screenshots in the browser demo).
  let view = $state<View>(new URLSearchParams(location.search).get('view') === 'settings' ? 'settings' : 'dashboard');
  let prefersDark = $state(window.matchMedia('(prefers-color-scheme: dark)').matches);

  settings.load();

  $effect(() => {
    const query = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = (e: MediaQueryListEvent) => (prefersDark = e.matches);
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  });

  $effect(() => applyTheme(resolveTheme(settings.value.theme, prefersDark)));

  const nav: { id: View; label: string }[] = [
    { id: 'dashboard', label: 'Dashboard' },
    { id: 'settings', label: 'Settings' },
  ];
</script>

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
    {:else}
      <Settings {settings} />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 208px 1fr;
    height: 100%;
  }

  aside {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 20px 12px 14px;
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
    background: var(--surface);
    color: var(--text);
    font-weight: 600;
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .version {
    margin-top: auto;
    padding: 0 10px;
    color: var(--muted);
    font-size: 12px;
  }

  main {
    overflow: auto;
    padding: 28px 32px;
  }
</style>
