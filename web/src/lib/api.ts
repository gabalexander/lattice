// The app's side of lattice's HTTP API (docs/web.md lists it): a function for each call, typed, throwing an
// `ApiError` with lattice's own message when it says no.

import { readEvents } from './sse';
import type { AskEvent, Job, JobKind, Model, Repo, RepoStatus, Settings, Wiki } from './types';

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

/** What lattice said when it said no: its JSON `message`, its text, or the status. */
async function errorOf(res: Response): Promise<ApiError> {
  const text = (await res.text().catch(() => '')).trim();
  let message = text;
  try {
    const json = JSON.parse(text);
    if (json && typeof json.message === 'string') message = json.message;
  } catch {
    // Plain text.
  }
  return new ApiError(message || `lattice answered ${res.status} ${res.statusText}`.trim(), res.status);
}

async function call<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers: body === undefined ? { Accept: 'application/json' } : { Accept: 'application/json', 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
      cache: 'no-store',
      signal,
    });
  } catch (err) {
    if ((err as Error).name === 'AbortError') throw err;
    throw new ApiError("Couldn't reach lattice: is `lattice serve` still running?", 0);
  }
  if (!res.ok) throw await errorOf(res);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

const repoPath = (key: string) => `/api/repos/${encodeURIComponent(key)}`;

export const api = {
  repos: (signal?: AbortSignal) => call<Repo[]>('GET', '/api/repos', undefined, signal),
  addRepo: (source: string) => call<{ key: string }>('POST', '/api/repos', { source }),
  deleteRepo: (key: string) => call<void>('DELETE', repoPath(key)),
  startJob: (key: string, kind: JobKind, model: Model | null = null, concurrency: number | null = null) =>
    call<Job>('POST', `${repoPath(key)}/jobs`, { kind, model, concurrency }),
  job: (id: number | string, signal?: AbortSignal) => call<Job>('GET', `/api/jobs/${encodeURIComponent(String(id))}`, undefined, signal),
  cancelJob: (id: number | string) => call<Job>('POST', `/api/jobs/${encodeURIComponent(String(id))}/cancel`),
  wiki: (key: string, version: number | null, signal?: AbortSignal) =>
    call<Wiki>('GET', `${repoPath(key)}/wiki${version ? `?version=${version}` : ''}`, undefined, signal),
  status: (key: string, signal?: AbortSignal) => call<RepoStatus>('GET', `${repoPath(key)}/status`, undefined, signal),
  open: (key: string, query: string) => call<void>('GET', `${repoPath(key)}/open?${query}`),
  settings: () => call<Settings>('GET', '/api/settings'),
  saveSettings: (settings: Settings) => call<Settings>('PUT', '/api/settings', settings),
};

/** Where a job's events stream from, for an EventSource. */
export const jobEventsUrl = (id: number | string) => `/api/jobs/${encodeURIComponent(String(id))}/events`;

export interface Question {
  question: string;
  conversation: string | null;
  section: string | null;
}

/** Asks about a repo's code, handing each event of the answer to `on` as it streams in. */
export async function ask(key: string, question: Question, on: (event: AskEvent) => void, signal: AbortSignal): Promise<void> {
  let res: Response;
  try {
    res = await fetch(`${repoPath(key)}/ask`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' },
      body: JSON.stringify(question),
      signal,
    });
  } catch (err) {
    if ((err as Error).name === 'AbortError') throw err;
    throw new ApiError("Couldn't reach lattice: is `lattice serve` still running?", 0);
  }
  if (!res.ok) throw await errorOf(res);
  if (!res.body) throw new ApiError('the answer came without a stream', res.status);
  await readEvents(res.body, (event) => on(event as AskEvent));
}
