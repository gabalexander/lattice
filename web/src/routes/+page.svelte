<!-- Home: the box a wiki starts from, and the wikis made so far as cards, each with its job's progress while
     one runs. The list is read again every couple of seconds while a job runs (every half minute otherwise, to
     see jobs started from the command line), rather than holding a stream open per card. -->
<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import { api, ApiError } from '$lib/api';
  import AppHeader from '$lib/components/AppHeader.svelte';
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
  import RepoCard from '$lib/components/RepoCard.svelte';
  import RepoInput from '$lib/components/RepoInput.svelte';
  import { toast } from '$lib/toast.svelte';
  import type { Model, Repo } from '$lib/types';

  let repos = $state<Repo[] | null>(null);
  let loadError = $state('');
  let model = $state<Model>('sonnet');
  let busy = $state(false);
  let startError = $state('');
  let existing = $state<{ repo: Repo; model: Model } | null>(null);
  let removing = $state<Repo | null>(null);

  const running = $derived(!!repos?.some((r) => r.job && ['queued', 'running'].includes(r.job.state)));
  const sorted = $derived(
    [...(repos ?? [])].sort((a, b) => {
      const live = (r: Repo) => (r.job && ['queued', 'running'].includes(r.job.state) ? 1 : 0);
      const at = (r: Repo) => Math.max(0, ...r.versions.map((v) => Date.parse(v.at) || 0), Date.parse(r.job?.started || '') || 0);
      return live(b) - live(a) || at(b) - at(a);
    }),
  );

  async function refresh() {
    try {
      repos = await api.repos();
      loadError = '';
    } catch (err) {
      loadError = (err as Error).message;
    }
  }

  onMount(() => {
    void api
      .settings()
      .then((s) => (model = s.model.includes('opus') ? 'opus' : 'sonnet'))
      .catch(() => {});
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      if (document.visibilityState === 'visible') await refresh();
      timer = setTimeout(tick, running ? 1500 : 30000);
    };
    void tick();
    const wake = () => {
      if (document.visibilityState === 'visible') {
        clearTimeout(timer);
        void tick();
      }
    };
    document.addEventListener('visibilitychange', wake);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('visibilitychange', wake);
    };
  });

  async function generate(source: string, chosen: Model) {
    busy = true;
    startError = '';
    try {
      const { key } = await api.addRepo(source);
      const all = await api.repos();
      repos = all;
      const repo = all.find((r) => r.key === key);
      if (repo?.job && ['queued', 'running'].includes(repo.job.state)) {
        await goto(`/jobs/${repo.job.id}`);
      } else if (repo && repo.versions.length) {
        existing = { repo, model: chosen };
      } else {
        await start(key, chosen);
      }
    } catch (err) {
      startError = (err as Error).message;
    } finally {
      busy = false;
    }
  }

  async function start(key: string, chosen: Model, kind: 'build' | 'regenerate' = 'build') {
    const job = await api.startJob(key, kind, chosen);
    await goto(`/jobs/${job.id}`);
  }

  async function remove(repo: Repo) {
    removing = null;
    try {
      await api.deleteRepo(repo.key);
      toast.show(`Removed ${repo.name}`);
    } catch (err) {
      toast.show(err instanceof ApiError ? err.message : `Couldn't remove ${repo.name}`);
    }
    await refresh();
  }
</script>

<svelte:head><title>lattice</title></svelte:head>

<AppHeader />

<main class="home">
  <section class="hero">
    <h1>Your code, as a wiki</h1>
    <p class="lede">
      lattice reads a repository with Claude and writes one page about it: an outline, a diagram for every part,
      and every name linked to the line it's defined on. It keeps the page up to date as the code changes, and
      answers questions about the code.
    </p>
    <RepoInput bind:model {busy} error={startError} onsubmit={generate} />
    {#if existing}
      <div class="existing" role="alert">
        <p>There's a wiki for <strong>{existing.repo.name}</strong> already.</p>
        <div class="row">
          <a class="pill small primary" href="/{existing.repo.key}">Open it</a>
          <button class="pill small" type="button" onclick={() => existing && start(existing.repo.key, existing.model, 'regenerate')}>
            Write a new version
          </button>
          <button class="pill small" type="button" onclick={() => (existing = null)}>Cancel</button>
        </div>
      </div>
    {/if}
  </section>

  <section class="wikis" aria-labelledby="wikis-title">
    <h2 id="wikis-title">Your wikis</h2>
    {#if loadError}
      <p class="notice error" role="alert">{loadError}</p>
    {:else if repos === null}
      <div class="grid" aria-busy="true">
        {#each [0, 1, 2] as i (i)}<div class="ghost"></div>{/each}
      </div>
    {:else if !repos.length}
      <div class="empty">
        <p>No wikis yet. Give lattice a repository above: it reads the code with Claude Code, on this machine, with your subscription.</p>
      </div>
    {:else}
      <div class="grid">
        {#each sorted as repo (repo.key)}
          <RepoCard {repo} ondelete={(r) => (removing = r)} />
        {/each}
      </div>
    {/if}
  </section>
</main>

{#if removing}
  <ConfirmDialog
    title="Remove {removing.name}?"
    confirm="Remove"
    danger
    onconfirm={() => removing && remove(removing)}
    oncancel={() => (removing = null)}
  >
    Its wiki and every version of it go{removing.source.kind === 'git' ? ', and so does lattice\'s clone' : ''}.
    {#if removing.source.kind === 'local'}The folder itself is left as it is.{/if}
  </ConfirmDialog>
{/if}

<style>
  .home {
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 25px 80px;
  }
  .hero {
    text-align: center;
    padding: 9vh 0 56px;
  }
  h1 {
    font: 400 44px/52px var(--font-display);
    letter-spacing: -0.015em;
    margin: 0 0 16px;
  }
  .lede {
    max-width: 640px;
    margin: 0 auto 36px;
    font-size: 16px;
    line-height: 26px;
    color: var(--text-2);
  }
  .existing {
    max-width: 760px;
    margin: 16px auto 0;
    padding: 14px 18px;
    border-radius: 16px;
    background: var(--panel);
    border: 1px solid var(--panel-line);
    text-align: left;
  }
  .existing p {
    margin: 0 0 10px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .wikis h2 {
    font: 500 20px/28px var(--font-display);
    margin: 0 0 16px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 340px), 1fr));
    gap: 14px;
  }
  .ghost {
    height: 132px;
    border-radius: 16px;
    background: var(--panel);
    animation: pulse 1.6s ease-in-out infinite;
  }
  .empty {
    padding: 28px;
    border-radius: 16px;
    border: 1px dashed var(--rule);
    color: var(--text-2);
    text-align: center;
  }
  .empty p {
    margin: 0;
  }
  .notice.error {
    color: var(--error);
  }
  @media (max-width: 839px) {
    .home {
      padding: 0 14px 56px;
    }
    .hero {
      padding: 32px 0 36px;
    }
    h1 {
      font-size: 32px;
      line-height: 40px;
    }
    .lede {
      font-size: 15px;
      line-height: 24px;
      margin-bottom: 24px;
    }
  }
</style>
