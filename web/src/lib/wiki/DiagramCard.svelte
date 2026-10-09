<!-- A diagram on its card, as Code Wiki draws them: drawn by mermaid once it comes near (or at once, in the
     chat), fitted to the card and centred, with a zoom button at its bottom right opening it large. One mermaid
     can't draw shows its source instead. After deepwiki-by-cc's MermaidDiagram (MIT; see
     THIRD_PARTY_NOTICES.md). -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { drawNow, drawWhenNear, fitted, type Drawing } from '$lib/mermaid';

  let {
    src,
    caption = '',
    now = false,
    onzoom,
  }: { src: string; caption?: string | null; now?: boolean; onzoom: (drawing: Drawing, caption: string) => void } = $props();

  let card: HTMLElement;
  let drawing = $state<Drawing | null>(null);
  let error = $state('');
  const label = $derived(caption || 'Diagram');

  onMount(() => {
    const done = (result: Awaited<ReturnType<typeof drawNow>>) => {
      if (result.ok) drawing = result.drawing;
      else error = result.error;
    };
    if (now) {
      void drawNow(src).then(done);
      return;
    }
    return drawWhenNear(card, src, done);
  });

  function zoom(event: MouseEvent) {
    if (!drawing || (event.target as Element).closest('a')) return;
    onzoom(drawing, label);
  }
</script>

<!-- The card is a large target for the mouse; the zoom button is the keyboard's way in. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<figure class="diagram-card" class:drawn={!!drawing} class:failed={!!error} aria-label={label} bind:this={card} onclick={zoom}>
  {#if drawing}
    <div class="diagram-view">{@html fitted(drawing)}</div>
    {#if caption}<figcaption class="caption">{caption}</figcaption>{/if}
    <button class="zoom-btn" type="button" aria-label="Zoom into the diagram: {label}" title="Zoom">
      <Icon name="zoom" size={30} />
    </button>
  {:else if error}
    <div class="diagram-view">
      <p class="diagram-fail">{error}</p>
      <pre class="code-block"><code>{src}</code></pre>
    </div>
  {:else}
    <div class="diagram-view"><div class="spinner" aria-label="Drawing the diagram"></div></div>
    {#if caption}<figcaption class="sr-only">{caption}</figcaption>{/if}
  {/if}
</figure>
