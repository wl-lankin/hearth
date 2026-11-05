<script lang="ts">
  import type { View } from '../nav';
  import type { SettingsStore } from '../settings.svelte';
  import Terminal from './Terminal.svelte';
  import Updates from './Updates.svelte';

  let { settings, onnavigate = () => {} }: { settings: SettingsStore; onnavigate?: (view: View) => void } = $props();
</script>

<h1>Settings</h1>

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

<section class="card">
  <h2>Herd</h2>
  <div class="row">
    <span>Move sites, PHP, services and tools over from Laravel Herd</span>
    <button onclick={() => onnavigate('moveover')}>Move over</button>
  </div>
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
    cursor: pointer;
  }

  .error {
    margin: 10px 0 0;
    color: var(--danger);
  }
</style>
