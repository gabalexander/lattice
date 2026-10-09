// A job followed as it runs: its state, its progress, the subsections written so far and its log, from
// `GET /api/jobs/<id>/events`. lattice sends what happened so far first on every connection, so a page opened
// halfway through, or one whose connection dropped and came back, sees the whole of it.

import { api, ApiError, jobEventsUrl } from './api';
import type { Job, Progress } from './types';

/** The most log lines kept: a long build writes thousands, and the page shows the latest. */
const LOG_LINES = 2000;

export class JobStream {
  job = $state<Job | null>(null);
  progress = $state<Progress | null>(null);
  log = $state<string[]>([]);
  /** The titles of the subsections written, in order. */
  written = $state<string[]>([]);
  /** The version it made, once it's done. */
  version = $state<number | null>(null);
  error = $state<string | null>(null);
  /** Whether the stream is down and the browser is trying it again. */
  reconnecting = $state(false);
  finished = $derived(this.job ? ['done', 'failed', 'cancelled'].includes(this.job.state) : false);

  private source: EventSource | null = null;
  private onDone: ((version: number) => void) | undefined;
  /** Whether it was still going when the page first read it: only then does its end move the page on. */
  private followedLive = false;

  constructor(
    readonly id: number,
    onDone?: (version: number) => void,
  ) {
    this.onDone = onDone;
  }

  /** Reads the job, and follows it if it's still going. */
  async start() {
    try {
      const job = await api.job(this.id);
      this.job = job;
      this.progress = job.progress;
      if (job.state === 'failed') this.error = job.error || 'The job failed.';
      if (job.state === 'done' && job.version) this.version = job.version;
    } catch (err) {
      this.error = err instanceof ApiError && err.status === 404 ? 'There is no such job.' : (err as Error).message;
      return;
    }
    this.followedLive = !this.finished;
    // A finished job's stream still has its log, then its end.
    this.follow();
  }

  private follow() {
    const source = new EventSource(jobEventsUrl(this.id));
    this.source = source;
    source.addEventListener('open', () => {
      // A connection starts with what happened so far, the log among it.
      this.log = [];
      this.written = [];
      this.reconnecting = false;
    });
    source.addEventListener('progress', (e) => this.onProgress(parse(e) as Progress));
    source.addEventListener('log', (e) => {
      const line = (parse(e) as { line?: string }).line ?? '';
      this.log = this.log.length >= LOG_LINES ? [...this.log.slice(-LOG_LINES + 1), line] : [...this.log, line];
    });
    source.addEventListener('done', (e) => {
      const version = (parse(e) as { version?: number }).version ?? null;
      this.version = version;
      this.settle('done');
      if (version && this.followedLive && this.onDone) this.onDone(version);
    });
    // lattice's `error` event, which has data; the browser's own, on a dropped connection, has none.
    source.addEventListener('error', (e) => {
      const data = (e as MessageEvent).data;
      if (typeof data === 'string') {
        const message = (parse(e as MessageEvent) as { message?: string }).message || 'The job failed.';
        this.error = message;
        this.settle(/cancel/i.test(message) ? 'cancelled' : 'failed');
      } else if (source.readyState === EventSource.CLOSED) {
        void this.recheck();
      } else {
        this.reconnecting = true;
      }
    });
  }

  private onProgress(progress: Progress) {
    const before = this.progress;
    if (before && before.phase === 'write' && before.current && before.current !== progress.current && !this.written.includes(before.current)) {
      this.written = [...this.written, before.current];
    }
    this.progress = progress;
    if (this.job && this.job.state === 'queued') this.job = { ...this.job, state: 'running' };
  }

  private settle(state: Job['state']) {
    this.source?.close();
    this.source = null;
    this.reconnecting = false;
    if (this.job) this.job = { ...this.job, state, finished: this.job.finished ?? new Date().toISOString() };
  }

  // The browser gave up on the stream: what lattice says of the job now.
  private async recheck() {
    this.source = null;
    try {
      const job = await api.job(this.id);
      this.job = job;
      if (job.state === 'failed') this.error = job.error || 'The job failed.';
      else if (!this.finished) {
        this.reconnecting = true;
        setTimeout(() => this.follow(), 2000);
      }
    } catch {
      this.reconnecting = true;
      setTimeout(() => this.follow(), 3000);
    }
  }

  async cancel() {
    const job = await api.cancelJob(this.id);
    this.job = job && typeof job === 'object' ? job : this.job && { ...this.job, state: 'cancelled' };
  }

  close() {
    this.source?.close();
    this.source = null;
  }
}

function parse(event: MessageEvent): unknown {
  try {
    return JSON.parse(event.data);
  } catch {
    return {};
  }
}
