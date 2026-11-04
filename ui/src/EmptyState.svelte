<script lang="ts">
  /** What a page shows while it has nothing yet: an illustration, a title, one line, maybe a button. */
  let {
    art,
    title,
    line,
    action,
  }: { art: string; title: string; line: string; action?: { label: string; onclick: () => void } } = $props();
</script>

<div class="empty-state">
  <img src={art} alt="" loading="lazy" />
  <h2>{title}</h2>
  <p>{line}</p>
  {#if action}
    <button class="primary" onclick={action.onclick}>{action.label}</button>
  {/if}
</div>

<style>
  .empty-state {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 18px 20px 30px;
    text-align: center;
  }

  /* A soft ember light under the illustration. */
  .empty-state::before {
    content: '';
    position: absolute;
    left: 50%;
    top: 40px;
    width: 340px;
    height: 220px;
    transform: translateX(-50%);
    background: radial-gradient(closest-side, rgb(240 122 64 / 0.22), transparent);
    pointer-events: none;
  }

  img {
    position: relative;
    height: 220px;
    max-width: 100%;
    object-fit: contain;
  }

  h2 {
    position: relative;
    margin: 6px 0 4px;
    font-size: 18px;
  }

  p {
    position: relative;
    margin: 0;
    color: var(--muted);
  }

  button {
    position: relative;
    margin-top: 16px;
  }
</style>
