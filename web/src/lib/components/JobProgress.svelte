<!-- How far a job has got: its phases (plan, write, link, overview) with the one it's in lit, a bar, and a line
     saying what it's on and what it has cost. Compact, as a card or a wiki's banner shows it, it's the bar and
     the line. After deepwiki-by-cc's JobProgress (MIT; see THIRD_PARTY_NOTICES.md). -->
<script lang="ts">
  import { money, PHASE_NAMES, PHASES, progressFraction, progressText } from '$lib/format';
  import type { JobState, Progress } from '$lib/types';

  let { progress, state, compact = false }: { progress: Progress | null; state: JobState; compact?: boolean } = $props();

  const fraction = $derived(state === 'done' ? 1 : progressFraction(progress));
  const phaseAt = $derived(progress ? PHASES.indexOf(progress.phase) : -1);
  const waiting = $derived(state === 'queued' || (state === 'running' && !progress));
  const line = $derived(
    state === 'queued'
      ? 'Waiting to start'
      : state === 'done'
        ? 'Done'
        : state === 'cancelled'
          ? `Cancelled${progress ? ` while ${progressText(progress).toLowerCase()}` : ''}`
          : state === 'failed'
            ? `Stopped${progress ? ` while ${progressText(progress).toLowerCase()}` : ''}`
            : !progress
              ? 'Starting'
              : progressText(progress),
  );
</script>

<div class="progress" class:compact>
  {#if !compact}
    <ol class="phases" aria-label="Phases">
      {#each PHASES as phase, i (phase)}
        {@const now = i === phaseAt && state === 'running'}
        {@const past = i < phaseAt || state === 'done'}
        <li class:now class:past aria-current={now ? 'step' : undefined}>
          <span class="dot" aria-hidden="true"></span>{PHASE_NAMES[phase]}
        </li>
      {/each}
    </ol>
  {/if}
  <div
    class="bar"
    class:waiting
    class:stopped={state === 'failed' || state === 'cancelled'}
    role="progressbar"
    aria-label="Progress"
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={waiting ? undefined : Math.round(fraction * 100)}
  >
    <div class="fill" style:width="{waiting ? 100 : Math.max(2, fraction * 100)}%"></div>
  </div>
  <div class="line">
    <span class="what">
      {line}{#if progress?.current && state === 'running'}<span class="current">{' · '}{progress.current}</span>{/if}
    </span>
    {#if progress?.cost_usd}<span class="cost">{money(progress.cost_usd)}</span>{/if}
  </div>
</div>

<style>
  .progress {
    display: grid;
    gap: 10px;
    min-width: 0;
  }
  .progress.compact {
    gap: 6px;
  }
  .phases {
    list-style: none;
    margin: 0 0 6px;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
    font: 500 13px/20px var(--font-display);
    color: var(--text-3);
  }
  .phases li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 10px;
    border-top: 2px solid var(--track);
    min-width: 0;
  }
  .phases li.past {
    color: var(--text-2);
    border-top-color: color-mix(in srgb, var(--accent) 55%, transparent);
  }
  .phases li.now {
    color: var(--accent);
    border-top-color: var(--accent);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.5;
    flex: none;
  }
  .now .dot {
    opacity: 1;
    animation: pulse 1.6s ease-in-out infinite;
  }
  .past .dot {
    opacity: 1;
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--track);
    overflow: hidden;
  }
  .compact .bar {
    height: 4px;
  }
  .fill {
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 0.4s ease;
  }
  .waiting .fill {
    background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--accent) 55%, transparent), transparent);
    background-size: 40% 100%;
    background-repeat: no-repeat;
    animation: sweep 1.6s ease-in-out infinite;
  }
  .stopped .fill {
    background: var(--text-3);
  }
  @keyframes sweep {
    from { background-position: -40% 0; }
    to { background-position: 140% 0; }
  }
  .line {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
    line-height: 20px;
    color: var(--text-2);
    min-width: 0;
  }
  .what {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .current {
    color: var(--text-3);
  }
  .cost {
    flex: none;
    font-variant-numeric: tabular-nums;
    color: var(--text-3);
  }
</style>
