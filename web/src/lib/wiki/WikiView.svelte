<!-- A wiki's page, as Code Wiki lays one out: the header (find, theme, help, share, chat), the outline on the
     left following the reader down, the document in the middle (the title, what made it, the overview beside
     its diagram, then every section and subsection with its diagram card), and the chat on the right. The app
     serves it at /<key>, with the versions and the jobs around it (RepoWiki.svelte); an export shows it alone.
     Carried over from crystal's wiki page (assets/wiki/app.js). -->
<script lang="ts">
  import './wiki.css';
  import { onMount, tick, type Snippet } from 'svelte';
  import { api } from '$lib/api';
  import Icon from '$lib/components/Icon.svelte';
  import Logo from '$lib/components/Logo.svelte';
  import ThemeToggle from '$lib/components/ThemeToggle.svelte';
  import { clickAction, codeTarget, commitUrl, forgeOf, openQuery, shortSha, type Mode } from '$lib/codelinks';
  import { formatDate, modelName, money } from '$lib/format';
  import { plainText, type MarkdownOptions } from '$lib/markdown';
  import type { Drawing } from '$lib/mermaid';
  import { theme } from '$lib/theme.svelte';
  import { copyText, toast } from '$lib/toast.svelte';
  import type { Wiki } from '$lib/types';
  import Chat from './Chat.svelte';
  import DiagramCard from './DiagramCard.svelte';
  import FindBox, { type FindItem, type FindRepo } from './FindBox.svelte';
  import HelpDialog from './HelpDialog.svelte';
  import Outline, { type Entry } from './Outline.svelte';
  import Prose from './Prose.svelte';
  import ZoomDialog from './ZoomDialog.svelte';

  let {
    wiki,
    mode,
    repoKey = null,
    home = '/',
    otherRepos,
    setHash = (hash: string, push: boolean) => history[push ? 'pushState' : 'replaceState'](history.state, '', hash),
    headTools,
    banner,
  }: {
    wiki: Wiki;
    mode: Mode;
    /** The repo's key, for asking and opening files; null in an export. */
    repoKey?: string | null;
    /** Where the mark leads: the list of wikis, or nowhere in an export. */
    home?: string | null;
    otherRepos?: () => Promise<FindRepo[]>;
    /** Puts `#id` in the address, as the router in use wants it done. */
    setHash?: (hash: string, push: boolean) => void;
    headTools?: Snippet;
    banner?: Snippet;
  } = $props();

  const WIDE = 1180;
  const PHONE = 840;
  const CHAT_KEY = 'lattice-chat';
  const mac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

  const repo = $derived(wiki.repo);
  const generated = $derived(wiki.generated || { at: '' });
  const name = $derived(repo.name || 'Repository');
  const options: MarkdownOptions = $derived({ code: codeTarget(repo, mode, mac), headingOffset: 3 });
  const sectionOptions: MarkdownOptions = $derived({ ...options, headingOffset: 2 });

  // Ids are the page's anchors; a wiki that repeats one has the repeat numbered rather than lost.
  const layout = $derived.by(() => {
    const seen = new Set<string>();
    const unique = (id: string, title: string) => {
      const base = String(id || title || 'section').trim().replace(/\s+/g, '-') || 'section';
      let out = base;
      for (let n = 2; seen.has(out); n++) out = `${base}-${n}`;
      seen.add(out);
      return out;
    };
    const entries: Entry[] = [];
    const find: FindItem[] = [];
    const sections = (wiki.sections || []).map((section) => {
      const id = unique(section.id, section.title);
      const index = entries.push({ id, title: section.title || id, level: 2, section: entries.length }) - 1;
      const raw = plainText(section.summary_md);
      find.push({ id, title: section.title || id, context: '', text: raw.toLowerCase(), raw, files: '' });
      const subsections = (section.subsections || []).map((sub) => {
        const subId = unique(sub.id, sub.title);
        entries.push({ id: subId, title: sub.title || subId, level: 3, section: index });
        const subRaw = plainText(sub.body_md);
        find.push({ id: subId, title: sub.title || subId, context: section.title, text: subRaw.toLowerCase(), raw: subRaw, files: (sub.files || []).join(' ').toLowerCase() });
        return { ...sub, id: subId };
      });
      return { ...section, id, subsections };
    });
    return { entries, find, sections };
  });

  let active = $state(0);
  let chatOpen = $state(false);
  let outlineOpen = $state(false);
  let findOpen = $state(false);
  let helpOpen = $state(false);
  let zoom = $state<{ drawing: Drawing; caption: string } | null>(null);
  let findInput: HTMLInputElement | undefined = $state();
  let top: HTMLElement;
  let chat: Chat | undefined = $state();
  let lockUntil = 0;

  const isWide = () => innerWidth >= WIDE;
  const isPhone = () => innerWidth < PHONE;
  const reduceMotion = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  const stored = (key: string) => {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  };

  function setChat(open: boolean, remember = true) {
    chatOpen = open;
    if (remember && isWide()) {
      try {
        localStorage.setItem(CHAT_KEY, open ? 'open' : 'closed');
      } catch {
        // Kept for this page only.
      }
    }
    if (open && !isWide()) setTimeout(() => chat?.focus(), 50);
  }

  // ---- Where the reader is --------------------------------------------------------------------------------

  // The entry in view is the topmost heading in a band at the top of the window; with none in it, the one last
  // scrolled past. At the bottom of the page, where the last headings can't reach the band, the last in sight.
  function watchHeadings(): () => void {
    const visible = new Set<number>();
    const index = new Map(layout.entries.map((e, i) => [e.id, i]));
    const observer = new IntersectionObserver(
      (records) => {
        let above = Infinity;
        for (const record of records) {
          const i = index.get(record.target.id);
          if (i === undefined) continue;
          if (record.isIntersecting) visible.add(i);
          else {
            visible.delete(i);
            if (record.rootBounds && record.boundingClientRect.top >= record.rootBounds.bottom) above = Math.min(above, i - 1);
          }
        }
        if (performance.now() < lockUntil) return;
        if (visible.size) active = Math.min(...visible);
        else if (above !== Infinity) active = Math.max(0, above);
      },
      { rootMargin: `-${top?.offsetHeight ?? 108}px 0px -62% 0px` },
    );
    for (const entry of layout.entries) {
      const el = document.getElementById(entry.id);
      if (el) observer.observe(el);
    }
    return () => observer.disconnect();
  }

  let scrollQueued = false;
  function onScroll() {
    if (scrollQueued) return;
    scrollQueued = true;
    requestAnimationFrame(() => {
      scrollQueued = false;
      const doc = document.documentElement;
      if (innerHeight + scrollY < doc.scrollHeight - 4 || performance.now() < lockUntil) return;
      for (let i = layout.entries.length - 1; i >= 0; i--) {
        const el = document.getElementById(layout.entries[i].id);
        if (el && el.getBoundingClientRect().top < innerHeight * 0.85) {
          active = i;
          break;
        }
      }
    });
  }

  function goTo(id: string, push = true) {
    const el = document.getElementById(id);
    if (!el) return;
    const i = layout.entries.findIndex((e) => e.id === id);
    if (i >= 0) {
      active = i;
      lockUntil = performance.now() + (reduceMotion() ? 100 : 900);
    }
    el.scrollIntoView({ behavior: reduceMotion() ? 'auto' : 'smooth', block: 'start' });
    setHash(`#${encodeURIComponent(id)}`, push);
    outlineOpen = false;
  }

  function step(delta: number) {
    const to = Math.max(0, Math.min(layout.entries.length - 1, active + delta));
    if (layout.entries[to]) goTo(layout.entries[to].id, false);
  }

  // ---- Clicks ---------------------------------------------------------------------------------------------

  async function openInEditor(link: HTMLElement) {
    const path = link.dataset.path || '';
    const line = link.dataset.line;
    if (!repoKey) return;
    try {
      await api.open(repoKey, openQuery(path, line));
      toast.show(`Opened ${path}${line ? `:${line}` : ''} in your editor`);
    } catch (err) {
      toast.show((err as { status?: number }).status === 404 ? `${path} isn't in the repository` : `Couldn't open ${path}: ${(err as Error).message}`);
    }
  }

  function onCodeClick(event: MouseEvent) {
    const link = (event.target as Element).closest<HTMLAnchorElement>('a.code-link');
    if (!link) return;
    const action = clickAction(mode, { meta: event.metaKey, ctrl: event.ctrlKey, shift: event.shiftKey, alt: event.altKey }, link.hasAttribute('href'));
    if (action === 'browser') return;
    event.preventDefault();
    if (action === 'editor') void openInEditor(link);
  }

  function onDocClick(event: MouseEvent) {
    const target = event.target as Element;
    const anchor = target.closest<HTMLElement>('button.anchor');
    if (anchor) {
      setHash(`#${encodeURIComponent(anchor.dataset.anchor || '')}`, false);
      void share(anchor.dataset.anchor || null);
      return;
    }
    const local = target.closest<HTMLAnchorElement>('a[href^="#"]');
    if (local && !local.classList.contains('code-link') && !event.metaKey && !event.ctrlKey) {
      const id = decodeURIComponent(local.getAttribute('href')!.slice(1));
      if (document.getElementById(id)) {
        event.preventDefault();
        goTo(id);
      }
      return;
    }
    onCodeClick(event);
  }

  async function share(id: string | null) {
    const entry = id ? layout.entries.find((e) => e.id === id) : layout.entries[active];
    const url = `${location.href.split('#')[0]}${entry ? `#${encodeURIComponent(entry.id)}` : ''}`;
    if (await copyText(url)) toast.show(entry ? `Copied a link to “${entry.title}”` : 'Copied a link to this page');
    else toast.show(url);
  }

  function onKey(event: KeyboardEvent) {
    const target = event.target as Element;
    const link = target.closest?.('a.code-link:not([href])');
    if (link && event.key === 'Enter') {
      event.preventDefault();
      void openInEditor(link as HTMLElement);
      return;
    }
    if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey) return;
    const typing = target.closest?.('input, textarea, select, [contenteditable="true"]');
    if (event.key === 'Escape') {
      if (outlineOpen) outlineOpen = false;
      else if (findOpen) findOpen = false;
      else if (chatOpen && !isWide()) setChat(false);
      return;
    }
    if (typing || document.querySelector('dialog[open]')) return;
    if (event.key === '/') {
      event.preventDefault();
      if (isPhone()) findOpen = true;
      void tick().then(() => findInput?.focus());
    } else if (event.key === 'c') {
      setChat(!chatOpen);
    } else if (event.key === 'j') {
      step(1);
    } else if (event.key === 'k') {
      step(-1);
    }
  }

  // ---- Starting --------------------------------------------------------------------------------------------

  onMount(() => {
    setChat(isWide() && stored(CHAT_KEY) !== 'closed', false);
    let unwatch = watchHeadings();
    const id = decodeURIComponent(location.hash.slice(1));
    const el = id ? document.getElementById(id) : null;
    if (el) {
      el.scrollIntoView({ block: 'start' });
      const i = layout.entries.findIndex((e) => e.id === id);
      if (i >= 0) active = i;
      // The fonts can move it once they're in; it's put back unless the reader has moved since.
      const y = scrollY;
      void document.fonts.ready.then(() => Math.abs(scrollY - y) < 2 && el.scrollIntoView({ block: 'start' }));
    }
    let wasWide = isWide();
    let wasPhone = isPhone();
    const resize = () => {
      if (isWide() !== wasWide) {
        wasWide = isWide();
        setChat(wasWide && stored(CHAT_KEY) !== 'closed', false);
      }
      if (isPhone() !== wasPhone) {
        wasPhone = isPhone();
        outlineOpen = false;
        // The headings' band moves with the header's height.
        unwatch();
        unwatch = watchHeadings();
      }
    };
    addEventListener('resize', resize);
    performance.mark('lattice-wiki-content');
    return () => {
      unwatch();
      removeEventListener('resize', resize);
    };
  });

  const made = $derived(
    `This page about ${name} was written from its code by ${generated.by || 'Claude'}${generated.at ? ` on ${formatDate(generated.at)}` : ''}, at commit ${shortSha(repo.commit)}${typeof generated.cost_usd === 'number' ? `, for ${money(generated.cost_usd)}` : ''}.`,
  );
  const forge = $derived(repo.web_url ? forgeOf(repo.web_url) : null);
  const onzoom = (drawing: Drawing, caption: string) => (zoom = { drawing, caption });
  const sectionNow = () => layout.entries[active]?.id ?? null;
  const suggestions = $derived(
    layout.sections.length ? [`How does ${layout.sections[0].title.toLowerCase()} work?`, `Where should I start reading ${name.split('/').pop()}?`] : [],
  );
</script>

<svelte:window onscroll={onScroll} onkeydown={onKey} />
<svelte:head><title>{name} · lattice</title></svelte:head>

<div class="wiki" class:chat-open={chatOpen} class:outline-open={outlineOpen} class:find-open={findOpen}>
  <a class="skip" href="#lw-doc-body">Skip to content</a>

  <header class="top" bind:this={top}>
    <div class="brand">
      <button class="round only-narrow" type="button" aria-label="On this page" aria-controls="lw-outline" aria-expanded={outlineOpen} onclick={() => (outlineOpen = !outlineOpen)}>
        <Icon name="menu" />
      </button>
      <Logo href={home} />
    </div>
    <FindBox items={layout.find} repos={otherRepos} ongo={(id) => goTo(id)} onclose={() => (findOpen = false)} bind:input={findInput} />
    <div class="actions">
      <button class="round only-narrow" type="button" aria-label="Find" title="Find" onclick={() => { findOpen = true; void tick().then(() => findInput?.focus()); }}>
        <Icon name="search" />
      </button>
      <ThemeToggle />
      <button class="round help-btn" type="button" aria-label="Help" title="Help" onclick={() => (helpOpen = true)}><Icon name="help" /></button>
      <button class="pill" type="button" title="Copy a link to the section in view" onclick={() => share(null)}><Icon name="share" size={20} /><span>Share</span></button>
      <button class="pill accent" type="button" aria-controls="lw-chat" aria-pressed={chatOpen} onclick={() => setChat(!chatOpen)}><Icon name="spark" size={20} /><span>Chat</span></button>
    </div>
  </header>

  <div class="layout">
    <div id="lw-outline" class="outline-host">
      <Outline
        entries={layout.entries}
        {active}
        updated={formatDate(generated.at)}
        commit={shortSha(repo.commit)}
        commitHref={commitUrl(repo)}
        note={`${generated.by || 'Claude'} can make mistakes, so double-check it.`}
        ongo={(id) => goTo(id)}
      >
        {#snippet tools()}
          <button class="pill small" type="button" onclick={() => theme.cycle()}><Icon name="contrast" size={18} />Theme: {theme.pref}</button>
          <button class="pill small" type="button" onclick={() => (helpOpen = true)}><Icon name="help" size={18} />Help</button>
        {/snippet}
      </Outline>
    </div>
    {#if outlineOpen}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="scrim" aria-hidden="true" onclick={() => (outlineOpen = false)}></div>
    {/if}

    <main class="doc">
      <div class="doc-head">
        <div class="title-row">
        <h1>{name}</h1>
        <span class="badge" title="Written by {[generated.by, formatDate(generated.at), money(generated.cost_usd)].filter(Boolean).join(' · ')}">
          <Icon name="spark" size={18} />Made with Claude{#if generated.model}<span class="sr-only"> {modelName(generated.model)}</span>{/if}
        </span>
        <span class="grow"></span>
          {#if repo.web_url}
            <a class="repo-link" href={repo.web_url} target="_blank" rel="noopener noreferrer" aria-label="{name} on {forge === 'github' ? 'GitHub' : forge === 'gitlab' ? 'GitLab' : 'its forge'}" title="{name} on {forge === 'github' ? 'GitHub' : forge === 'gitlab' ? 'GitLab' : 'its forge'}">
              <Icon name={forge === 'github' ? 'github' : forge === 'gitlab' ? 'gitlab' : 'open'} size={24} />
            </a>
          {/if}
        </div>
        {#if headTools}<div class="doc-meta">{@render headTools()}</div>{/if}
      </div>
      {@render banner?.()}

      <!-- Clicks on its links and buttons are taken here, once, rather than on each of thousands; the keys
           reach them as they would anyway. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="doc-body" id="lw-doc-body" tabindex="-1" onclick={onDocClick}>
        {#if wiki.version !== 1}
          <p class="notice">This wiki says it's version {wiki.version}, which this page doesn't know; some of it may not show.</p>
        {/if}
        <section class="overview" aria-label="Overview">
          <div class="overview-grid" class:no-diagram={!wiki.overview?.diagram?.mermaid}>
            {#if wiki.overview?.diagram?.mermaid}
              <DiagramCard src={wiki.overview.diagram.mermaid} caption={wiki.overview.diagram.caption || `${name} at a glance`} {onzoom} />
            {/if}
            <Prose md={wiki.overview?.summary_md || ''} options={sectionOptions} {onzoom} />
          </div>
        </section>
        {#each layout.sections as section (section.id)}
          <section class="sec">
            <h2 class="heading" id={section.id}>
              <span>{section.title}</span>
              <button class="anchor" type="button" data-anchor={section.id} aria-label="Copy a link to “{section.title}”" title="Copy link"><Icon name="link" /></button>
            </h2>
            {#if section.diagram?.mermaid}<DiagramCard src={section.diagram.mermaid} caption={section.diagram.caption || section.title} {onzoom} />{/if}
            <Prose md={section.summary_md || ''} options={sectionOptions} {onzoom} />
            {#each section.subsections as sub (sub.id)}
              <section class="sub">
                <h3 class="heading" id={sub.id}>
                  <span>{sub.title}</span>
                  <button class="anchor" type="button" data-anchor={sub.id} aria-label="Copy a link to “{sub.title}”" title="Copy link"><Icon name="link" /></button>
                </h3>
                {#if sub.diagram?.mermaid}<DiagramCard src={sub.diagram.mermaid} caption={sub.diagram.caption || sub.title} {onzoom} />{/if}
                <Prose md={sub.body_md || ''} {options} {onzoom} />
              </section>
            {/each}
          </section>
        {/each}
      </div>
    </main>

    <div id="lw-chat" class="chat-host">
      <Chat
        bind:this={chat}
        {mode}
        {repoKey}
        repoName={name}
        {options}
        section={sectionNow}
        {suggestions}
        {onzoom}
        onclose={() => setChat(false)}
        oncodeclick={onCodeClick}
      />
    </div>
  </div>

  {#if zoom}<ZoomDialog drawing={zoom.drawing} caption={zoom.caption} onclose={() => (zoom = null)} />{/if}
  {#if helpOpen}<HelpDialog {made} {mode} onclose={() => (helpOpen = false)} />{/if}
</div>

<style>
  .outline-host,
  .chat-host {
    display: contents;
  }
</style>
