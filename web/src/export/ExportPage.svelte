<!-- An exported wiki: the page as lattice serves it, but read-only, its links into the code going to the forge
     and its chat saying asking needs `lattice serve`. -->
<script lang="ts">
  import { toast } from '$lib/toast.svelte';
  import type { Wiki } from '$lib/types';
  import WikiView from '$lib/wiki/WikiView.svelte';

  let { wiki }: { wiki: Wiki } = $props();
</script>

<WikiView {wiki} mode="static" home={null} />

{#if toast.text}
  <div class="toast" role="status" aria-live="polite">{toast.text}</div>
{/if}
