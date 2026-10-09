<!-- "On this page": the sections, and the subsections of the one being read, with the entry in view lit as the
     reader scrolls; at its foot when the wiki was written and at which commit. After deepwiki-by-cc's
     TableOfContents and WikiTree (MIT; see THIRD_PARTY_NOTICES.md), laid out as Code Wiki's. -->
<script lang="ts" module>
  export interface Entry {
    id: string;
    title: string;
    level: 2 | 3;
    /** The index of its section's entry, its own for a section. */
    section: number;
  }
</script>

<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import type { Snippet } from 'svelte';

  let {
    entries,
    active,
    updated,
    commit,
    commitHref,
    note,
    ongo,
    tools,
  }: {
    entries: Entry[];
    active: number;
    updated: string;
    commit: string;
    commitHref: string | null;
    note: string;
    ongo: (id: string) => void;
    tools?: Snippet;
  } = $props();

  let list: HTMLOListElement;
  const sections = $derived(entries.map((e, i) => ({ ...e, i })).filter((e) => e.level === 2));
  const openSection = $derived(entries[active]?.section ?? -1);

  // The outline scrolls, on its own, to keep the entry in view in sight.
  $effect(() => {
    const el = list?.querySelector<HTMLElement>('a.active');
    if (!el) return;
    const top = el.offsetTop - list.offsetTop;
    const bottom = top + el.offsetHeight;
    if (top < list.scrollTop + 8) list.scrollTop = Math.max(0, top - 8);
    else if (bottom > list.scrollTop + list.clientHeight - 8) list.scrollTop = bottom - list.clientHeight + 8;
  });

  function click(event: MouseEvent, id: string) {
    if (event.metaKey || event.ctrlKey || event.shiftKey) return;
    event.preventDefault();
    ongo(id);
  }
</script>

<nav class="outline" aria-label="On this page">
  <div class="outline-title">On this page</div>
  <ol class="outline-list" bind:this={list}>
    {#each sections as section (section.id)}
      <li class:open={openSection === section.i}>
        <a href="#{section.id}" class:active={active === section.i} aria-current={active === section.i ? 'location' : undefined} onclick={(e) => click(e, section.id)}>{section.title}</a>
        {#if openSection === section.i}
          <ol>
            {#each entries.slice(section.i + 1) as sub, j (sub.id)}
              {#if sub.section === section.i}
                <li>
                  <a href="#{sub.id}" class:active={active === section.i + 1 + j} aria-current={active === section.i + 1 + j ? 'location' : undefined} onclick={(e) => click(e, sub.id)}>{sub.title}</a>
                </li>
              {/if}
            {/each}
          </ol>
        {/if}
      </li>
    {/each}
  </ol>
  <div class="outline-foot">
    <div class="foot-row"><span>Updated on</span><span>{updated || '—'}</span></div>
    <div class="foot-row">
      <span>Commit</span>
      {#if commitHref}<a href={commitHref} target="_blank" rel="noopener noreferrer">{commit}</a>{:else}<span>{commit || '—'}</span>{/if}
    </div>
    {#if tools}<div class="drawer-tools only-phone">{@render tools()}</div>{/if}
    <p class="ai-note"><Icon name="spark" size={11} class="inline-spark" /> {note}</p>
  </div>
</nav>

<style>
  .ai-note :global(.inline-spark) {
    display: inline;
    vertical-align: -1px;
    color: var(--claude);
  }
</style>
