// Drawing the wiki's diagrams with mermaid, which lattice serves at /assets/mermaid.min.js (pinned at 11.17.2
// and downloaded once into its cache) and the app loads only when the first diagram comes near. A wiki may
// have a hundred diagrams, so each is drawn as it comes within a screen or so of the window, the nearest
// first, one at a time with the page let breathe between them.
//
// Carried over from crystal's wiki page (assets/wiki/app.js).

import { sanitizeMermaid } from './mermaid-sanitize';

/** What mermaid makes of a diagram: its SVG, the id mermaid gave it, and its natural size. */
export interface Drawing {
  svg: string;
  id: string;
  width: number;
  height: number;
}

type Result = { ok: true; drawing: Drawing } | { ok: false; error: string };

interface Card {
  el: Element;
  src: string;
  near: boolean;
  state: 'idle' | 'queued' | 'drawing' | 'done';
  done: (result: Result) => void;
}

interface MermaidApi {
  initialize(config: Record<string, unknown>): void;
  render(id: string, text: string): Promise<{ svg: string }>;
}

declare global {
  interface Window {
    mermaid?: MermaidApi;
  }
}

let url = '/assets/mermaid.min.js';
let loading: Promise<MermaidApi> | null = null;
let themeDrawn: string | null = null;
const cards = new Map<Element, Card>();
const queue: Card[] = [];
let running = false;
let observer: IntersectionObserver | null = null;
let drawn = 0;

/** Where mermaid is, when it isn't lattice's /assets/mermaid.min.js: an export has it beside its page. */
export function setMermaidUrl(at: string) {
  url = at;
}

function load(): Promise<MermaidApi> {
  if (loading) return loading;
  loading = (async () => {
    if (!window.mermaid) {
      await new Promise<void>((resolve, reject) => {
        const script = document.createElement('script');
        script.src = url;
        script.async = true;
        script.onload = () => resolve();
        script.onerror = () => reject(new Error(`couldn't load ${url}`));
        document.head.appendChild(script);
      });
    }
    if (!window.mermaid) throw new Error('mermaid.min.js loaded but defined no mermaid');
    try {
      await document.fonts.load('12px "Google Sans Code"');
    } catch {
      // Drawn in the fallback font.
    }
    initialize(window.mermaid);
    return window.mermaid;
  })();
  loading.catch(() => {
    loading = null;
  });
  return loading;
}

const currentTheme = () => document.documentElement.dataset.resolvedTheme || 'dark';

// Mermaid's own colours for the theme in force; the page's stylesheet colours what it draws from the theme's
// variables over them (wiki.css), so the diagrams follow a change of theme without being drawn again.
function initialize(mermaid: MermaidApi) {
  const styles = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
  const dark = currentTheme() === 'dark';
  const c = { card: v('--card', '#303030'), line: v('--dg-line', '#e3e3e3'), text: v('--dg-text', '#fff'), faint: v('--dg-faint', '#8e918f'), panel: v('--panel', '#131314') };
  const font = '"Google Sans Code", ui-monospace, SFMono-Regular, Menlo, monospace';
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    theme: 'base',
    darkMode: dark,
    fontFamily: font,
    fontSize: 12,
    themeVariables: {
      darkMode: dark,
      fontFamily: font,
      fontSize: '12px',
      background: c.card,
      mainBkg: c.card,
      primaryColor: c.card,
      primaryTextColor: c.text,
      primaryBorderColor: c.line,
      secondaryColor: c.card,
      secondaryTextColor: c.text,
      secondaryBorderColor: c.line,
      tertiaryColor: c.card,
      tertiaryTextColor: c.text,
      tertiaryBorderColor: c.line,
      nodeBorder: c.line,
      nodeTextColor: c.text,
      lineColor: c.line,
      textColor: c.text,
      titleColor: c.text,
      clusterBkg: c.card,
      clusterBorder: c.faint,
      edgeLabelBackground: c.card,
      actorBkg: c.card,
      actorBorder: c.line,
      actorTextColor: c.text,
      actorLineColor: c.faint,
      signalColor: c.line,
      signalTextColor: c.text,
      labelBoxBkgColor: c.card,
      labelBoxBorderColor: c.line,
      labelTextColor: c.text,
      loopTextColor: c.text,
      noteBkgColor: c.card,
      noteBorderColor: c.faint,
      noteTextColor: c.text,
      activationBkgColor: c.panel,
      activationBorderColor: c.line,
    },
    flowchart: { curve: 'linear', htmlLabels: true, padding: 10, nodeSpacing: 44, rankSpacing: 62, diagramPadding: 10, useMaxWidth: false },
    sequence: { useMaxWidth: false, mirrorActors: false, actorFontFamily: font, noteFontFamily: font, messageFontFamily: font, actorFontSize: 12, noteFontSize: 12, messageFontSize: 12, boxMargin: 8 },
    class: { useMaxWidth: false },
    state: { useMaxWidth: false },
    er: { useMaxWidth: false },
    gantt: { useMaxWidth: false },
    journey: { useMaxWidth: false },
    mindmap: { useMaxWidth: false },
    timeline: { useMaxWidth: false },
  });
  themeDrawn = currentTheme();
}

/** Tells mermaid the theme changed, for the diagrams drawn from now on. */
export function themeChanged() {
  if (window.mermaid && themeDrawn !== currentTheme()) initialize(window.mermaid);
}

function watcher(): IntersectionObserver {
  if (observer) return observer;
  observer = new IntersectionObserver(
    (records) => {
      for (const record of records) {
        const card = cards.get(record.target);
        if (!card) continue;
        card.near = record.isIntersecting;
        if (card.near && card.state === 'idle') enqueue(card);
      }
    },
    { rootMargin: '900px 0px' },
  );
  return observer;
}

function enqueue(card: Card) {
  card.state = 'queued';
  queue.push(card);
  void pump();
}

const breathe = () =>
  new Promise<void>((resolve) => {
    if ('requestIdleCallback' in window) requestIdleCallback(() => resolve(), { timeout: 120 });
    else setTimeout(resolve, 16);
  });

// Draws the queued diagrams one at a time, the one nearest the middle of the window first.
async function pump() {
  if (running) return;
  running = true;
  try {
    const mermaid = await load();
    while (queue.length) {
      const middle = innerHeight / 2;
      const distance = (card: Card) => {
        const rect = card.el.getBoundingClientRect();
        return Math.abs(rect.top + rect.height / 2 - middle);
      };
      queue.sort((a, b) => distance(a) - distance(b));
      const card = queue.shift()!;
      if (!card.near || !cards.has(card.el)) {
        card.state = 'idle';
        continue;
      }
      card.state = 'drawing';
      card.done(await draw(mermaid, card.src));
      card.state = 'done';
      await breathe();
    }
  } catch (err) {
    const why = `Diagrams need mermaid, which didn't load: ${(err as Error).message}.`;
    for (const card of queue.splice(0)) {
      card.state = 'idle';
      card.done({ ok: false, error: why });
    }
  } finally {
    running = false;
  }
}

async function draw(mermaid: MermaidApi, src: string): Promise<Result> {
  const id = `lattice-mmd-${drawn++}`;
  try {
    const { svg } = await mermaid.render(id, sanitizeMermaid(src));
    const box = /viewBox="[\d.-]+ [\d.-]+ ([\d.]+) ([\d.]+)"/.exec(svg);
    const width = box ? Number(box[1]) : 600;
    const height = box ? Number(box[2]) : 400;
    return { ok: true, drawing: { svg, id, width, height } };
  } catch (err) {
    // Mermaid leaves its drawing of the error behind; the card shows the source in its place.
    for (const leftover of [document.getElementById(`d${id}`), document.getElementById(id)]) leftover?.remove();
    const why = err instanceof Error ? err.message.split('\n')[0].replace(/[\s:.]+$/, '') : '';
    return { ok: false, error: `This diagram couldn't be drawn${why ? `: ${why}` : ''}.` };
  }
}

/** Draws `src` into the card `el` once it comes near, telling `done` what came of it. Returns the undoing. */
export function drawWhenNear(el: Element, src: string, done: (result: Result) => void): () => void {
  const card: Card = { el, src, near: false, state: 'idle', done };
  cards.set(el, card);
  watcher().observe(el);
  return () => {
    cards.delete(el);
    observer?.unobserve(el);
  };
}

/** Draws `src` now, whether or not it's near: a diagram in the chat's answer. */
export async function drawNow(src: string): Promise<Result> {
  try {
    return await draw(await load(), src);
  } catch (err) {
    return { ok: false, error: `Diagrams need mermaid, which didn't load: ${(err as Error).message}.` };
  }
}

/** A drawing's SVG made fit to sit in a box: its size taken off, kept in proportion. */
export function fitted(drawing: Drawing): string {
  return drawing.svg.replace(/<svg([^>]*)>/, (_, attrs: string) => {
    const rest = attrs
      .replace(/\s(width|height|style)="[^"]*"/g, '')
      .replace(/\spreserveAspectRatio="[^"]*"/, '');
    return `<svg${rest} style="max-width:${drawing.width}px;max-height:${drawing.height}px" preserveAspectRatio="xMidYMid meet" aria-hidden="true">`;
  });
}

/** A drawing's SVG at its natural size, its ids renamed so it doesn't meet the card's own: for the zoom. */
export function enlarged(drawing: Drawing): string {
  return drawing.svg
    .split(drawing.id)
    .join(`${drawing.id}-zoom`)
    .replace(/<svg([^>]*)>/, (_, attrs: string) => {
      const rest = attrs.replace(/\s(width|height|style)="[^"]*"/g, '');
      return `<svg${rest} width="${drawing.width}" height="${drawing.height}" style="max-width:none" aria-hidden="true">`;
    });
}
