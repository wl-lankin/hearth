<script lang="ts">
  import logo from '../../assets/logo.svg?url';
  import { mailList } from './api';
  import { groups, unreadCount, type View } from './nav';

  /** The menu on the left: Home, three groups, Settings at the bottom. */
  let { view, onnavigate }: { view: View; onnavigate: (view: View) => void } = $props();
  let unread = $state(0);

  async function refresh() {
    try {
      unread = unreadCount(await mailList());
    } catch {
      // The badge is a hint: without the core it just stays as it was.
    }
  }

  $effect(() => {
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  });
</script>

{#snippet entry(id: View, label: string)}
  <button class:active={view === id} aria-current={view === id ? 'page' : undefined} onclick={() => onnavigate(id)}>
    <span class="label">{label}</span>
    {#if id === 'mail' && unread > 0}<span class="badge" aria-label="{unread} unread">{unread}</span>{/if}
  </button>
{/snippet}

<aside>
  <!-- Level with the window buttons; it moves the window like the strip beside it. -->
  <div class="brand" data-tauri-drag-region>
    <img class="mark" src={logo} alt="" width="28" height="28" />
    <span class="name">Hearth</span>
  </div>
  <nav aria-label="Sections">
    {#each groups as group (group.label)}
      <div class="group">
        {#if group.label}<div class="heading">{group.label}</div>{/if}
        {#each group.items as item (item.id)}
          {@render entry(item.id, item.label)}
        {/each}
      </div>
    {/each}
  </nav>
  <div class="bottom">
    {@render entry('settings', 'Settings')}
    <div class="version">v{__HEARTH_VERSION__}</div>
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
    padding: 0 12px 14px;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }

  .brand {
    display: flex;
    flex: none;
    align-items: center;
    gap: 10px;
    height: calc(var(--titlebar-height) + 10px);
    margin-bottom: 8px;
    padding: 4px 8px 0;
    -webkit-user-select: none;
    user-select: none;
  }

  .brand > * {
    pointer-events: none;
  }

  /* macOS draws its traffic lights in the top left corner: the logo steps aside. */
  :global(html[data-platform='macos']) .brand {
    padding-left: 78px;
  }

  .mark {
    display: block;
    filter: drop-shadow(0 4px 10px rgb(240 122 64 / 0.35));
  }

  .name {
    font-size: 16px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .heading {
    padding: 0 10px 4px;
    color: var(--muted);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    opacity: 0.8;
  }

  button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 10px;
    border: 1px solid transparent;
    border-radius: 9px;
    background: transparent;
    color: #d9cbc1;
    text-align: left;
  }

  button:hover:not(.active) {
    background: rgb(255 255 255 / 0.05);
    color: var(--text);
  }

  button.active {
    border-color: rgb(240 122 64 / 0.22);
    background: linear-gradient(90deg, rgb(240 122 64 / 0.26), rgb(240 122 64 / 0.06));
    color: #fff;
    font-weight: 600;
  }

  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .badge {
    min-width: 18px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 700;
    line-height: 18px;
    text-align: center;
  }

  .bottom {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: auto;
  }

  .version {
    padding: 0 10px;
    color: var(--muted);
    font-size: 12px;
  }

  /* A short window: tighter, so Settings stays in view down to the smallest size. */
  @media (max-height: 680px) {
    aside {
      gap: 10px;
    }

    nav {
      gap: 6px;
    }

    .heading {
      padding-bottom: 1px;
    }

    button {
      padding: 4px 10px;
    }

    .version {
      display: none;
    }
  }
</style>
