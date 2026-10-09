<!-- A job's page: what it's doing (its phase, how far along, the subsection it's writing, what it has cost),
     the subsections written so far, and its log, live; Cancel while it runs. When it's done, it moves on to the
     version it made. -->
<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { onMount, tick } from 'svelte';
  import { api } from '$lib/api';
  import AppHeader from '$lib/components/AppHeader.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import JobProgress from '$lib/components/JobProgress.svelte';
  import { duration, KIND_NAMES, modelName, money } from '$lib/format';
  import { JobStream } from '$lib/jobs.svelte';
  import type { Repo } from '$lib/types';

  const id = Number(page.params.id);
  let repo = $state<Repo | null>(null);
  let now = $state(Date.now());
  let cancelling = $state(false);
  let follow = $state(true);
  let logEl: HTMLElement | undefined = $state();

  const stream = new JobStream(id, (version) => {
    const key = stream.job?.repo;
    if (key) setTimeout(() => goto(`/${key}/v/${version}`), 900);
  });

  const job = $derived(stream.job);
  const jobState = $derived(job?.state ?? 'queued');
  const elapsed = $derived(job?.started ? (job.finished ? Date.parse(job.finished) : now) - Date.parse(job.started) : 0);
  const title = $derived(repo?.name ?? job?.repo ?? `Job ${id}`);
  const progress = $derived(stream.progress);
  const written = $derived(
    !progress || progress.phase === 'plan' ? '—' : progress.phase === 'write' ? `${progress.done} of ${progress.total}` : 'All',
  );
  /** The subsections written before this page started following the job, which it has no titles for. */
  const before = $derived(
    !progress || progress.phase === 'plan' ? 0 : Math.max(0, (progress.phase === 'write' ? progress.done : progress.total) - stream.written.length),
  );
  const STATE_WORDS = { queued: 'Queued', running: 'Running', done: 'Done', failed: 'Failed', cancelled: 'Cancelled' } as const;

  onMount(() => {
    void stream.start().then(async () => {
      if (stream.job) repo = (await api.repos().catch(() => [])).find((r) => r.key === stream.job?.repo) ?? null;
    });
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => {
      clearInterval(timer);
      stream.close();
    };
  });

  // The log keeps to its end while the reader hasn't scrolled up it.
  $effect(() => {
    void stream.log.length;
    if (follow && logEl) void tick().then(() => logEl && (logEl.scrollTop = logEl.scrollHeight));
  });

  function onLogScroll() {
    if (!logEl) return;
    follow = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
  }

  async function cancel() {
    cancelling = true;
    try {
      await stream.cancel();
    } catch (err) {
      stream.error = (err as Error).message;
    } finally {
      cancelling = false;
    }
  }
</script>

<svelte:head><title>{KIND_NAMES[job?.kind ?? 'build']} · {title} · lattice</title></svelte:head>

<AppHeader />

<main class="job">
  <a class="back" href={repo?.versions.length ? `/${repo.key}` : '/'}><Icon name="back" size={18} />{repo?.versions.length ? title : 'Every wiki'}</a>

  <section class="panel head" aria-labelledby="job-title">
    <div class="title-row">
      <h1 id="job-title">{title}</h1>
      <span class="chip dot {jobState === 'done' ? 'done' : jobState === 'failed' ? 'failed' : jobState === 'running' ? 'running' : jobState === 'queued' ? 'queued' : ''}">
        {[KIND_NAMES[job?.kind ?? 'build'], job?.model ? modelName(job.model) : null, STATE_WORDS[jobState]].filter(Boolean).join(' · ')}
      </span>
      <span class="spacer"></span>
      {#if job && !stream.finished}
        <button class="pill small danger" type="button" onclick={cancel} disabled={cancelling}>
          {cancelling ? 'Cancelling' : 'Cancel'}
        </button>
      {/if}
    </div>
    <dl class="facts">
      <div><dt>Running for</dt><dd>{job?.started ? duration(elapsed) : '—'}</dd></div>
      <div><dt>Cost so far</dt><dd>{money(stream.progress?.cost_usd) || '—'}</dd></div>
      <div><dt>Subsections written</dt><dd>{written}</dd></div>
    </dl>
    {#if job}
      <JobProgress progress={stream.progress} state={jobState} />
    {/if}
    {#if stream.reconnecting}
      <p class="note">Lost touch with lattice; trying again…</p>
    {/if}
    {#if jobState === 'done'}
      <p class="ok" role="status">
        <Icon name="check" size={20} />Version {stream.version ?? job?.version} is ready.
        {#if job && (stream.version ?? job.version)}<a href="/{job.repo}/v/{stream.version ?? job.version}">Open the wiki</a>{/if}
      </p>
    {:else if jobState === 'failed' || jobState === 'cancelled'}
      <div class="failed" role="alert">
        <Icon name="warn" size={20} />
        <div>
          <p>{jobState === 'cancelled' ? 'This job was cancelled.' : stream.error || 'This job failed.'}</p>
          {#if job}
            <button class="pill small" type="button" onclick={async () => job && goto(`/jobs/${(await api.startJob(job.repo, 'resume')).id}`)}>
              <Icon name="play" size={18} />Resume
            </button>
          {/if}
        </div>
      </div>
    {:else if stream.error}
      <p class="failed" role="alert">{stream.error}</p>
    {/if}
  </section>

  <div class="cols">
    <section class="panel written" aria-labelledby="written-title">
      <h2 id="written-title">Subsections</h2>
      {#if before || stream.written.length || (stream.progress?.phase === 'write' && stream.progress.current)}
        <ol>
          {#if before}<li class="earlier">{before === 1 ? 'One' : before} written before this page opened</li>{/if}
          {#each stream.written as item, i (i)}
            <li><Icon name="check" size={16} />{item}</li>
          {/each}
          {#if stream.progress?.phase === 'write' && stream.progress.current && jobState === 'running'}
            <li class="now"><span class="spinner small"></span>{stream.progress.current}</li>
          {/if}
        </ol>
      {:else}
        <p class="note">{jobState === 'queued' ? 'It starts as soon as lattice has room for it.' : stream.progress?.phase === 'plan' || !stream.progress ? 'Planning the outline from the code…' : 'Nothing written in this job.'}</p>
      {/if}
    </section>

    <section class="panel log-panel" aria-labelledby="log-title">
      <div class="log-head">
        <h2 id="log-title">Log</h2>
        {#if !follow}<button class="pill small" type="button" onclick={() => { follow = true; if (logEl) logEl.scrollTop = logEl.scrollHeight; }}>Follow</button>{/if}
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre class="log" bind:this={logEl} onscroll={onLogScroll} tabindex="0" aria-label="The job's log" aria-live="off">{#each stream.log as line, i (i)}{line}{'\n'}{/each}{#if !stream.log.length}<span class="note">No log yet.</span>{/if}</pre>
    </section>
  </div>
</main>

<style>
  .job {
    max-width: 1200px;
    margin: 0 auto;
    padding: 4px 25px 56px;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    text-decoration: none;
    font-size: 14px;
    margin: 0 0 14px;
    border-radius: 6px;
  }
  .back:hover {
    color: var(--text);
  }
  .panel {
    background: var(--panel);
    border: 1px solid var(--panel-line);
    border-radius: 16px;
    padding: 22px 25px;
    min-width: 0;
  }
  .head {
    display: grid;
    gap: 18px;
    margin-bottom: 14px;
  }
  .title-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px 16px;
  }
  h1 {
    font: 400 26px/34px var(--font-display);
    margin: 0;
    overflow-wrap: anywhere;
  }
  .spacer {
    flex: 1;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 40px;
    margin: 0;
  }
  .facts div {
    display: grid;
  }
  .facts dt {
    font-size: 12px;
    line-height: 18px;
    color: var(--text-3);
  }
  .facts dd {
    margin: 0;
    font: 400 20px/28px var(--font-display);
    font-variant-numeric: tabular-nums;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: var(--ok);
  }
  .failed {
    display: flex;
    gap: 10px;
    margin: 0;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--error-soft);
    color: var(--error);
    line-height: 22px;
  }
  .failed p {
    margin: 0 0 10px;
    overflow-wrap: anywhere;
  }
  .note {
    color: var(--text-3);
    margin: 0;
    font-size: 13px;
  }
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
    gap: 14px;
    align-items: start;
  }
  h2 {
    font: 500 16px/24px var(--font-display);
    margin: 0 0 10px;
  }
  .written ol {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 60vh;
    overflow-y: auto;
  }
  .written li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 0;
    line-height: 22px;
    color: var(--text-2);
    border-bottom: 1px solid color-mix(in srgb, var(--rule) 50%, transparent);
  }
  .written li :global(.icon) {
    color: var(--ok);
  }
  .written li.earlier {
    color: var(--text-3);
    font-size: 13px;
  }
  .written li.now {
    color: var(--text);
  }
  .log-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .log {
    margin: 0;
    height: 60vh;
    overflow: auto;
    padding: 12px 14px;
    border-radius: 8px;
    background: var(--code-block-bg);
    border: 1px solid var(--rule);
    font: 400 12px/19px var(--font-code);
    color: var(--text-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  @media (max-width: 839px) {
    .job {
      padding: 0 10px 40px;
    }
    .panel {
      padding: 18px 16px;
    }
    .cols {
      grid-template-columns: minmax(0, 1fr);
    }
    .log {
      height: 50vh;
    }
  }
</style>
