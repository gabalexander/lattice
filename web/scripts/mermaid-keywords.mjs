#!/usr/bin/env node
// Holds the page's repairs of a diagram (src/lib/mermaid-sanitize.ts, names that are mermaid's keywords among
// them) to mermaid itself: mermaid's own parse, in headless Chrome, over every keyword of every kind and the cases
// in tests/mermaid/keywords.json, and over every diagram of the wikis given, each as it was written and as the
// page repairs it before drawing it. It prints how many parse each way and the diagrams that still don't once
// repaired, and exits 1 when a keyword or a case doesn't.
//
//     node scripts/mermaid-keywords.mjs ~/.cache/lattice/downloads/11.17.2/mermaid.min.js [wiki.json ...]
//
// The mermaid is the one lattice downloads into its cache when it first serves a wiki. CHROME names the browser
// when it isn't Google Chrome on a Mac.

import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const [mermaidPath, ...wikis] = process.argv.slice(2);
if (!mermaidPath) {
  console.error('usage: mermaid-keywords.mjs <mermaid.min.js> [wiki.json ...]');
  process.exit(2);
}
const web = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixture = JSON.parse(readFileSync(join(web, '../tests/mermaid/keywords.json'), 'utf8'));

// The page's own repairs, read as the app is built.
const vite = await createServer({ root: web, configFile: false, logLevel: 'silent', appType: 'custom', server: { middlewareMode: true, hmr: false } });
const { sanitizeMermaid } = await vite.ssrLoadModule('/src/lib/mermaid-sanitize.ts');
const { findMermaidFences } = await vite.ssrLoadModule('/src/lib/fences.ts');
await vite.close();

const groups = [
  {
    name: 'keywords',
    held: true,
    diagrams: Object.entries(fixture.words).flatMap(([kind, words]) => words.map((w) => fixture.templates[kind].before.replaceAll('{w}', w))),
  },
  { name: 'cases', held: true, diagrams: fixture.cases.map((c) => c.before) },
  ...wikis.map((path) => ({ name: basename(path), held: false, diagrams: diagramsOf(JSON.parse(readFileSync(path, 'utf8'))) })),
];

// Every diagram of a wiki: its cards' and the ```mermaid fences in its texts.
function diagramsOf(wiki) {
  const found = [];
  const walk = (value) => {
    if (Array.isArray(value)) value.forEach(walk);
    else if (value && typeof value === 'object') {
      for (const [key, field] of Object.entries(value)) {
        if (key === 'mermaid' && typeof field === 'string') found.push(field);
        else if (typeof field === 'string') found.push(...findMermaidFences(field).map((fence) => fence.code));
        else walk(field);
      }
    }
  };
  walk(wiki);
  return found;
}

const parse = await mermaidParser(mermaidPath);
let failed = false;
try {
  const rows = [];
  const broken = [];
  for (const group of groups) {
    const repaired = group.diagrams.map(sanitizeMermaid);
    const before = await parse(group.diagrams);
    const after = await parse(repaired);
    rows.push([group.name, group.diagrams.length, before.filter((r) => r.ok).length, after.filter((r) => r.ok).length]);
    after.forEach((result, k) => {
      if (result.ok) return;
      broken.push({ group: group.name, source: repaired[k], error: result.error });
      if (group.held) failed = true;
    });
  }
  const widths = [Math.max(8, ...rows.map((r) => r[0].length)), 8, 16, 14];
  const line = (cells) => cells.map((cell, k) => (k ? String(cell).padStart(widths[k]) : String(cell).padEnd(widths[k]))).join('  ');
  console.log(line(['', 'diagrams', 'parse as written', 'parse repaired']));
  for (const row of rows) console.log(line(row));
  for (const { group, source, error } of broken) console.log(`\n${group}, still not parsing once repaired: ${error}\n${source}`);
} finally {
  await parse.close();
}
process.exit(failed ? 1 : 0);

// mermaid.parse in headless Chrome: a function from diagrams to whether each parses, with mermaid's error if not.
async function mermaidParser(path) {
  const chrome = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  const port = 9400 + Math.floor(Math.random() * 400);
  const profile = mkdtempSync(join(tmpdir(), 'lattice-mermaid-'));
  const browser = spawn(chrome, ['--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, '--no-first-run', 'about:blank'], { stdio: 'ignore' });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  let version;
  for (let i = 0; i < 100 && !version; i++) {
    try {
      version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
    } catch {
      await sleep(100);
    }
  }
  if (!version) throw new Error(`${chrome} didn't start`);
  const ws = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener('open', r, { once: true }));
  let next = 0;
  const pending = new Map();
  ws.addEventListener('message', (e) => {
    const msg = JSON.parse(e.data);
    pending.get(msg.id)?.(msg);
    pending.delete(msg.id);
  });
  const send = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = ++next;
      pending.set(id, (msg) => (msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result)));
      ws.send(JSON.stringify({ id, method, params, sessionId }));
    });
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }, sessionId);
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  };
  await evaluate(`${readFileSync(path, 'utf8')}\n;mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' })`);
  const run = (diagrams) =>
    evaluate(`(async () => {
      const out = [];
      for (const source of ${JSON.stringify(diagrams)}) {
        try {
          await mermaid.parse(source);
          out.push({ ok: true });
        } catch (err) {
          out.push({ ok: false, error: String(err?.message ?? err).split('\\n')[0] });
        }
      }
      return out;
    })()`);
  run.close = async () => {
    ws.close();
    browser.kill();
    await sleep(200);
    rmSync(profile, { recursive: true, force: true });
  };
  return run;
}
