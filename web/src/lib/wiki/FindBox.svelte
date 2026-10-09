<!-- Find, in the header as Code Wiki has it: this page's sections and subsections by their titles, files and
     text, and lattice's other wikis by name, as you type; arrows and Enter pick one. -->
<script lang="ts" module>
  export interface FindItem {
    id: string;
    title: string;
    context: string;
    text: string;
    raw: string;
    files: string;
  }
  export interface FindRepo {
    key: string;
    name: string;
    detail: string;
  }
</script>

<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import { escapeHtml } from '$lib/markdown';

  let {
    items,
    repos = () => Promise.resolve([]),
    ongo,
    onclose,
    input = $bindable(),
  }: {
    items: FindItem[];
    repos?: () => Promise<FindRepo[]>;
    ongo: (id: string) => void;
    onclose: () => void;
    input?: HTMLInputElement;
  } = $props();

  type Result = { kind: 'section'; id: string; title: string; context: string } | { kind: 'repo'; href: string; title: string; context: string };

  let query = $state('');
  let others = $state<FindRepo[]>([]);
  let sel = $state(0);
  let open = $state(false);

  function highlight(text: string, words: string[]): string {
    let html = escapeHtml(text);
    for (const word of words) {
      const re = new RegExp(`(${escapeHtml(word).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'gi');
      html = html.replace(re, '<mark>$1</mark>');
    }
    return html;
  }

  function snippet(raw: string, word: string): string {
    const at = raw.toLowerCase().indexOf(word);
    if (at < 0) return raw.slice(0, 90);
    const from = Math.max(0, at - 36);
    return `${from ? '…' : ''}${raw.slice(from, at + 70)}…`;
  }

  const results = $derived.by((): { page: Result[]; repos: Result[] } => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return { page: [], repos: [] };
    const scored: { item: FindItem; score: number }[] = [];
    for (const item of items) {
      const title = item.title.toLowerCase();
      let score = 0;
      if (words.every((w) => title.includes(w))) score = 3 + (title.startsWith(words[0]) ? 1 : 0);
      else if (words.every((w) => title.includes(w) || item.files.includes(w))) score = 2;
      else if (words.every((w) => item.text.includes(w) || title.includes(w))) score = 1;
      if (score) scored.push({ item, score });
    }
    scored.sort((a, b) => b.score - a.score);
    const page: Result[] = scored.slice(0, 8).map(({ item, score }) => ({
      kind: 'section',
      id: item.id,
      title: highlight(item.title, words),
      context: escapeHtml(score === 1 ? snippet(item.raw, words[0]) : item.context || 'Section'),
    }));
    const found: Result[] = others
      .filter((r) => words.every((w) => `${r.name} ${r.detail}`.toLowerCase().includes(w)))
      .slice(0, 6)
      .map((r) => ({ kind: 'repo', href: `/${r.key}`, title: highlight(r.name, words), context: escapeHtml(r.detail) }));
    return { page, repos: found };
  });
  const shown = $derived([...results.page, ...results.repos]);

  $effect(() => {
    void query;
    sel = 0;
  });

  function choose(result: Result | undefined) {
    if (!result) return;
    if (result.kind === 'repo') {
      location.href = result.href;
      return;
    }
    query = '';
    open = false;
    input?.blur();
    ongo(result.id);
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' && shown.length) sel = (sel + 1) % shown.length;
    else if (event.key === 'ArrowUp' && shown.length) sel = (sel - 1 + shown.length) % shown.length;
    else if (event.key === 'Enter') choose(shown[sel]);
    else if (event.key === 'Escape') {
      query = '';
      open = false;
      input?.blur();
      onclose();
    } else return;
    event.preventDefault();
  }

  function onFocus() {
    open = true;
    void repos().then((list) => (others = list));
  }
</script>

<div class="find" role="search">
  <input
    bind:this={input}
    bind:value={query}
    type="search"
    placeholder="Find sections and wikis"
    autocomplete="off"
    spellcheck="false"
    role="combobox"
    aria-label="Find sections and wikis"
    aria-controls="lw-find-results"
    aria-expanded={open && !!query.trim()}
    aria-autocomplete="list"
    aria-activedescendant={open && shown.length ? `lw-find-${sel}` : undefined}
    onfocus={onFocus}
    onblur={() => setTimeout(() => (open = false), 150)}
    onkeydown={onKey}
  />
  <Icon name="search" size={24} class="find-icon" />
  {#if open && query.trim()}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="find-results" id="lw-find-results" role="listbox" tabindex="-1" onmousedown={(e) => e.preventDefault()}>
      {#if results.page.length}<div class="find-group" role="presentation">On this page</div>{/if}
      {#each results.page as result, i (i)}
        <a class="find-item" class:sel={sel === i} role="option" id="lw-find-{i}" aria-selected={sel === i} href="#{result.kind === 'section' ? result.id : ''}" onclick={(e) => { e.preventDefault(); choose(result); }}>
          <span class="find-title">{@html result.title}</span><span class="find-context">{@html result.context}</span>
        </a>
      {/each}
      {#if results.repos.length}<div class="find-group" role="presentation">Wikis</div>{/if}
      {#each results.repos as result, j (j)}
        {@const i = results.page.length + j}
        <a class="find-item" class:sel={sel === i} role="option" id="lw-find-{i}" aria-selected={sel === i} href={result.kind === 'repo' ? result.href : '/'}>
          <span class="find-title">{@html result.title}</span><span class="find-context">{@html result.context}</span>
        </a>
      {/each}
      {#if !shown.length}<div class="find-none">Nothing found for “{query.trim()}”</div>{/if}
    </div>
  {/if}
</div>
