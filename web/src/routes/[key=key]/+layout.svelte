<!-- A repo's wiki, at /<key> (the latest version) and /<key>/v/<n>: the page itself (WikiView), with the
     versions and the jobs around it. A job running for the repo shows in a banner at the top of the document,
     live; when it makes a version, the banner offers it. A repo with no version yet shows its build instead. -->
<script lang="ts">
  import { pushState, replaceState } from '$app/navigation';
  import { page } from '$app/state';
  import { onDestroy } from 'svelte';
  import { api, ApiError } from '$lib/api';
  import AppHeader from '$lib/components/AppHeader.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import JobProgress from '$lib/components/JobProgress.svelte';
  import { shortSha } from '$lib/codelinks';
  import { formatDate, KIND_NAMES, latestVersion, sourceText } from '$lib/format';
  import { JobStream } from '$lib/jobs.svelte';
  import { toast } from '$lib/toast.svelte';
  import type { JobKind, Model, Repo, RepoStatus, Wiki } from '$lib/types';
  import VersionMenu from '$lib/wiki/VersionMenu.svelte';
  import WikiView from '$lib/wiki/WikiView.svelte';

  let { children } = $props();

  const key = $derived(page.params.key!);
  const asked = $derived(page.params.n ? Number(page.params.n) : null);

  let repos = $state<Repo[]>([]);
  let wiki = $state<Wiki | null>(null);
  let status = $state<RepoStatus | null>(null);
  let failure = $state<{ status: number; message: string } | null>(null);
  let loading = $state(true);
  let stream = $state<JobStream | null>(null);
  /** The repo `stream` is a job of. */
  let streamFor = '';
  let ready = $state<number | null>(null);
  let starting = $state(false);

  const repo = $derived(repos.find((r) => r.key === key) ?? null);
  const latest = $derived(repo ? latestVersion(repo.versions) : null);
  const shown = $derived(asked ?? latest?.n ?? null);
  const shownVersion = $derived(repo?.versions.find((v) => v.n === shown) ?? null);
  const running = $derived(!!stream && !stream.finished);
  const lastJob = $derived(stream?.job ?? repo?.job ?? null);

  async function loadRepos() {
    repos = await api.repos();
  }

  async function load(key: string, version: number | null) {
    if (stream && streamFor !== key) {
      stream.close();
      stream = null;
    }
    loading = true;
    failure = null;
    ready = null;
    try {
      await loadRepos();
      const found = repos.find((r) => r.key === key);
      if (!found) {
        failure = { status: 404, message: 'lattice has no repository by that name.' };
        return;
      }
      follow(found);
      if (!found.versions.length) {
        wiki = null;
        return;
      }
      const [w, s] = await Promise.all([api.wiki(key, version), api.status(key).catch(() => null)]);
      wiki = w;
      status = s;
    } catch (err) {
      const status = err instanceof ApiError ? err.status : 0;
      failure = { status, message: status === 404 ? `There's no version ${version} of this wiki.` : (err as Error).message };
    } finally {
      loading = false;
    }
  }

  // Follows the job running for the repo, if one is, for the banner.
  function follow(r: Repo) {
    const job = r.job;
    if (!job || !['queued', 'running'].includes(job.state)) return;
    if (stream?.id === job.id) return;
    stream?.close();
    const s = new JobStream(job.id, async (version) => {
      await loadRepos();
      if (!wiki) await load(key, null);
      else {
        ready = version;
        toast.show(`Version ${version} is ready`);
      }
    });
    stream = s;
    streamFor = r.key;
    void s.start();
  }

  $effect(() => {
    void load(key, asked);
  });

  onDestroy(() => stream?.close());

  async function act(kind: JobKind, model?: Model) {
    if (starting) return;
    starting = true;
    try {
      const job = await api.startJob(key, kind, model ?? null);
      await loadRepos();
      const r = repos.find((x) => x.key === key);
      if (r) follow({ ...r, job });
      toast.show(`${KIND_NAMES[kind]} started`);
    } catch (err) {
      toast.show((err as Error).message);
    } finally {
      starting = false;
    }
  }

  const otherRepos = async () =>
    (await api.repos().catch(() => repos))
      .filter((r) => r.key !== key && r.versions.length)
      .map((r) => ({ key: r.key, name: r.name, detail: sourceText(r.source) }));

  const setHash = (hash: string, push: boolean) => (push ? pushState : replaceState)(hash, {});
</script>

{@render children()}

{#if wiki && repo}
  {#key wiki}
    <WikiView {wiki} mode="serve" repoKey={key} codeRoot={repo.root} {otherRepos} {setHash}>
      {#snippet headTools()}
        <VersionMenu
          repoKey={key}
          versions={repo.versions}
          shown={shown ?? 1}
          busy={running || starting}
          canResume={!!lastJob && ['failed', 'cancelled'].includes(lastJob.state)}
          onaction={act}
        />
      {/snippet}
      {#snippet banner()}
        {#if stream && running}
          <div class="banner" role="status">
            <span class="spinner small" aria-hidden="true"></span>
            <div class="grow">
              <JobProgress progress={stream.progress} state={stream.job?.state ?? 'queued'} compact />
            </div>
            <a href="/jobs/{stream.id}">Log</a>
          </div>
        {:else if ready}
          <div class="banner" role="status">
            <Icon name="check" size={20} />
            <span class="grow">Version {ready} is ready.</span>
            <a class="pill small primary" href="/{key}" onclick={() => (ready = null)}>Show it</a>
          </div>
        {:else if lastJob && lastJob.state === 'failed' && shown === latest?.n}
          <div class="banner warn" role="alert">
            <Icon name="warn" size={20} />
            <span class="grow">The last {KIND_NAMES[lastJob.kind].toLowerCase()} failed{lastJob.error ? `: ${lastJob.error}` : '.'}</span>
            <a href="/jobs/{lastJob.id}">Log</a>
            <button class="pill small" type="button" onclick={() => act('resume')}>Resume</button>
          </div>
        {:else if shownVersion && latest && shownVersion.n !== latest.n}
          <div class="banner">
            <Icon name="history" size={20} />
            <span class="grow">You're reading version {shownVersion.n}, from {formatDate(shownVersion.at)}; the latest is version {latest.n}.</span>
            <a class="pill small" href="/{key}">Read the latest</a>
          </div>
        {:else if status?.stale}
          <div class="banner warn">
            <Icon name="sync" size={20} />
            <span class="grow">
              The code has moved on since this version{status.commit && status.head ? ` (${shortSha(status.commit)} → ${shortSha(status.head)})` : ''}.
            </span>
            <button class="pill small accent" type="button" disabled={starting} onclick={() => act('sync')}><Icon name="sync" size={18} />Sync</button>
          </div>
        {/if}
      {/snippet}
    </WikiView>
  {/key}
{:else}
  <AppHeader />
  <main class="state">
    {#if loading}
      <div class="spinner" aria-label="Loading"></div>
    {:else if failure}
      <h1>{failure.status === 404 ? 'Nothing here' : "Couldn't show this wiki"}</h1>
      <p>{failure.message}</p>
      <a class="pill" href="/">Every wiki</a>
    {:else if repo}
      <h1>{repo.name}</h1>
      <p class="source">{sourceText(repo.source)}</p>
      {#if stream && running}
        <div class="panel">
          <JobProgress progress={stream.progress} state={stream.job?.state ?? 'queued'} />
          <a class="pill small" href="/jobs/{stream.id}"><Icon name="terminal" size={18} />Watch it being written</a>
        </div>
      {:else}
        <p>{lastJob?.state === 'failed' ? `Its build failed${lastJob.error ? `: ${lastJob.error}` : '.'}` : "There's no wiki for it yet."}</p>
        <div class="row">
          {#if lastJob && ['failed', 'cancelled'].includes(lastJob.state)}
            <button class="pill primary" type="button" disabled={starting} onclick={() => act('resume')}>Resume</button>
          {/if}
          <button class="pill {lastJob ? '' : 'primary'}" type="button" disabled={starting} onclick={() => act('build', 'sonnet')}>Write it with Sonnet</button>
          <button class="pill" type="button" disabled={starting} onclick={() => act('build', 'opus')}>With Opus</button>
        </div>
      {/if}
    {/if}
  </main>
{/if}

<style>
  .state {
    max-width: 640px;
    margin: 10vh auto 0;
    padding: 0 24px;
    text-align: center;
  }
  h1 {
    font: 400 32px/40px var(--font-display);
    margin: 0 0 6px;
    overflow-wrap: anywhere;
  }
  p {
    color: var(--text-2);
    margin: 0 0 22px;
  }
  .source {
    font: 400 13px/20px var(--font-code);
    color: var(--text-3);
  }
  .panel {
    display: grid;
    gap: 18px;
    justify-items: center;
    text-align: left;
    padding: 22px 25px;
    border-radius: 16px;
    background: var(--panel);
    border: 1px solid var(--panel-line);
  }
  .panel > :global(.progress) {
    width: 100%;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 10px;
  }
</style>
