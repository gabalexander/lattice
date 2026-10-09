<!-- A repo on the home page: its name, where it comes from, its latest version (commit, when, model, cost), and
     while a job runs for it, its progress, live. -->
<script lang="ts">
  import { shortSha } from '$lib/codelinks';
  import { KIND_NAMES, latestVersion, modelName, money, sourceText, timeAgo } from '$lib/format';
  import type { Repo } from '$lib/types';
  import Icon from './Icon.svelte';
  import JobProgress from './JobProgress.svelte';

  let { repo, ondelete }: { repo: Repo; ondelete: (repo: Repo) => void } = $props();

  const latest = $derived(latestVersion(repo.versions));
  const job = $derived(repo.job && ['queued', 'running'].includes(repo.job.state) ? repo.job : null);
  const failed = $derived(repo.job && repo.job.state === 'failed' ? repo.job : null);
  const href = $derived(latest ? `/${repo.key}` : job ? `/jobs/${job.id}` : `/${repo.key}`);
</script>

<article class="card" class:live={!!job}>
  <a class="cover" {href} aria-label="{repo.name}{latest ? '' : job ? ': being written' : ': no wiki yet'}"></a>
  <div class="top">
    <Icon name={repo.source.kind === 'local' ? 'folder' : repo.source.url.includes('github.com') ? 'github' : 'git'} size={18} class="kind" />
    <h3>{repo.name}</h3>
    {#if job}
      <span class="chip dot {job.state}">{job.state === 'queued' ? 'Queued' : KIND_NAMES[job.kind]}</span>
    {:else if failed}
      <span class="chip failed">Failed</span>
    {/if}
  </div>
  <p class="source" title={sourceText(repo.source)}>{sourceText(repo.source)}</p>
  {#if job}
    <div class="job">
      <JobProgress progress={job.progress} state={job.state} compact />
    </div>
  {:else if failed}
    <p class="why">{failed.error || `The ${failed.kind} failed.`}</p>
  {/if}
  <div class="meta">
    {#if latest}
      <span title="Commit {latest.commit}"><code>{shortSha(latest.commit)}</code></span>
      <span>{timeAgo(latest.at)}</span>
      <span>{modelName(latest.model)}</span>
      {#if latest.cost_usd}<span>{money(latest.cost_usd)}</span>{/if}
      {#if repo.versions.length > 1}<span>{repo.versions.length} versions</span>{/if}
    {:else if !job}
      <span>No wiki yet</span>
    {/if}
    <button class="delete round small" type="button" aria-label="Remove {repo.name}" title="Remove" onclick={() => ondelete(repo)}>
      <Icon name="delete" size={18} />
    </button>
  </div>
</article>

<style>
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 18px 18px 10px 20px;
    border-radius: 16px;
    background: var(--panel);
    border: 1px solid var(--panel-line);
    min-width: 0;
    transition: border-color 0.15s, background-color 0.15s;
  }
  .card:hover {
    border-color: var(--rule);
    background: color-mix(in srgb, var(--panel) 92%, var(--text));
  }
  .card:has(.cover:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .cover {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    z-index: 0;
  }
  .cover:focus-visible {
    outline: none;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .top :global(.kind) {
    color: var(--text-3);
  }
  h3 {
    margin: 0;
    font: 500 17px/24px var(--font-display);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1 1 auto;
  }
  .source {
    margin: 0;
    font: 400 12.5px/18px var(--font-code);
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .job {
    margin: 8px 0 2px;
  }
  .why {
    margin: 4px 0 0;
    font-size: 13px;
    line-height: 20px;
    color: var(--error);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 14px;
    margin-top: auto;
    padding-top: 6px;
    font-size: 13px;
    line-height: 20px;
    color: var(--text-2);
  }
  .meta code {
    font-size: 11.5px;
    padding: 2px 5px;
    border-radius: 4px;
    background: var(--code-bg);
    color: var(--code-fg);
  }
  .delete {
    position: relative;
    z-index: 1;
    margin-left: auto;
    color: var(--text-3);
    opacity: 0;
    transition: opacity 0.12s;
  }
  .card:hover .delete,
  .delete:focus-visible {
    opacity: 1;
  }
  @media (hover: none) {
    .delete {
      opacity: 1;
    }
  }
</style>
