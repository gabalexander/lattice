<script lang="ts">
  import { page } from '$app/state';
  import AppHeader from '$lib/components/AppHeader.svelte';
</script>

<svelte:head><title>Not here · lattice</title></svelte:head>

<AppHeader />
<main class="missing">
  <h1>{page.status === 404 ? 'Nothing here' : 'Something went wrong'}</h1>
  <p>{page.status === 404 ? `lattice has no page at ${page.url.pathname}.` : page.error?.message}</p>
  <a class="pill" href="/">Every wiki</a>
</main>

<style>
  .missing {
    max-width: 560px;
    margin: 12vh auto 0;
    padding: 0 24px;
    text-align: center;
  }
  h1 {
    font: 400 32px/40px var(--font-display);
    margin: 0 0 8px;
  }
  p {
    color: var(--text-2);
    margin: 0 0 24px;
  }
</style>
