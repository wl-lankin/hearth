<script lang="ts">
  /**
   * What a list shows while it loads: rows laid like kindling in the shape of the real ones. A
   * fire front runs through them, each status dot glows like an ember and throws a spark now and
   * then. Appears only after a short delay, so a quick answer never flashes it.
   */
  let { rows = 3, label = 'Loading' }: { rows?: number; label?: string } = $props();
</script>

<div class="kindling" role="status" aria-live="polite">
  <span class="sr">{label}</span>
  {#each { length: rows } as _, i (i)}
    <div class="log" style:--i={i} aria-hidden="true">
      <span class="ember">
        <i class="spark"></i>
        <i class="spark two"></i>
      </span>
      <span class="stick name"></span>
      <span class="stick tag"></span>
      <span class="stick state"></span>
      <span class="sticks">
        <span class="stick btn"></span>
        <span class="stick btn"></span>
        <span class="stick btn wide"></span>
      </span>
    </div>
  {/each}
</div>

<style>
  .kindling {
    animation: arrive 260ms var(--ease) 140ms both;
  }

  @keyframes arrive {
    from {
      opacity: 0;
    }
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  .log {
    --delay: calc(var(--i) * 180ms);
    position: relative;
    display: grid;
    grid-template-columns: 8px minmax(80px, 150px) 56px minmax(0, 1fr) auto;
    gap: 8px 14px;
    align-items: center;
    padding: 14px 20px;
    overflow: hidden;
    opacity: calc(1 - var(--i) * 0.22);
  }

  .log + .log {
    border-top: 1px solid var(--border);
  }

  /* The fire front: one glow that runs through all the kindling at once. The gradient is laid
     over the window (fixed), so it continues from stick to stick and row to row instead of each
     stick glowing on its own; its slant makes the lower rows catch a moment later. */
  .stick {
    display: block;
    height: 10px;
    border-radius: 999px;
    background-color: var(--surface-2);
    background-image: linear-gradient(
      105deg,
      transparent 0 38%,
      rgb(194 54 31 / 0.35) 44%,
      rgb(240 122 64 / 0.75) 48.5%,
      rgb(255 210 122 / 0.9) 50%,
      rgb(240 122 64 / 0.5) 51.5%,
      rgb(194 54 31 / 0.18) 56%,
      transparent 64% 100%
    );
    background-attachment: fixed;
    background-size: 220vw 100vh;
    background-repeat: no-repeat;
    animation: front 2.6s cubic-bezier(0.45, 0.05, 0.35, 1) infinite;
  }

  @keyframes front {
    from {
      background-position: 100% 0;
    }
    to {
      background-position: 0% 0;
    }
  }

  .name {
    height: 12px;
  }

  .tag {
    height: 16px;
  }

  .state {
    width: min(90px, 100%);
  }

  .sticks {
    display: flex;
    gap: 6px;
  }

  .btn {
    width: 52px;
    height: 24px;
    border-radius: 7px;
  }

  .btn.wide {
    width: 72px;
  }

  /* The ember: glows and dims like charcoal in a draught. */
  .ember {
    position: relative;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: radial-gradient(circle at 40% 35%, #ffd27a, var(--accent) 55%, var(--accent-deep));
    animation: glow 1.6s ease-in-out var(--delay) infinite alternate;
  }

  @keyframes glow {
    from {
      opacity: 0.55;
      box-shadow: 0 0 2px rgb(240 122 64 / 0.3);
    }
    to {
      opacity: 1;
      box-shadow:
        0 0 6px rgb(240 122 64 / 0.8),
        0 0 14px rgb(194 54 31 / 0.45);
    }
  }

  /* Sparks: rise, drift and burn out. */
  .spark {
    position: absolute;
    left: 3px;
    top: 0;
    width: 2px;
    height: 2px;
    border-radius: 50%;
    background: #ffd27a;
    box-shadow: 0 0 4px #f07a40;
    opacity: 0;
    animation: rise 2.2s ease-out calc(var(--delay) + 0.4s) infinite;
  }

  .spark.two {
    animation-duration: 2.8s;
    animation-delay: calc(var(--delay) + 1.5s);
    --drift: -7px;
  }

  @keyframes rise {
    0% {
      opacity: 0;
      transform: translate(0, 0) scale(1);
    }
    12% {
      opacity: 1;
    }
    100% {
      opacity: 0;
      transform: translate(var(--drift, 6px), -22px) scale(0.4);
    }
  }

  @media (max-width: 720px) {
    .log {
      grid-template-columns: 8px minmax(0, 1fr) 56px;
    }
    .state,
    .sticks {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spark {
      display: none;
    }
    .ember,
    .stick {
      animation: none;
    }
  }
</style>
