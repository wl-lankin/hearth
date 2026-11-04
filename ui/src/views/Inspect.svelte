<script lang="ts">
  import Tabs from '../Tabs.svelte';
  import Dumps from './Dumps.svelte';
  import Profiles from './Profiles.svelte';
  import Requests from './Requests.svelte';

  /** What the sites did: their dumps, their requests and queries, their profiles. */
  let { tab = 'dumps' }: { tab?: 'dumps' | 'requests' | 'profiles' } = $props();
  let active = $derived<string>(tab);
</script>

<h1>Inspect</h1>
<Tabs
  tabs={[
    { id: 'dumps', label: 'Dumps' },
    { id: 'requests', label: 'Requests' },
    { id: 'profiles', label: 'Profiles' },
  ]}
  {active}
  onchange={(id) => (active = id)}
/>
{#if active === 'requests'}
  <Requests />
{:else if active === 'profiles'}
  <Profiles />
{:else}
  <Dumps />
{/if}
