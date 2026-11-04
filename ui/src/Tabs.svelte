<script lang="ts">
  /** Tabs of a merged page (Runtimes, Inspect). */
  let {
    tabs,
    active,
    onchange,
  }: { tabs: { id: string; label: string }[]; active: string; onchange: (id: string) => void } = $props();
</script>

<div class="tabs" role="tablist">
  {#each tabs as tab (tab.id)}
    <button role="tab" aria-selected={tab.id === active} class:on={tab.id === active} onclick={() => onchange(tab.id)}>
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: inline-flex;
    gap: 2px;
    margin-bottom: 22px;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: 11px;
    background: rgb(0 0 0 / 0.2);
  }

  .tabs button {
    padding: 5px 16px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    backdrop-filter: none;
  }

  .tabs button:hover:not(.on) {
    color: var(--text);
    background: rgb(255 255 255 / 0.05);
  }

  .tabs button.on {
    background: linear-gradient(180deg, rgb(240 122 64 / 0.28), rgb(240 122 64 / 0.12));
    color: var(--text);
    font-weight: 600;
    box-shadow: inset 0 0 0 1px rgb(240 122 64 / 0.3);
  }
</style>
