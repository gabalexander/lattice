<!-- Some markdown on the page: its text rendered (markdown.ts), its ```mermaid fences drawn as diagram cards in
     their places, and its code blocks coloured once highlight.js is in. -->
<script lang="ts">
  import { findMermaidFences } from '$lib/fences';
  import { highlightWithin } from '$lib/highlight';
  import { renderMarkdown, type MarkdownOptions } from '$lib/markdown';
  import type { Drawing } from '$lib/mermaid';
  import DiagramCard from './DiagramCard.svelte';

  let {
    md,
    options,
    diagrams = true,
    drawNow = false,
    onzoom,
  }: {
    md: string;
    options: MarkdownOptions;
    /** Whether its mermaid fences are drawn; an answer still streaming shows them as code. */
    diagrams?: boolean;
    drawNow?: boolean;
    onzoom: (drawing: Drawing, caption: string) => void;
  } = $props();

  let el: HTMLElement;

  type Chunk = { kind: 'text'; html: string } | { kind: 'diagram'; src: string };

  const chunks = $derived.by((): Chunk[] => {
    if (!diagrams) return [{ kind: 'text', html: renderMarkdown(md, options) }];
    const out: Chunk[] = [];
    let at = 0;
    for (const fence of findMermaidFences(md)) {
      if (fence.start > at) out.push({ kind: 'text', html: renderMarkdown(md.slice(at, fence.start), options) });
      out.push({ kind: 'diagram', src: fence.code });
      at = fence.end;
    }
    if (at < md.length) out.push({ kind: 'text', html: renderMarkdown(md.slice(at), options) });
    return out;
  });

  $effect(() => {
    void chunks;
    if (el) void highlightWithin(el);
  });
</script>

<div class="prose" bind:this={el}>
  {#each chunks as chunk, i (chunk.kind === 'diagram' ? `${i}:${chunk.src}` : i)}
    {#if chunk.kind === 'text'}
      {@html chunk.html}
    {:else}
      <DiagramCard src={chunk.src} now={drawNow} {onzoom} />
    {/if}
  {/each}
</div>
