// How the app says dates, money, models and sources.

import type { JobKind, Phase, Progress, Source } from './types';

export function formatDate(at: string | null | undefined): string {
  if (!at) return '';
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

/** How long ago `at` was, said briefly: "just now", "5 min ago", "3 h ago", "2 days ago", else the date. */
export function timeAgo(at: string | null | undefined, now: number = Date.now()): string {
  if (!at) return '';
  const then = Date.parse(at);
  if (Number.isNaN(then)) return '';
  const s = Math.max(0, Math.round((now - then) / 1000));
  if (s < 45) return 'just now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d < 14) return d === 1 ? 'yesterday' : `${d} days ago`;
  return formatDate(at);
}

/** A span of time as a clock would say it: 42s, 3m 05s, 1h 02m. */
export function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, '0')}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
}

export function money(usd: number | null | undefined): string {
  if (typeof usd !== 'number' || !Number.isFinite(usd)) return '';
  return usd < 0.995 && usd > 0 ? `$${usd.toFixed(2)}` : `$${usd.toFixed(usd >= 100 ? 0 : 2)}`;
}

/** A model's family name, from an alias or a full id: `claude-opus-5-5` is "Opus". */
export function modelName(model: string | null | undefined): string {
  const m = String(model || '').toLowerCase();
  if (m.includes('opus')) return 'Opus';
  if (m.includes('sonnet')) return 'Sonnet';
  if (m.includes('haiku')) return 'Haiku';
  if (m.includes('fable')) return 'Fable';
  return model || '';
}

export function sourceText(source: Source): string {
  return source.kind === 'local' ? source.path.replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, '~') : source.url;
}

export const KIND_NAMES: Record<JobKind, string> = { build: 'Build', sync: 'Sync', resume: 'Resume', regenerate: 'Regenerate' };

export const PHASES: Phase[] = ['plan', 'write', 'link', 'overview'];
export const PHASE_NAMES: Record<Phase, string> = { plan: 'Planning', write: 'Writing', link: 'Linking', overview: 'Overview' };

/** A job's progress in a line: "Writing 12 of 64". */
export function progressText(progress: Progress | null): string {
  if (!progress) return 'Starting';
  const name = PHASE_NAMES[progress.phase] || progress.phase;
  return progress.total > 0 ? `${name} ${progress.done} of ${progress.total}` : name;
}

/** How far along a job is, 0 to 1: the phases weighed by how long they take, writing the most. */
export function progressFraction(progress: Progress | null): number {
  if (!progress) return 0;
  const weights: Record<Phase, [number, number]> = { plan: [0, 0.08], write: [0.08, 0.85], link: [0.85, 0.95], overview: [0.95, 1] };
  const [from, to] = weights[progress.phase] || [0, 1];
  const within = progress.total > 0 ? Math.min(1, Math.max(0, progress.done / progress.total)) : 0;
  return from + (to - from) * within;
}

/** What someone types for a source, said back as what it is: a path, a GitHub repo or a git URL. */
export function describeSource(text: string): string | null {
  const s = text.trim();
  if (!s) return null;
  if (/^(\/|~\/|\.{1,2}\/|[A-Za-z]:\\)/.test(s)) return 'A folder on this machine';
  if (/^(https?|ssh|git):\/\//.test(s) || /^[\w.-]+@[\w.-]+:/.test(s)) return 'A git repository, cloned with your git';
  if (/^[\w.-]+\/[\w.-]+$/.test(s)) return 'A GitHub repository';
  return null;
}

/** The newest of a repo's versions, whatever order they come in. */
export function latestVersion<T extends { n: number }>(versions: T[]): T | null {
  return versions.reduce<T | null>((best, v) => (!best || v.n > best.n ? v : best), null);
}
