<script lang="ts">
  import type { ThemeSetting } from '../api';
  import type { SettingsStore } from '../settings.svelte';
  import Terminal from './Terminal.svelte';
  import Updates from './Updates.svelte';

  let { settings }: { settings: SettingsStore } = $props();

  const themes: { id: ThemeSetting; label: string }[] = [
    { id: 'system', label: 'System' },
    { id: 'light', label: 'Light' },
    { id: 'dark', label: 'Dark' },
  ];
</script>

<h1>Settings</h1>

<section class="card">
  <h2>Appearance</h2>
  <div class="row">
    <span id="theme-label">Theme</span>
    <div class="segmented" role="radiogroup" aria-labelledby="theme-label">
      {#each themes as theme (theme.id)}
        <button
          role="radio"
          aria-checked={settings.value.theme === theme.id}
          class:on={settings.value.theme === theme.id}
          onclick={() => settings.update({ theme: theme.id })}
        >
          {theme.label}
        </button>
      {/each}
    </div>
  </div>
</section>

<section class="card">
  <h2>Startup</h2>
  <label class="row">
    <span>Start at login</span>
    <input
      type="checkbox"
      checked={settings.value.launchAtLogin}
      onchange={(e) => settings.update({ launchAtLogin: e.currentTarget.checked })}
    />
  </label>
  <label class="row">
    <span>Start hidden in the tray</span>
    <input
      type="checkbox"
      checked={settings.value.startHidden}
      onchange={(e) => settings.update({ startHidden: e.currentTarget.checked })}
    />
  </label>
  {#if settings.error}
    <p class="error" role="alert">{settings.error}</p>
  {/if}
</section>

<Terminal />

<Updates />

<style>
  h1 {
    margin: 0 0 20px;
    font-size: 22px;
    font-weight: 650;
  }

  .card {
    max-width: 720px;
    margin-bottom: 16px;
    padding: 16px 20px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    min-height: 40px;
  }

  label.row {
    cursor: pointer;
  }

  .row + .row {
    border-top: 1px solid var(--border);
  }

  input[type='checkbox'] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .segmented {
    display: inline-flex;
    padding: 3px;
    gap: 2px;
    background: var(--surface-2);
    border-radius: 9px;
  }

  .segmented button {
    padding: 4px 14px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }

  .segmented button.on {
    background: var(--surface);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.12);
  }

  .error {
    margin: 10px 0 0;
    color: var(--danger);
  }
</style>
