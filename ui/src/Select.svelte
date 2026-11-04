<script lang="ts">
  /**
   * A dropdown in Hearth's own look (the native one ignores the theme): a button showing the
   * chosen option and a list under it. Keyboard: arrows, Home/End, Enter or Space to pick,
   * Escape to close; a click outside closes it.
   */
  type Option = { value: string; label: string };
  let {
    value,
    options,
    label,
    id,
    disabled = false,
    onchange,
  }: { value: string; options: Option[]; label: string; id?: string; disabled?: boolean; onchange: (value: string) => void } = $props();

  let open = $state(false);
  let active = $state(0);
  let button = $state<HTMLButtonElement | null>(null);
  let place = $state({ left: 0, top: 0, width: 0, up: false });
  const listId = `select-${Math.random().toString(36).slice(2)}`;

  const chosen = $derived(options.find((o) => o.value === value));

  function show() {
    if (disabled || options.length === 0) return;
    const r = button?.getBoundingClientRect();
    if (r) {
      // Opens upwards when the window has no room below.
      const below = window.innerHeight - r.bottom;
      const up = below < Math.min(240, options.length * 34 + 12) && r.top > below;
      place = { left: r.left, top: up ? r.top - 4 : r.bottom + 4, width: r.width, up };
    }
    active = Math.max(0, options.findIndex((o) => o.value === value));
    open = true;
  }

  function pick(index: number) {
    const option = options[index];
    open = false;
    button?.focus();
    if (option && option.value !== value) onchange(option.value);
  }

  function keydown(event: KeyboardEvent) {
    const last = options.length - 1;
    if (!open) {
      if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(event.key)) {
        event.preventDefault();
        show();
      }
      return;
    }
    const moves: Record<string, () => number> = {
      ArrowDown: () => Math.min(last, active + 1),
      ArrowUp: () => Math.max(0, active - 1),
      Home: () => 0,
      End: () => last,
    };
    if (moves[event.key]) {
      event.preventDefault();
      active = moves[event.key]();
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      pick(active);
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      open = false;
    }
  }

  $effect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      const target = event.target as Node;
      if (!button?.contains(target) && !document.getElementById(listId)?.contains(target)) open = false;
    };
    const close = () => (open = false);
    document.addEventListener('pointerdown', outside, true);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', close, true);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', close, true);
    };
  });
</script>

<button
  bind:this={button}
  {id}
  type="button"
  class="select"
  class:open
  {disabled}
  aria-label={label}
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={open ? listId : undefined}
  onclick={() => (open ? (open = false) : show())}
  onkeydown={keydown}
>
  <span class="text">{chosen?.label ?? ''}</span>
  <svg class="chevron" viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M3 4.5 6 7.5 9 4.5" /></svg>
</button>

{#if open}
  <ul
    id={listId}
    class="list"
    class:up={place.up}
    role="listbox"
    aria-label={label}
    style:left="{place.left}px"
    style:top="{place.top}px"
    style:min-width="{place.width}px"
  >
    {#each options as option, i (option.value)}
      <li
        role="option"
        aria-selected={option.value === value}
        class:active={i === active}
        onpointerenter={() => (active = i)}
        onclick={() => pick(i)}
        onkeydown={() => {}}
      >
        <span>{option.label}</span>
        {#if option.value === value}
          <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true"><path d="M2.5 6.2 5 8.5 9.5 3.5" /></svg>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .select {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    width: 100%;
    min-width: 0;
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: rgb(0 0 0 / 0.22);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms ease;
  }
  .select:hover:not(:disabled),
  .select.open {
    border-color: var(--muted);
  }
  .select:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  svg {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .chevron {
    color: var(--muted);
    transition: transform 120ms ease;
  }
  .open .chevron {
    transform: rotate(180deg);
  }
  .list {
    position: fixed;
    z-index: 60;
    max-height: 240px;
    margin: 0;
    padding: 4px;
    overflow: auto;
    list-style: none;
    /* Dense glass: the page behind must not show through the options. */
    background: rgb(34 27 23 / 0.94);
    backdrop-filter: blur(16px);
    border: 1px solid rgb(255 255 255 / 0.12);
    border-radius: 12px;
    box-shadow: 0 16px 40px rgb(0 0 0 / 0.45);
  }
  .list.up {
    transform: translateY(-100%);
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 10px;
    border-radius: 6px;
    white-space: nowrap;
    cursor: pointer;
  }
  li.active {
    background: var(--surface-2);
  }
  li[aria-selected='true'] {
    color: var(--accent);
    font-weight: 600;
  }
</style>
