#!/usr/bin/env node
// A stand-in for `lattice serve`, for working on the web app without lattice: its HTTP API (docs/web.md)
// answered from fixtures and from memory, with jobs that run and stream their progress, a chat that streams an
// answer reading files as it goes, and settings that save. Nothing it does reaches outside the machine but
// fetching mermaid once (the same pinned file lattice serves, its SHA-256 checked) into mock/.cache/.
//
//     node mock/server.mjs                  the API on 127.0.0.1:7348, for `npm run dev` to proxy to
//     node mock/server.mjs --app build      the built app too, as lattice serves it: http://127.0.0.1:7348/
//
// It checks Host and Origin as lattice does, so a request the real server would refuse fails here too. Its
// repos: `crystal` (the hand-written fixture, two versions, the code moved on since), `atlas` (a synthetic
// 16-section, 90-subsection wiki, for performance), `tokio` (being built now), `payments` (its build failed)
// and `notes` (never built).

import { createHash } from 'node:crypto';
import { createReadStream, existsSync, mkdirSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { synthesize } from './synth.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : null;
};
const PORT = Number(flag('--port') || process.env.PORT || 7348);
const APP = flag('--app') ? resolve(HERE, '..', flag('--app')) : null;
/** How much slower than real the made-up jobs run: 1 is a write every ~0.6s. */
const PACE = Number(flag('--pace') || 1);

const MERMAID = {
  version: '11.17.2',
  url: 'https://cdn.jsdelivr.net/npm/mermaid@11.17.2/dist/mermaid.min.js',
  sha256: '581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8',
};
const CACHE = join(HERE, '.cache');

// ---- State -----------------------------------------------------------------------------------------------

const crystalWiki = JSON.parse(readFileSync(join(HERE, 'fixtures', 'crystal.wiki.json'), 'utf8'));
const atlasWiki = synthesize({ sections: 16, subsections: 90 });
const hoursAgo = (h) => new Date(Date.now() - h * 3600e3).toISOString();
const sha = (seed) => createHash('sha1').update(String(seed)).digest('hex');

const withVersion = (wiki, { commit, at, model, by, cost }) => ({
  ...wiki,
  repo: { ...wiki.repo, commit },
  generated: { ...wiki.generated, at, model, by, cost_usd: cost },
});

const state = {
  settings: { model: 'sonnet', concurrency: 4, budget_usd: 30, ask_model: 'sonnet', ask_budget_usd: 0.5, exclude: ['vendor/**', '*.min.js'] },
  repos: new Map(),
  jobs: new Map(),
  nextJob: 41,
};

function addRepo(key, name, source, versions) {
  state.repos.set(key, { key, name, source, versions: versions.map((v) => v.meta), wikis: new Map(versions.map((v) => [v.meta.n, v.wiki])), job: null, head: null });
}

{
  const v1 = { n: 1, commit: sha('crystal-1'), branch: 'master', model: 'claude-sonnet-5-5', at: hoursAgo(26 * 24), cost_usd: 3.84 };
  const v2 = { n: 2, commit: crystalWiki.repo.commit, branch: 'master', model: 'claude-opus-5-5', at: hoursAgo(28), cost_usd: 9.12 };
  addRepo('crystal', 'gabalexander/crystal', { kind: 'local', path: '/Users/you/src/crystal' }, [
    { meta: v1, wiki: withVersion(crystalWiki, { commit: v1.commit, at: v1.at, model: v1.model, by: 'Claude Sonnet 5.5', cost: v1.cost_usd }) },
    { meta: v2, wiki: withVersion(crystalWiki, { commit: v2.commit, at: v2.at, model: v2.model, by: 'Claude Opus 5.5', cost: v2.cost_usd }) },
  ]);
  state.repos.get('crystal').head = sha('crystal-head');
  const a1 = { n: 1, commit: atlasWiki.repo.commit, branch: 'main', model: 'claude-sonnet-5-5', at: hoursAgo(3), cost_usd: 12.5 };
  addRepo('atlas', 'example/atlas', { kind: 'git', url: 'https://github.com/example/atlas' }, [{ meta: a1, wiki: atlasWiki }]);
  addRepo('tokio', 'tokio-rs/tokio', { kind: 'git', url: 'https://github.com/tokio-rs/tokio' }, []);
  addRepo('payments', 'payments-service', { kind: 'git', url: 'git@git.internal.example:platform/payments-service.git' }, []);
  addRepo('notes', 'notes', { kind: 'local', path: '/Users/you/src/notes' }, []);
}

// ---- Jobs: made up, but paced and shaped like a real build ------------------------------------------------

const TITLES = [
  'Starting the runtime', 'The scheduler and its workers', 'Tasks, wakers and the run queue', 'Timers and the time wheel', 'I/O drivers and readiness',
  'TCP and UDP sockets', 'Unix sockets and pipes', 'Channels: mpsc, oneshot, broadcast and watch', 'Mutexes, semaphores and notify', 'Spawning blocking work',
  'Signals and process management', 'The filesystem, in a thread pool', 'Tracing and metrics', 'Shutting down', 'Testing with the mocked clock',
];

function jobJson(job) {
  const { events, listeners, timer, ...rest } = job;
  return rest;
}

function emit(job, event, data) {
  job.events.push({ event, data });
  for (const res of job.listeners) send(res, event, data);
}

function send(res, event, data) {
  res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
}

function startJob(repo, kind, model, { failAt = null, total = null, preRun = 0 } = {}) {
  const id = state.nextJob++;
  const job = { id, repo: repo.key, kind, state: 'queued', model: model || state.settings.model, concurrency: state.settings.concurrency, created: new Date().toISOString(), progress: null, started: null, finished: null, error: null, version: null, events: [], listeners: new Set(), timer: null };
  state.jobs.set(id, job);
  repo.job = job;
  const count = total ?? (kind === 'sync' ? 6 : TITLES.length);
  const model2 = model || state.settings.model;
  let cost = 0;
  let step = 0;
  const log = (line) => emit(job, 'log', { line: `${new Date().toISOString().slice(11, 19)} ${line}` });
  const progress = (p) => {
    job.progress = { ...p, cost_usd: +cost.toFixed(2) };
    emit(job, 'progress', job.progress);
  };
  const plan = [];
  plan.push(() => {
    job.state = 'running';
    job.started = new Date().toISOString();
    log(`${kind} of ${repo.name} with ${model2}, ${state.settings.concurrency} at once, budget $${state.settings.budget_usd}`);
    log(repo.source.kind === 'git' ? `git fetch ${repo.source.url}` : `reading ${repo.source.path}`);
    progress({ phase: 'plan', done: 0, total: 1, current: 'The outline' });
  });
  plan.push(() => {
    cost += 0.42;
    log(`claude -p --model ${model2}: planned ${count} subsections in ${Math.ceil(count / 4)} sections`);
    progress({ phase: 'plan', done: 1, total: 1, current: null });
  });
  for (let i = 0; i < count; i++) {
    plan.push(() => {
      const title = TITLES[i % TITLES.length];
      if (failAt === i) {
        log(`error: claude -p exited 1 writing "${title}": the budget of $${state.settings.budget_usd} is spent`);
        finish(job, repo, 'failed', `the budget of $${state.settings.budget_usd} was spent writing "${title}"`);
        return false;
      }
      cost += 0.3 + ((i * 37) % 11) / 20;
      log(`writing "${title}": read 9 files, 3 greps`);
      if (i % 4 === 3) log(`  checked 2 diagrams, 31 code links`);
      progress({ phase: 'write', done: i, total: count, current: title });
    });
  }
  plan.push(() => {
    log(`linking: 412 code names resolved, 3 left as plain code`);
    progress({ phase: 'link', done: 0, total: count, current: null });
  });
  plan.push(() => {
    progress({ phase: 'link', done: count, total: count, current: null });
  });
  plan.push(() => {
    cost += 0.61;
    log('writing the overview');
    progress({ phase: 'overview', done: 0, total: 1, current: 'Overview' });
  });
  plan.push(() => {
    const n = (repo.versions.at(-1)?.n ?? 0) + 1;
    const base = repo.wikis.get(repo.versions.at(-1)?.n) ?? withVersion(crystalWiki, {});
    const commit = repo.head ?? sha(`${repo.key}-${n}`);
    const at = new Date().toISOString();
    const meta = { n, commit, branch: base.repo.branch ?? 'main', model: `claude-${model2}-5-5`, at, cost_usd: +cost.toFixed(2) };
    repo.versions.push(meta);
    repo.wikis.set(n, withVersion({ ...base, repo: { ...base.repo, name: repo.name } }, { commit, at, model: meta.model, by: `Claude ${model2 === 'opus' ? 'Opus' : 'Sonnet'} 5.5`, cost: meta.cost_usd }));
    repo.head = null;
    log(`wrote version ${n} ($${cost.toFixed(2)})`);
    job.version = n;
    finish(job, repo, 'done');
  });
  const tick = () => {
    if (job.state === 'cancelled' || job.state === 'failed' || job.state === 'done') return;
    const fn = plan[step++];
    if (!fn) return;
    if (fn() === false) return;
    job.timer = setTimeout(tick, (step <= 2 ? 900 : 650) * PACE);
  };
  // A job can be made already some way in, for a page to open halfway through one.
  for (let i = 0; i < preRun; i++) plan[step++]();
  job.timer = setTimeout(tick, preRun ? 300 : 600 * PACE);
  return job;
}

function finish(job, repo, how, error = null) {
  clearTimeout(job.timer);
  job.state = how;
  job.finished = new Date().toISOString();
  job.error = error;
  if (how === 'done') emit(job, 'done', { version: job.version });
  else emit(job, 'error', { message: error ?? how });
  for (const res of job.listeners) res.end();
  job.listeners.clear();
}

startJob(state.repos.get('tokio'), 'build', 'opus', { preRun: 9 });
{
  // A build that spent its budget an hour ago.
  const payments = state.repos.get('payments');
  const error = 'the budget of $30 was spent writing "Timers and the time wheel"';
  const progress = { phase: 'write', done: 4, total: 15, current: 'Timers and the time wheel', cost_usd: 30.04 };
  const job = { id: 40, repo: payments.key, kind: 'build', state: 'failed', progress, started: hoursAgo(2), finished: hoursAgo(1.8), error, version: null, listeners: new Set(), timer: null };
  job.events = [
    { event: 'log', data: { line: '07:12:09 build of payments-service with sonnet, 4 at once, budget $30' } },
    { event: 'log', data: { line: `07:31:44 error: claude -p exited 1: ${error}` } },
    { event: 'progress', data: progress },
    { event: 'error', data: { message: error } },
  ];
  state.jobs.set(job.id, job);
  payments.job = job;
}

// ---- The chat ----------------------------------------------------------------------------------------------

function answer(repo, question, section) {
  const wiki = repo.wikis.get(repo.versions.at(-1)?.n);
  const files = [];
  for (const sec of wiki?.sections ?? []) for (const sub of sec.subsections) if (!section || section === sec.id || section === sub.id) files.push(...(sub.files ?? []));
  const [a = 'src/main.rs', b = 'src/lib.rs', c = 'README.md'] = [...new Set(files)];
  const text =
    `In short: the daemon owns every session, and everything else asks it over its socket.\n\n` +
    `It starts in [\`daemon::run\`](code:${a}#L130-L275), which binds the socket and then loops, taking one request a line. ` +
    `Each request is read by [\`Request::parse\`](code:${b}#L42) and answered in place, so a slow client never holds up the others.\n\n` +
    `1. A client connects and sends one JSON line.\n2. The daemon looks the session up and does what was asked.\n3. It answers with one line, or keeps the connection for a stream ([\`attach\`](code:${b}#L310)).\n\n` +
    `| Request | Answered by |\n|---|---|\n| \`send\` | [\`Session::send\`](code:${a}#L88) |\n| \`wait\` | [\`Waiters::add\`](code:${a}#L512) |\n\n` +
    '```mermaid\nflowchart LR\n  client["Client<br/>(crystal send)"] -->|one JSON line| socket["Socket"]\n  socket --> daemon["Daemon loop"]\n  daemon -->|writes| pty["Session PTY"]\n```\n\n' +
    `You asked: *${question.trim().slice(0, 160)}*. See also [${wiki?.sections?.[0]?.title ?? 'the overview'}](#${wiki?.sections?.[0]?.id ?? ''}). Sources: ${a}:130-275, ${c}:1`;
  return { tools: [{ name: 'Read', path: a }, { name: 'Grep', pattern: 'fn run' }, { name: 'Read', path: b }], text };
}

// ---- HTTP --------------------------------------------------------------------------------------------------

const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.txt': 'text/plain; charset=utf-8', '.png': 'image/png' };

function json(res, status, body) {
  res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  res.end(JSON.stringify(body));
}
const fail = (res, status, message) => json(res, status, { message });

function readBody(req, limit = 64 * 1024) {
  return new Promise((done, reject) => {
    let size = 0;
    const chunks = [];
    req.on('data', (c) => {
      size += c.length;
      if (size > limit) reject(new Error('too large'));
      else chunks.push(c);
    });
    req.on('end', () => {
      try {
        done(chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {});
      } catch {
        reject(new Error('not JSON'));
      }
    });
  });
}

function repoJson(repo) {
  return { key: repo.key, name: repo.name, source: repo.source, versions: repo.versions, job: repo.job ? jobJson(repo.job) : null };
}

async function mermaid() {
  const file = join(CACHE, `mermaid-${MERMAID.version}.min.js`);
  if (existsSync(file)) return file;
  mkdirSync(CACHE, { recursive: true });
  const res = await fetch(MERMAID.url);
  if (!res.ok) throw new Error(`${MERMAID.url} answered ${res.status}`);
  const bytes = Buffer.from(await res.arrayBuffer());
  const got = createHash('sha256').update(bytes).digest('hex');
  if (got !== MERMAID.sha256) throw new Error(`mermaid's SHA-256 is ${got}, not ${MERMAID.sha256}`);
  writeFileSync(`${file}.part`, bytes);
  renameSync(`${file}.part`, file);
  return file;
}

function serveFile(res, file, cache) {
  res.writeHead(200, { 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream', 'Cache-Control': cache });
  createReadStream(file).pipe(res);
}

async function api(req, res, url) {
  const parts = url.pathname.split('/').slice(2).map(decodeURIComponent);
  const method = req.method;
  const [what, key, sub] = parts;
  if (what === 'repos' && !key) {
    if (method === 'GET') return json(res, 200, [...state.repos.values()].map(repoJson));
    if (method === 'POST') {
      const { source } = await readBody(req);
      if (typeof source !== 'string' || !source.trim()) return fail(res, 400, 'source: give a folder, a git URL or owner/repo');
      const s = source.trim();
      if (/^(\/|~)/.test(s) && /nope|missing/.test(s)) return fail(res, 400, `${s} isn't a git repository`);
      const name = s.replace(/\.git$/, '').replace(/^.*[/:]([^/:]+\/[^/:]+)$/, '$1');
      const repoKey = name.split('/').pop().toLowerCase().replace(/[^a-z0-9-]+/g, '-');
      if (!state.repos.has(repoKey)) {
        const source = /^(\/|~)/.test(s) ? { kind: 'local', path: s } : { kind: 'git', url: /^[\w.-]+\/[\w.-]+$/.test(s) ? `https://github.com/${s}` : s };
        addRepo(repoKey, /^(\/|~)/.test(s) ? name.split('/').pop() : name, source, []);
      }
      return json(res, 200, { key: repoKey });
    }
  }
  const repo = what === 'repos' ? state.repos.get(key) : null;
  if (what === 'repos' && !repo) return fail(res, 404, `there's no repo ${key}`);
  if (repo && !sub && method === 'DELETE') {
    if (repo.job && ['queued', 'running'].includes(repo.job.state)) finish(repo.job, repo, 'cancelled');
    state.repos.delete(repo.key);
    res.writeHead(204).end();
    return;
  }
  if (repo && sub === 'jobs' && method === 'POST') {
    const body = await readBody(req);
    if (!['build', 'sync', 'resume', 'regenerate'].includes(body.kind)) return fail(res, 400, 'kind: build, sync, resume or regenerate');
    if (repo.job && ['queued', 'running'].includes(repo.job.state)) return fail(res, 409, `a ${repo.job.kind} of ${repo.name} is running already`);
    if (body.kind === 'sync' && !repo.versions.length) return fail(res, 409, `${repo.name} has no version to sync yet`);
    return json(res, 200, jobJson(startJob(repo, body.kind, body.model)));
  }
  if (repo && sub === 'wiki' && method === 'GET') {
    const n = url.searchParams.get('version') ? Number(url.searchParams.get('version')) : repo.versions.at(-1)?.n;
    const wiki = repo.wikis.get(n);
    return wiki ? json(res, 200, wiki) : fail(res, 404, n ? `there's no version ${n}` : `${repo.name} has no wiki yet`);
  }
  if (repo && sub === 'status' && method === 'GET') {
    const latest = repo.versions.at(-1);
    return json(res, 200, { stale: !!(repo.head && latest && repo.head !== latest.commit), head: repo.head, commit: latest?.commit ?? null, job: repo.job ? jobJson(repo.job) : null });
  }
  if (repo && sub === 'open' && method === 'GET') {
    const path = url.searchParams.get('path') ?? '';
    if (!path || path.includes('..') || path.startsWith('/')) return fail(res, 404, `${path} isn't in the repository`);
    console.log(`open ${repo.key}: ${path}:${url.searchParams.get('line') ?? 1}`);
    res.writeHead(204).end();
    return;
  }
  if (repo && sub === 'ask' && method === 'POST') {
    const { question, section } = await readBody(req);
    if (typeof question !== 'string' || !question.trim()) return fail(res, 400, 'the question is empty');
    res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store' });
    if (/error/i.test(question)) {
      send(res, 'tool', { name: 'Read', path: 'src/main.rs' });
      setTimeout(() => (send(res, 'error', { message: 'claude -p exited 1: the question cost more than its budget of $0.50' }), res.end()), 600);
      return;
    }
    const { tools, text } = answer(repo, question, section);
    const pieces = text.match(/[\s\S]{1,14}/g);
    let i = 0;
    const steps = [...tools.map((t) => () => send(res, 'tool', t)), ...pieces.map((p) => () => send(res, 'delta', { text: p }))];
    // The files first, a beat each, then the answer a few words at a time ("slow" in the question slows it).
    let timer;
    const next = () => {
      if (i < steps.length) {
        steps[i++]();
        timer = setTimeout(next, /slow/i.test(question) ? 160 : i <= tools.length ? 450 : 24);
      } else {
        send(res, 'done', { conversation: '6f1c2b9e-1d1e-4c5a-9a55-0e7f7a1d2c3b', cost_usd: 0.04 });
        res.end();
      }
    };
    timer = setTimeout(next, 300);
    req.on('close', () => clearTimeout(timer));
    return;
  }
  if (what === 'jobs' && key) {
    const job = state.jobs.get(Number(key));
    if (!job) return fail(res, 404, `there's no job ${key}`);
    if (!sub && method === 'GET') return json(res, 200, jobJson(job));
    if (sub === 'cancel' && method === 'POST') {
      if (['queued', 'running'].includes(job.state)) finish(job, state.repos.get(job.repo) ?? {}, 'cancelled', 'cancelled');
      return json(res, 200, jobJson(job));
    }
    if (sub === 'events' && method === 'GET') {
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store' });
      // What happened so far: the latest progress and every line of the log, then what's next.
      const logs = job.events.filter((e) => e.event === 'log');
      for (const e of logs) send(res, e.event, e.data);
      if (job.progress) send(res, 'progress', job.progress);
      const end = job.events.find((e) => e.event === 'done' || e.event === 'error');
      if (end) {
        send(res, end.event, end.data);
        res.end();
        return;
      }
      job.listeners.add(res);
      req.on('close', () => job.listeners.delete(res));
      return;
    }
  }
  if (what === 'settings') {
    if (method === 'GET') return json(res, 200, state.settings);
    if (method === 'PUT') {
      const s = await readBody(req);
      const models = ['sonnet', 'opus'];
      if (!models.includes(s.model)) return fail(res, 400, 'model: sonnet or opus');
      if (!models.includes(s.ask_model)) return fail(res, 400, 'ask_model: sonnet or opus');
      if (!Number.isInteger(s.concurrency) || s.concurrency < 1 || s.concurrency > 16) return fail(res, 400, 'concurrency: a whole number from 1 to 16');
      if (typeof s.budget_usd !== 'number' || s.budget_usd <= 0) return fail(res, 400, 'budget_usd: more than 0');
      if (typeof s.ask_budget_usd !== 'number' || s.ask_budget_usd <= 0) return fail(res, 400, 'ask_budget_usd: more than 0');
      if (!Array.isArray(s.exclude) || s.exclude.some((g) => typeof g !== 'string')) return fail(res, 400, 'exclude: a list of globs');
      state.settings = { model: s.model, concurrency: s.concurrency, budget_usd: s.budget_usd, ask_model: s.ask_model, ask_budget_usd: s.ask_budget_usd, exclude: s.exclude };
      return json(res, 200, state.settings);
    }
  }
  return fail(res, 404, `no ${method} ${url.pathname}`);
}

const server = createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', `http://${req.headers.host}`);
  // As lattice does: only this machine's names, and no other site's page writing.
  const host = (req.headers.host ?? '').replace(/:\d+$/, '');
  if (host !== '127.0.0.1' && host !== 'localhost') return fail(res, 403, 'lattice answers to 127.0.0.1 and localhost only');
  const origin = req.headers.origin;
  if (['POST', 'PUT', 'DELETE'].includes(req.method ?? '')) {
    if (!origin) return fail(res, 403, "a page that doesn't say its origin");
    if (origin !== `http://127.0.0.1:${PORT}` && origin !== `http://localhost:${PORT}`) return fail(res, 403, "another site's page");
  }
  try {
    if (url.pathname.startsWith('/api/')) return await api(req, res, url);
    if (url.pathname === '/assets/mermaid.min.js') return serveFile(res, await mermaid(), 'public, max-age=86400');
    if (!APP) return fail(res, 404, 'the app is served by `npm run dev` (or start this with --app build)');
    const file = normalize(join(APP, decodeURIComponent(url.pathname)));
    if (!file.startsWith(APP + sep) && file !== APP) return fail(res, 400, 'no');
    if (existsSync(file) && statSync(file).isFile()) return serveFile(res, file, url.pathname.startsWith('/_app/immutable/') ? 'public, max-age=31536000, immutable' : 'no-cache');
    return serveFile(res, join(APP, 'index.html'), 'no-cache');
  } catch (err) {
    console.error(err);
    if (!res.headersSent) fail(res, 500, String(err.message ?? err));
    else res.end();
  }
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`lattice's mock API on http://127.0.0.1:${PORT}/${APP ? ` (and the app from ${APP})` : ''}`);
});
