<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { theme } from '$lib/theme.svelte';
  import { toast } from '$lib/toast.svelte';

  let { children } = $props();

  onMount(() => theme.apply());
</script>

{@render children()}

{#if toast.text}
  <div class="toast" role="status" aria-live="polite">{toast.text}</div>
{/if}
