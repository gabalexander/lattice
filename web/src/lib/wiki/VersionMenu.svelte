<!-- The versions of a repo's wiki, each build kept and tagged with its model, and what can be done next: Sync
     (rewrite what the code changed since), Resume (finish a build that stopped) and Regenerate (start over,
     with either model). After deepwiki-by-cc's version switcher (MIT; see THIRD_PARTY_NOTICES.md). -->
<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import { shortSha } from '$lib/codelinks';
  import { formatDate, modelName, money } from '$lib/format';
  import type { JobKind, Model, Version } from '$lib/types';

  let {
    repoKey,
    versions,
    shown,
    busy,
    canResume,
    onaction,
  }: {
    repoKey: string;
    versions: Version[];
    /** The version on the page. */
    shown: number;
    /** Whether a job runs for the repo now, which the actions wait for. */
    busy: boolean;
    canResume: boolean;
    onaction: (kind: JobKind, model?: Model) => void;
  } = $props();

  let openMenu = $state<'versions' | 'more' | null>(null);
  const sorted = $derived([...versions].sort((a, b) => b.n - a.n));
  const latest = $derived(sorted[0]?.n ?? shown);
  const current = $derived(versions.find((v) => v.n === shown));

  function toggle(which: 'versions' | 'more', event: MouseEvent) {
    openMenu = openMenu === which ? null : which;
    if (openMenu) {
      const wrap = (event.currentTarget as HTMLElement).parentElement;
      requestAnimationFrame(() => wrap?.querySelector<HTMLElement>('[role^="menuitem"][aria-checked="true"], [role^="menuitem"]')?.focus());
    }
  }

  function act(kind: JobKind, model?: Model) {
    openMenu = null;
    onaction(kind, model);
  }

  function onKey(event: KeyboardEvent) {
    const menu = event.currentTarget as HTMLElement;
    const items = [...menu.querySelectorAll<HTMLElement>('[role^="menuitem"]:not([aria-disabled="true"])')];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      items[(at + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    } else if (event.key === 'Escape') {
      event.stopPropagation();
      openMenu = null;
      (menu.parentElement?.querySelector('button') as HTMLElement | null)?.focus();
    } else if (event.key === 'Tab') {
      openMenu = null;
    }
  }
</script>

<svelte:window onclick={(e) => openMenu && !(e.target as Element).closest('.version-tools') && (openMenu = null)} />

<div class="version-tools">
  <div class="menu-wrap">
    <button class="facts" type="button" aria-haspopup="menu" aria-expanded={openMenu === 'versions'} onclick={(e) => toggle('versions', e)} title="Every version of this wiki">
      <Icon name="history" size={18} />
      <span class="v">Version {shown}{#if versions.length > 1}<span class="faint">{' '}of {latest}</span>{/if}</span>
      {#if current}
        <span class="more-facts">
          <span>{modelName(current.model)}</span><span>{formatDate(current.at)}</span><code>{shortSha(current.commit)}</code>{#if current.cost_usd}<span>{money(current.cost_usd)}</span>{/if}
        </span>
      {/if}
      <Icon name="down" size={18} />
    </button>
    {#if openMenu === 'versions'}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="menu versions" role="menu" aria-label="Versions" onkeydown={onKey}>
        {#each sorted as v (v.n)}
          <a role="menuitemradio" aria-checked={v.n === shown} tabindex="-1" href={v.n === latest ? `/${repoKey}` : `/${repoKey}/v/${v.n}`} onclick={() => (openMenu = null)}>
            <span class="vn">v{v.n}</span>
            <span class="vd">
              <span>{[formatDate(v.at), modelName(v.model), money(v.cost_usd)].filter(Boolean).join(' · ')}</span>
              <span class="faint"><code>{shortSha(v.commit)}</code>{v.branch ? ` on ${v.branch}` : ''}{v.n === latest ? ' · latest' : ''}</span>
            </span>
            {#if v.n === shown}<Icon name="check" size={18} />{/if}
          </a>
        {/each}
      </div>
    {/if}
  </div>
  <span class="grow"></span>
  <button class="pill small" type="button" disabled={busy} onclick={() => act('sync')} title="Rewrite what the code has changed since the latest version">
    <Icon name="sync" size={18} />Sync
  </button>
  <div class="menu-wrap">
    <button class="round small" type="button" aria-label="More" title="More" aria-haspopup="menu" aria-expanded={openMenu === 'more'} onclick={(e) => toggle('more', e)}>
      <Icon name="more" size={20} />
    </button>
    {#if openMenu === 'more'}
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div class="menu" role="menu" aria-label="More" onkeydown={onKey}>
        <button role="menuitem" type="button" tabindex="-1" aria-disabled={busy || !canResume} disabled={busy || !canResume} onclick={() => act('resume')}>
          <Icon name="play" size={20} />Resume the last build
        </button>
        <hr />
        <button role="menuitem" type="button" tabindex="-1" aria-disabled={busy} disabled={busy} onclick={() => act('regenerate', 'sonnet')}>
          <Icon name="redo" size={20} />Regenerate with Sonnet
        </button>
        <button role="menuitem" type="button" tabindex="-1" aria-disabled={busy} disabled={busy} onclick={() => act('regenerate', 'opus')}>
          <Icon name="redo" size={20} />Regenerate with Opus
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .version-tools {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }
  .version-tools > .menu-wrap:first-child {
    flex: none;
  }
  .grow {
    flex: 1;
  }
  .facts {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    max-width: 100%;
    padding: 0 8px 0 6px;
    margin-left: -6px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--text-2);
    font-size: 13.5px;
    line-height: 20px;
    white-space: nowrap;
    min-width: 0;
  }
  .facts:hover,
  .facts[aria-expanded='true'] {
    background: var(--hover);
    color: var(--text);
  }
  .v {
    font-weight: 500;
    color: var(--text);
  }
  .more-facts {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
  }
  .more-facts > * + *::before {
    content: '·';
    margin-right: 8px;
    color: var(--text-3);
  }
  .more-facts code {
    font-size: 11.5px;
  }
  .faint {
    color: var(--text-3);
  }
  .versions {
    left: 0;
    right: auto;
    min-width: 290px;
    max-height: 60vh;
    overflow-y: auto;
  }
  .versions a {
    align-items: flex-start !important;
  }
  .vn {
    font: 500 14px/20px var(--font-display);
    min-width: 30px;
  }
  .vd {
    display: grid;
    flex: 1;
    font-size: 13px;
    line-height: 20px;
  }
  .vd code {
    font-size: 11.5px;
  }
  .menu button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  /* The facts the line has room for, by the width of the document it heads. */
  @container doc-meta (max-width: 620px) {
    .more-facts > :nth-child(n + 3) {
      display: none;
    }
  }
  @container doc-meta (max-width: 420px) {
    .more-facts {
      display: none;
    }
  }
  @media (max-width: 839px) {
    .versions {
      min-width: min(290px, calc(100vw - 40px));
    }
  }
</style>
