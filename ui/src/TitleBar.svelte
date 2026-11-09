<script module lang="ts">
  export type Platform = 'windows' | 'macos' | 'other';

  /** Which window chrome to draw: Windows gets Stoke's own buttons, macOS keeps its traffic lights. */
  export function platformOf(userAgent: string): Platform {
    if (/Windows/i.test(userAgent)) return 'windows';
    if (/Macintosh|Mac OS X/i.test(userAgent)) return 'macos';
    return 'other';
  }
</script>

<script lang="ts">
  /**
   * The strip at the top of the window. The system title bar is gone, so this strip moves the
   * window (and maximizes it on double-click); on Windows it also holds the window buttons.
   */
  let { platform }: { platform: Platform } = $props();
  let maximized = $state(false);

  // Outside the app (browser demo, tests) there is no window to control.
  const inApp = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  async function current() {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    return getCurrentWindow();
  }

  $effect(() => {
    if (!inApp || platform !== 'windows') return;
    let stop: (() => void) | undefined;
    current().then(async (win) => {
      maximized = await win.isMaximized();
      stop = await win.onResized(async () => (maximized = await win.isMaximized()));
    });
    return () => stop?.();
  });

  const act = (what: 'minimize' | 'toggleMaximize' | 'close') => async () => {
    if (inApp) await (await current())[what]();
  };
</script>

<div class="titlebar" class:mac={platform === 'macos'} data-tauri-drag-region>
  {#if platform === 'windows'}
    <div class="controls">
      <button aria-label="Minimize" onclick={act('minimize')}>
        <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" /></svg>
      </button>
      <button aria-label="Maximize" title={maximized ? 'Restore' : 'Maximize'} onclick={act('toggleMaximize')}>
        <svg viewBox="0 0 10 10" aria-hidden="true">
          {#if maximized}
            <path d="M2 0.5h7.5v7.5M0.5 2.5h7v7h-7z" />
          {:else}
            <path d="M0.5 0.5h9v9h-9z" />
          {/if}
        </svg>
      </button>
      <button class="close" aria-label="Close" onclick={act('close')}>
        <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" /></svg>
      </button>
    </div>
  {/if}
</div>

<style>
  .titlebar {
    position: fixed;
    /* Over the page column only: the rail's top holds the logo. */
    inset: 0 0 auto var(--rail-width);
    z-index: 50;
    height: var(--titlebar-height);
    display: flex;
    justify-content: flex-end;
    -webkit-user-select: none;
    user-select: none;
  }

  .controls {
    display: flex;
    height: 100%;
  }

  .controls button {
    width: 46px;
    height: 100%;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--muted);
    transition: background-color 150ms ease, color 150ms ease;
  }

  .controls button:hover {
    background: rgb(255 255 255 / 0.08);
    color: var(--text);
  }

  .controls button.close:hover {
    background: #c42b1c;
    color: #fff;
  }

  svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1;
  }
</style>
