#!/usr/bin/env node
// Measures a wiki's page in headless Chrome: how soon its text is on screen, and how smoothly it scrolls from
// top to bottom while its diagrams are drawn. Run it against the mock serving the built app, whose `atlas` is a
// synthetic wiki of 16 sections, 90 subsections and 107 diagrams:
//
//     npm run build && node mock/server.mjs --app build &
//     node scripts/perf.mjs http://127.0.0.1:7348/atlas [--cpu 4]
//
// It prints the times and the frames as JSON. CHROME names the browser when it isn't Google Chrome on a Mac.
// Carried over from crystal's wiki page (assets/wiki/dev/perf.mjs).

import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const url = args.find((a) => !a.startsWith('--') && !/^\d+$/.test(a));
const cpu = Number(args[args.indexOf('--cpu') + 1]) || 1;
if (!url) {
  console.error('usage: perf.mjs <url> [--cpu N]');
  process.exit(2);
}
const chrome = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const port = 9400 + Math.floor(Math.random() * 400);
const profile = mkdtempSync(join(tmpdir(), 'lattice-perf-'));
const browser = spawn(chrome, ['--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, '--window-size=1568,900', '--no-first-run', 'about:blank'], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let version;
for (let i = 0; i < 100 && !version; i++) {
  try {
    version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  } catch {
    await sleep(100);
  }
}
const ws = new WebSocket(version.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r, { once: true }));
let next = 0;
const pending = new Map();
ws.addEventListener('message', (e) => {
  const msg = JSON.parse(e.data);
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg);
    pending.delete(msg.id);
  }
});
const send = (method, params = {}, sessionId) =>
  new Promise((resolve, reject) => {
    const id = ++next;
    pending.set(id, (msg) => (msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result)));
    ws.send(JSON.stringify({ id, method, params, sessionId }));
  });

try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const page = (method, params) => send(method, params, sessionId);
  const evaluate = async (expression) => {
    const r = await page('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  };
  await page('Page.enable');
  await page('Emulation.setDeviceMetricsOverride', { width: 1568, height: 900, deviceScaleFactor: 1, mobile: false });
  if (cpu > 1) await page('Emulation.setCPUThrottlingRate', { rate: cpu });
  // Long tasks and paints, from the very start.
  await page('Page.addScriptToEvaluateOnNewDocument', {
    source: `window.__perf = { long: [], lcp: 0 };
      new PerformanceObserver((l) => { for (const e of l.getEntries()) __perf.long.push(e.duration); }).observe({ type: 'longtask', buffered: true });
      new PerformanceObserver((l) => { for (const e of l.getEntries()) __perf.lcp = e.startTime; }).observe({ type: 'largest-contentful-paint', buffered: true });`,
  });
  await page('Page.navigate', { url });
  for (let i = 0; i < 200; i++) {
    if (await evaluate('performance.getEntriesByName("lattice-wiki-content").length > 0').catch(() => false)) break;
    await sleep(50);
  }
  await sleep(1500);
  const cards = `document.querySelectorAll('.doc-body .diagram-card')`;
  const load = await evaluate(`(() => {
    const nav = performance.getEntriesByType('navigation')[0];
    const paint = Object.fromEntries(performance.getEntriesByType('paint').map((p) => [p.name, Math.round(p.startTime)]));
    const mark = (n) => { const e = performance.getEntriesByName(n)[0]; return e ? Math.round(e.startTime) : null; };
    return {
      sections: document.querySelectorAll('.doc-body h2.heading').length,
      subsections: document.querySelectorAll('.doc-body h3.heading').length,
      diagrams: ${cards}.length,
      codeLinks: document.querySelectorAll('.doc-body a.code-link').length,
      domNodes: document.getElementsByTagName('*').length,
      domContentLoaded: Math.round(nav.domContentLoadedEventEnd),
      firstContentfulPaint: paint['first-contentful-paint'],
      contentRendered: mark('lattice-wiki-content'),
      largestContentfulPaint: Math.round(__perf.lcp),
      drawnAtRest: document.querySelectorAll('.doc-body .diagram-card.drawn').length,
      longTasksDuringLoad: __perf.long.length,
      longestTaskDuringLoad: Math.round(Math.max(0, ...__perf.long)),
    };
  })()`);
  // Scrolls the whole page at a steady 1,500 px a second, as a reader flicking down it would, timing every
  // frame, while diagrams are drawn as they come near.
  const scroll = await evaluate(`new Promise((resolve) => {
    __perf.long = [];
    const end = document.documentElement.scrollHeight - innerHeight;
    const frames = [];
    let last = performance.now();
    const start = last;
    const step = (now) => {
      frames.push(now - last);
      last = now;
      const y = Math.min(end, (now - start) * 1.5);
      window.scrollTo(0, y);
      if (y < end) requestAnimationFrame(step);
      else {
        frames.sort((a, b) => a - b);
        resolve({
          pageHeight: end + innerHeight,
          seconds: +((now - start) / 1000).toFixed(1),
          frames: frames.length,
          medianFrameMs: +frames[Math.floor(frames.length / 2)].toFixed(1),
          p95FrameMs: +frames[Math.floor(frames.length * 0.95)].toFixed(1),
          worstFrameMs: +frames[frames.length - 1].toFixed(1),
          framesOver50ms: frames.filter((f) => f > 50).length,
          longTasks: __perf.long.length,
          longestTaskMs: Math.round(Math.max(0, ...__perf.long)),
          drawnAfterScroll: document.querySelectorAll('.doc-body .diagram-card.drawn').length,
          failed: document.querySelectorAll('.doc-body .diagram-card.failed').length,
        });
      }
    };
    requestAnimationFrame(step);
  })`);
  console.log(JSON.stringify({ url, cpuSlowdown: cpu, load, scroll }, null, 2));
} finally {
  ws.close();
  browser.kill();
  await sleep(200);
  rmSync(profile, { recursive: true, force: true });
}
