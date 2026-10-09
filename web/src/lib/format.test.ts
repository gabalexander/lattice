// How the app says time, money, models, sources and a job's progress.

import { expect, test } from 'vitest';
import { describeSource, duration, latestVersion, modelName, money, progressFraction, progressText, sourceText, timeAgo } from './format';

test('time ago', () => {
  const now = Date.parse('2026-10-09T12:00:00Z');
  expect(timeAgo('2026-10-09T11:59:30Z', now)).toBe('just now');
  expect(timeAgo('2026-10-09T11:55:00Z', now)).toBe('5 min ago');
  expect(timeAgo('2026-10-09T09:00:00Z', now)).toBe('3 h ago');
  expect(timeAgo('2026-10-08T10:00:00Z', now)).toBe('yesterday');
  expect(timeAgo('2026-10-04T12:00:00Z', now)).toBe('5 days ago');
  expect(timeAgo('not a date', now)).toBe('');
  expect(timeAgo(null, now)).toBe('');
});

test('durations', () => {
  expect(duration(42_000)).toBe('42s');
  expect(duration(185_000)).toBe('3m 05s');
  expect(duration(3_720_000)).toBe('1h 02m');
  expect(duration(-5)).toBe('0s');
});

test('money', () => {
  expect(money(4.2)).toBe('$4.20');
  expect(money(0.034)).toBe('$0.03');
  expect(money(123.4)).toBe('$123');
  expect(money(null)).toBe('');
});

test('models, by alias or id', () => {
  expect(modelName('sonnet')).toBe('Sonnet');
  expect(modelName('claude-opus-5-5')).toBe('Opus');
  expect(modelName('something-else')).toBe('something-else');
});

test('sources', () => {
  expect(sourceText({ kind: 'local', path: '/Users/ana/src/app' })).toBe('~/src/app');
  expect(sourceText({ kind: 'git', url: 'git@git.example.com:t/app.git' })).toBe('git@git.example.com:t/app.git');
  expect(describeSource('~/src/app')).toBe('A folder on this machine');
  expect(describeSource('git@git.example.com:team/app.git')).toBe('A git repository, cloned with your git');
  expect(describeSource('https://git.example.com/team/app')).toBe('A git repository, cloned with your git');
  expect(describeSource('golang/go')).toBe('A GitHub repository');
  expect(describeSource('hello')).toBeNull();
});

test("a job's progress, in words and as a fraction weighted by phase", () => {
  expect(progressText(null)).toBe('Starting');
  expect(progressText({ phase: 'write', done: 12, total: 64, current: 'x', cost_usd: 1 })).toBe('Writing 12 of 64');
  expect(progressText({ phase: 'plan', done: 0, total: 0, current: null, cost_usd: null })).toBe('Planning');
  expect(progressFraction({ phase: 'plan', done: 0, total: 1, current: null, cost_usd: null })).toBe(0);
  expect(progressFraction({ phase: 'write', done: 32, total: 64, current: null, cost_usd: null })).toBeCloseTo(0.465);
  expect(progressFraction({ phase: 'overview', done: 1, total: 1, current: null, cost_usd: null })).toBe(1);
});

test('the latest version, whatever the order', () => {
  expect(latestVersion([{ n: 2 }, { n: 3 }, { n: 1 }])).toEqual({ n: 3 });
  expect(latestVersion([])).toBeNull();
});
