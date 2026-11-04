<script lang="ts">
  import Tabs from '../Tabs.svelte';
  import NodePage from './Node.svelte';
  import Php from './Php.svelte';

  /** PHP and Node, the languages Hearth runs. */
  let { tab = 'php' }: { tab?: 'php' | 'node' } = $props();
  let active = $derived<string>(tab);
</script>

<h1>Runtimes</h1>
<Tabs
  tabs={[
    { id: 'php', label: 'PHP' },
    { id: 'node', label: 'Node' },
  ]}
  {active}
  onchange={(id) => (active = id)}
/>
{#if active === 'node'}
  <NodePage />
{:else}
  <Php />
{/if}
