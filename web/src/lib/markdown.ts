// A small, safe markdown renderer for the wiki and its chat: CommonMark's blocks and inlines that a wiki uses
// (paragraphs, headings, lists, block quotes, fenced code, GitHub's tables, links, emphasis and code spans),
// everything else escaped. Raw HTML is never passed through: a tag in the text is shown as text, but for <br>.
//
// Links into the code are written `[label](code:PATH#L10-L20)`; the renderer hands each to `opts.code`, which
// says where it goes, and marks it with `data-path` and `data-line` for the page to open it. A ```mermaid
// fence becomes a diagram card the page draws; other fences are left for `highlight.ts` to colour. Pure: a
// string in, a string of HTML out.
//
// Carried over from crystal's wiki page (assets/wiki/markdown.js), with deepwiki-by-cc's fence rules, its
// `Sources:` footer and its line-citation pills (MIT; see THIRD_PARTY_NOTICES.md).

import { closesFence, openFence } from './fences';

/** Where a link into the code goes: `href` on the forge (or null when only the editor opens it), and a hint. */
export interface CodeTarget {
  href: string | null;
  title?: string;
}

export interface MarkdownOptions {
  /** Where a link into the code goes, or null to show its label alone. */
  code?: (path: string, line: number | null, end: number | null) => CodeTarget | null;
  /** Levels to push the text's own headings down by, under the page's. */
  headingOffset?: number;
  /** Whether a ```mermaid fence becomes a diagram card (true) or shows as code. */
  diagrams?: boolean;
}

const ESCAPES: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };

export function escapeHtml(text: unknown): string {
  return String(text).replace(/[&<>"']/g, (c) => ESCAPES[c]);
}

const PUNCT = /[!-/:-@[-`{-~ -⁯⸀-⹿　-〿]/;
const isPunct = (c: string) => c !== '' && PUNCT.test(c);
const isSpace = (c: string) => c === '' || /\s/.test(c);

const ENTITIES: Record<string, string> = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', copy: '©', mdash: '—', ndash: '–', hellip: '…', rarr: '→', larr: '←', times: '×',
};

function decodeEntity(entity: string): string | null {
  if (entity[1] === '#') {
    const hex = entity[2] === 'x' || entity[2] === 'X';
    const code = hex ? parseInt(entity.slice(3, -1), 16) : parseInt(entity.slice(2, -1), 10);
    if (!code || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff)) return '�';
    return String.fromCodePoint(code);
  }
  const name = entity.slice(1, -1);
  return Object.prototype.hasOwnProperty.call(ENTITIES, name) ? ENTITIES[name] : null;
}

// ---- Links ----------------------------------------------------------------------------------------------

export interface CodeRef {
  path: string;
  line: number | null;
  end: number | null;
}

/** A `code:` destination read into its path and lines: `src/x.rs#L10-L20` is lines 10 to 20. */
export function parseCodeTarget(target: string): CodeRef {
  const hash = target.indexOf('#');
  let path = hash < 0 ? target : target.slice(0, hash);
  const frag = hash < 0 ? '' : target.slice(hash + 1);
  path = path.replace(/^\.?\/+/, '');
  try {
    path = decodeURIComponent(path);
  } catch {
    // Kept as written.
  }
  const lines = /^L(\d+)(?:-L?(\d+))?$/.exec(frag);
  return { path, line: lines ? Number(lines[1]) : null, end: lines && lines[2] ? Number(lines[2]) : null };
}

type Target = { kind: 'anchor'; href: string } | { kind: 'web'; href: string } | ({ kind: 'code' } & CodeRef) | { kind: 'none' };

// What a link's destination becomes: an anchor on the page, a link into the code, a web or mail link, or
// nothing (an unknown scheme like `javascript:` is shown as its text alone). A relative path is taken as a
// path in the repository, as a wiki's links are.
function resolveLink(dest: string): Target {
  const href = dest.trim();
  if (href.startsWith('#')) return { kind: 'anchor', href };
  const scheme = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(href);
  if (scheme) {
    const name = scheme[1].toLowerCase();
    if (name === 'code') return { kind: 'code', ...parseCodeTarget(href.slice(5)) };
    if (name === 'http' || name === 'https' || name === 'mailto') return { kind: 'web', href };
    return { kind: 'none' };
  }
  if (href.startsWith('//') || href === '') return { kind: 'none' };
  return { kind: 'code', ...parseCodeTarget(href) };
}

// Link text shaped like `worker.ts:61-118` is a citation of lines, shown as a small pill.
const CITATION = /^[\w./-]+:\d+(?:-\d+)?$/;

function linkHtml(target: Target, inner: string, title: string | null, opts: MarkdownOptions, label = ''): string {
  const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
  switch (target.kind) {
    case 'anchor':
      return `<a href="${escapeHtml(target.href)}"${titleAttr}>${inner}</a>`;
    case 'web':
      return `<a href="${escapeHtml(target.href)}" target="_blank" rel="noopener noreferrer"${titleAttr}>${inner}</a>`;
    case 'code': {
      const where = opts.code ? opts.code(target.path, target.line, target.end) : null;
      if (!where) return inner;
      let attrs = ` data-path="${escapeHtml(target.path)}"`;
      if (target.line) attrs += ` data-line="${target.line}"`;
      if (target.end) attrs += ` data-end="${target.end}"`;
      if (where.href) attrs += ` href="${escapeHtml(where.href)}" target="_blank" rel="noopener noreferrer"`;
      else attrs += ' role="link" tabindex="0"';
      const hint = where.title || title;
      if (hint) attrs += ` title="${escapeHtml(hint)}"`;
      const cls = CITATION.test(label.trim()) ? 'code-link citation' : 'code-link';
      return `<a class="${cls}"${attrs}>${inner}</a>`;
    }
    default:
      return inner;
  }
}

// ---- Inlines --------------------------------------------------------------------------------------------

function findCodeClose(src: string, from: number, n: number): number {
  let i = from;
  while (i < src.length) {
    const at = src.indexOf('`', i);
    if (at < 0) return -1;
    let end = at;
    while (src[end] === '`') end++;
    if (end - at === n) return at;
    i = end;
  }
  return -1;
}

// Where the `[` at `start` is closed, skipping code spans, escapes and nested brackets.
function findLabelEnd(src: string, start: number): number {
  let depth = 0;
  for (let i = start; i < src.length; i++) {
    const c = src[i];
    if (c === '\\') {
      i++;
    } else if (c === '`') {
      const run = /^`+/.exec(src.slice(i))![0];
      const close = findCodeClose(src, i + run.length, run.length);
      i = close >= 0 ? close + run.length - 1 : i + run.length - 1;
    } else if (c === '[') {
      depth++;
    } else if (c === ']') {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

const unescapePunct = (s: string) => s.replace(/\\([!-/:-@[-`{-~])/g, '$1');

// An inline link's `(destination "title")`, from the `(` at `start`: its destination, title and end.
function parseInlineDest(src: string, start: number): { dest: string; title: string | null; end: number } | null {
  let i = start + 1;
  while (i < src.length && /[ \t\n]/.test(src[i])) i++;
  let dest: string;
  if (src[i] === '<') {
    const close = src.indexOf('>', i);
    if (close < 0 || src.slice(i + 1, close).includes('\n')) return null;
    dest = src.slice(i + 1, close);
    i = close + 1;
  } else {
    let depth = 0;
    const from = i;
    while (i < src.length) {
      const c = src[i];
      if (c === '\\' && i + 1 < src.length) {
        i += 2;
        continue;
      }
      if (/\s/.test(c)) break;
      if (c === '(') depth++;
      if (c === ')') {
        if (depth === 0) break;
        depth--;
      }
      i++;
    }
    dest = unescapePunct(src.slice(from, i));
  }
  while (i < src.length && /[ \t\n]/.test(src[i])) i++;
  let title: string | null = null;
  const open = src[i];
  if (open === '"' || open === "'" || open === '(') {
    const closeChar = open === '(' ? ')' : open;
    let close = i + 1;
    while (close < src.length && src[close] !== closeChar) close += src[close] === '\\' ? 2 : 1;
    if (close >= src.length) return null;
    title = unescapePunct(src.slice(i + 1, close));
    i = close + 1;
    while (i < src.length && /[ \t\n]/.test(src[i])) i++;
  }
  if (src[i] !== ')') return null;
  return { dest, title, end: i + 1 };
}

const normalizeLabel = (label: string) => label.trim().replace(/\s+/g, ' ').toLowerCase();

type Refs = Record<string, { dest: string; title: string | null }>;
type Delim = { t: 'delim'; ch: string; n: number; orig: number; canOpen: boolean; canClose: boolean };
type Node = { t: 'text'; v: string } | { t: 'html'; v: string } | Delim;

function renderInline(src: string, opts: MarkdownOptions, refs: Refs, inLink: boolean): string {
  const nodes: Node[] = [];
  let text = '';
  const flush = () => {
    if (text) nodes.push({ t: 'text', v: text });
    text = '';
  };
  const html = (v: string) => {
    flush();
    nodes.push({ t: 'html', v });
  };
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === '\\') {
      const next = src[i + 1];
      if (next === '\n') {
        html('<br>');
        i += 2;
      } else if (next !== undefined && isPunct(next)) {
        text += next;
        i += 2;
      } else {
        text += c;
        i++;
      }
      continue;
    }
    if (c === '`') {
      const run = /^`+/.exec(src.slice(i))![0];
      const close = findCodeClose(src, i + run.length, run.length);
      if (close < 0) {
        text += run;
        i += run.length;
        continue;
      }
      let code = src.slice(i + run.length, close).replace(/\n/g, ' ');
      if (code.length > 2 && code[0] === ' ' && code[code.length - 1] === ' ' && code.trim()) code = code.slice(1, -1);
      html(`<code>${escapeHtml(code)}</code>`);
      i = close + run.length;
      continue;
    }
    if (c === '<') {
      const rest = src.slice(i);
      const br = /^<br\s*\/?>/i.exec(rest);
      if (br) {
        html('<br>');
        i += br[0].length;
        continue;
      }
      const auto = /^<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^\s<>]*)>/.exec(rest);
      if (auto && !inLink) {
        const target = resolveLink(auto[1]);
        html(target.kind === 'none' ? escapeHtml(auto[0]) : linkHtml(target, escapeHtml(auto[1]), null, opts, auto[1]));
        i += auto[0].length;
        continue;
      }
      const mail = /^<([a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*)>/.exec(rest);
      if (mail && !inLink) {
        html(linkHtml({ kind: 'web', href: `mailto:${mail[1]}` }, escapeHtml(mail[1]), null, opts));
        i += mail[0].length;
        continue;
      }
      text += c;
      i++;
      continue;
    }
    if ((c === '[' || (c === '!' && src[i + 1] === '[')) && !inLink) {
      const image = c === '!';
      const open = image ? i + 1 : i;
      const close = findLabelEnd(src, open);
      if (close > 0) {
        const label = src.slice(open + 1, close);
        let target: Target | null = null;
        let title: string | null = null;
        let end = close + 1;
        if (src[close + 1] === '(') {
          const dest = parseInlineDest(src, close + 1);
          if (dest) {
            target = resolveLink(dest.dest);
            title = dest.title;
            end = dest.end;
          }
        }
        if (!target) {
          let ref = label;
          const full = /^\[([^\]]*)\]/.exec(src.slice(close + 1));
          if (full) {
            if (full[1].trim()) ref = full[1];
            end = close + 1 + full[0].length;
          }
          const def = refs[normalizeLabel(ref)];
          if (def) {
            target = resolveLink(def.dest);
            title = def.title;
          } else {
            end = close + 1;
          }
        }
        if (target) {
          // An image is shown as a link to it: the page loads nothing from elsewhere.
          const inner = image ? escapeHtml(label || 'image') : renderInline(label, opts, refs, true);
          html(target.kind === 'none' ? inner : linkHtml(target, inner, title, opts, label.replace(/`/g, '')));
          i = end;
          continue;
        }
      }
      text += c;
      i++;
      continue;
    }
    if (c === '*' || c === '_' || c === '~') {
      let end = i;
      while (src[end] === c) end++;
      const n = end - i;
      const before = i > 0 ? src[i - 1] : '';
      const after = end < src.length ? src[end] : '';
      const left = !isSpace(after) && (!isPunct(after) || isSpace(before) || isPunct(before));
      const right = !isSpace(before) && (!isPunct(before) || isSpace(after) || isPunct(after));
      let canOpen = left;
      let canClose = right;
      if (c === '_') {
        canOpen = left && (!right || isPunct(before));
        canClose = right && (!left || isPunct(after));
      }
      if (c === '~' && n !== 2) {
        canOpen = false;
        canClose = false;
      }
      flush();
      nodes.push({ t: 'delim', ch: c, n, orig: n, canOpen, canClose });
      i = end;
      continue;
    }
    if (c === '\n') {
      if (/ {2,}$/.test(text)) {
        text = text.replace(/ +$/, '');
        html('<br>');
      } else {
        text = text.replace(/ +$/, '') + '\n';
      }
      i++;
      while (src[i] === ' ') i++;
      continue;
    }
    if (c === '&') {
      const entity = /^&(?:#\d{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z][a-zA-Z0-9]{1,31});/.exec(src.slice(i));
      if (entity) {
        const decoded = decodeEntity(entity[0]);
        if (decoded !== null) {
          text += decoded;
          i += entity[0].length;
          continue;
        }
      }
      text += c;
      i++;
      continue;
    }
    if ((c === 'h' || c === 'w') && !inLink && (i === 0 || /[\s(*_~]/.test(src[i - 1]))) {
      const bare = /^(?:https?:\/\/|www\.)[^\s<]*[^\s<?!.,:;*_~)'"]/.exec(src.slice(i));
      if (bare) {
        const href = bare[0].startsWith('www.') ? `https://${bare[0]}` : bare[0];
        html(linkHtml({ kind: 'web', href }, escapeHtml(bare[0]), null, opts));
        i += bare[0].length;
        continue;
      }
    }
    text += c;
    i++;
  }
  flush();
  processEmphasis(nodes);
  let out = '';
  for (const node of nodes) {
    if (node.t === 'text') out += escapeHtml(node.v);
    else if (node.t === 'html') out += node.v;
    else out += escapeHtml(node.ch.repeat(node.n));
  }
  return out;
}

// CommonMark's emphasis: each closing run matched with the nearest opener of its kind before it.
function processEmphasis(nodes: Node[]) {
  for (let ci = 0; ci < nodes.length; ci++) {
    const closer = nodes[ci];
    if (closer.t !== 'delim' || !closer.canClose || closer.n === 0) continue;
    let matched = false;
    for (let oi = ci - 1; oi >= 0; oi--) {
      const opener = nodes[oi];
      if (opener.t !== 'delim' || opener.ch !== closer.ch || !opener.canOpen || opener.n === 0) continue;
      const odd = (opener.canClose || closer.canOpen) && (opener.orig + closer.orig) % 3 === 0 && !(opener.orig % 3 === 0 && closer.orig % 3 === 0);
      if (odd && closer.ch !== '~') continue;
      const use = closer.ch === '~' ? 2 : closer.n >= 2 && opener.n >= 2 ? 2 : 1;
      const tag = closer.ch === '~' ? 'del' : use === 2 ? 'strong' : 'em';
      for (let k = oi + 1; k < ci; k++) {
        const between = nodes[k];
        if (between.t === 'delim') nodes[k] = { t: 'text', v: between.ch.repeat(between.n) };
      }
      opener.n -= use;
      closer.n -= use;
      nodes.splice(ci, 0, { t: 'html', v: `</${tag}>` });
      nodes.splice(oi + 1, 0, { t: 'html', v: `<${tag}>` });
      ci += 1;
      matched = true;
      if (closer.n > 0) ci -= 1;
      break;
    }
    if (!matched && !closer.canOpen) closer.canClose = false;
  }
}

// ---- Blocks ---------------------------------------------------------------------------------------------

const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}> ?/;
const ITEM = /^( {0,3})([-+*]|\d{1,9}[.)])(?=[ \t]|$)([ \t]*)/;
const TABLE_DELIM = /^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;
const DEFINITION = /^ {0,3}\[([^\]]+)\]:[ \t]*<?([^\s>]+)>?(?:[ \t]+(?:"([^"]*)"|'([^']*)'|\(([^)]*)\)))?[ \t]*$/;
const SOURCES = /^(?:<strong>)?Sources?:/;

const isBlank = (line: string) => /^[ \t]*$/.test(line);
const indentOf = (line: string) => /^ */.exec(line)![0].length;

function expandTabs(line: string): string {
  if (!line.includes('\t')) return line;
  let out = '';
  for (const c of line) out += c === '\t' ? ' '.repeat(4 - (out.length % 4)) : c;
  return out;
}

function splitRow(line: string): string[] {
  let row = line.trim();
  if (row.startsWith('|')) row = row.slice(1);
  if (row.endsWith('|') && !row.endsWith('\\|')) row = row.slice(0, -1);
  const cells: string[] = [];
  let cell = '';
  let inCode = 0;
  for (let i = 0; i < row.length; i++) {
    const c = row[i];
    if (c === '\\' && row[i + 1] === '|') {
      cell += '|';
      i++;
    } else if (c === '`') {
      let n = 0;
      while (row[i + n] === '`') n++;
      if (inCode === 0) inCode = n;
      else if (inCode === n) inCode = 0;
      cell += '`'.repeat(n);
      i += n - 1;
    } else if (c === '|' && inCode === 0) {
      cells.push(cell.trim());
      cell = '';
    } else {
      cell += c;
    }
  }
  cells.push(cell.trim());
  return cells;
}

// Whether a line starts a block that ends the paragraph before it.
function interrupts(line: string): boolean {
  if (openFence(line) || HEADING.test(line) || QUOTE.test(line) || RULE.test(line)) return true;
  const item = ITEM.exec(line);
  if (item && !isBlank(line.slice(item[0].length))) return !/^\d/.test(item[2]) || /^1[.)]$/.test(item[2]);
  return false;
}

interface Ctx {
  opts: MarkdownOptions;
  refs: Refs;
}

function renderBlocks(lines: string[], ctx: Ctx, tight: boolean): string {
  const out: string[] = [];
  const offset = ctx.opts.headingOffset || 0;
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (isBlank(line)) {
      i++;
      continue;
    }
    const fence = openFence(line);
    if (fence) {
      const body: string[] = [];
      const dedent = new RegExp(`^ {0,${fence.indent}}`);
      i++;
      while (i < lines.length && !closesFence(lines[i], fence)) body.push(lines[i++].replace(dedent, ''));
      i++;
      const code = body.join('\n');
      const lang = fence.lang.toLowerCase();
      if (lang === 'mermaid' && ctx.opts.diagrams !== false) {
        out.push(`<figure class="diagram-card" data-mermaid><pre class="mermaid-src">${escapeHtml(code)}</pre></figure>`);
      } else {
        const attrs = lang ? ` class="language-${escapeHtml(lang)}" data-lang="${escapeHtml(lang)}"` : '';
        out.push(`<pre class="code-block"><code${attrs}>${escapeHtml(code)}</code></pre>`);
      }
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading) {
      const level = Math.min(6, heading[1].length + offset);
      out.push(`<h${level}>${renderInline(heading[2] || '', ctx.opts, ctx.refs, false)}</h${level}>`);
      i++;
      continue;
    }
    if (RULE.test(line)) {
      out.push('<hr>');
      i++;
      continue;
    }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && !isBlank(lines[i])) {
        if (QUOTE.test(lines[i])) body.push(lines[i].replace(QUOTE, ''));
        else if (interrupts(lines[i])) break;
        else body.push(lines[i]);
        i++;
      }
      out.push(`<blockquote>${renderBlocks(body, ctx, false)}</blockquote>`);
      continue;
    }
    if (ITEM.test(line)) {
      i = renderList(lines, i, ctx, out);
      continue;
    }
    if (line.includes('|') && i + 1 < lines.length && TABLE_DELIM.test(lines[i + 1]) && lines[i + 1].includes('-')) {
      const head = splitRow(line);
      const aligns = splitRow(lines[i + 1]).map((cell) =>
        cell.startsWith(':') && cell.endsWith(':') ? 'center' : cell.endsWith(':') ? 'right' : cell.startsWith(':') ? 'left' : '',
      );
      if (aligns.length === head.length) {
        i += 2;
        const rows: string[][] = [];
        while (i < lines.length && !isBlank(lines[i]) && !interrupts(lines[i])) rows.push(splitRow(lines[i++]));
        const cell = (tag: string, text: string | undefined, col: number) => {
          const align = aligns[col] ? ` style="text-align:${aligns[col]}"` : '';
          return `<${tag}${align}>${renderInline(text || '', ctx.opts, ctx.refs, false)}</${tag}>`;
        };
        const thead = `<thead><tr>${head.map((text, col) => cell('th', text, col)).join('')}</tr></thead>`;
        const tbody = rows.length ? `<tbody>${rows.map((row) => `<tr>${head.map((_, col) => cell('td', row[col], col)).join('')}</tr>`).join('')}</tbody>` : '';
        out.push(`<div class="table-wrap"><table>${thead}${tbody}</table></div>`);
        continue;
      }
    }
    if (indentOf(line) >= 4 && !tight) {
      const body: string[] = [];
      while (i < lines.length && (indentOf(lines[i]) >= 4 || isBlank(lines[i]))) body.push(lines[i++].slice(4));
      while (body.length && isBlank(body[body.length - 1])) body.pop();
      out.push(`<pre class="code-block"><code>${escapeHtml(body.join('\n'))}</code></pre>`);
      continue;
    }
    // A paragraph, or a heading underlined with = or -.
    const para = [line.trimStart()];
    i++;
    let setext = 0;
    while (i < lines.length && !isBlank(lines[i])) {
      if (/^ {0,3}=+[ \t]*$/.test(lines[i])) {
        setext = 1;
        i++;
        break;
      }
      if (/^ {0,3}-+[ \t]*$/.test(lines[i])) {
        setext = 2;
        i++;
        break;
      }
      if (interrupts(lines[i])) break;
      para.push(lines[i++].trimStart());
    }
    const inner = renderInline(para.join('\n').trimEnd(), ctx.opts, ctx.refs, false);
    if (setext) {
      const level = Math.min(6, setext + offset);
      out.push(`<h${level}>${inner}</h${level}>`);
    } else if (tight) {
      out.push(inner);
    } else {
      // A `Sources:` line closes a part with the files it was written from: a quiet footer, not prose.
      out.push(SOURCES.test(inner) ? `<p class="sources">${inner}</p>` : `<p>${inner}</p>`);
    }
  }
  return out.join(tight ? '\n' : '');
}

// A list from line `start`: its items, each its lines with the marker's indent taken off, rendered as blocks;
// tight (no paragraphs) unless a blank line separates its items or the blocks in one.
function renderList(lines: string[], start: number, ctx: Ctx, out: string[]): number {
  const first = ITEM.exec(lines[start])!;
  const ordered = /^\d/.test(first[2]);
  const delim = first[2].slice(-1);
  const items: { body: string[]; number: number | null }[] = [];
  let loose = false;
  let i = start;
  while (i < lines.length) {
    const m = ITEM.exec(lines[i]);
    if (!m) break;
    if (/^\d/.test(m[2]) !== ordered || m[2].slice(-1) !== delim) break;
    const rest = lines[i].slice(m[0].length);
    const spacing = m[3].length;
    const width = m[1].length + m[2].length + (spacing >= 1 && spacing <= 4 && !isBlank(rest) ? spacing : 1);
    const body = [isBlank(rest) ? '' : ' '.repeat(Math.max(0, spacing - (width - m[1].length - m[2].length))) + rest];
    i++;
    let sawBlank = false;
    while (i < lines.length) {
      const line = lines[i];
      if (isBlank(line)) {
        body.push('');
        sawBlank = true;
        i++;
        continue;
      }
      if (indentOf(line) >= width) {
        if (sawBlank && body.some((l) => !isBlank(l))) loose = true;
        body.push(line.slice(width));
        sawBlank = false;
        i++;
        continue;
      }
      if (!sawBlank && !ITEM.test(line) && !interrupts(line) && !isBlank(body[body.length - 1] || '')) {
        body.push(line.trimStart());
        i++;
        continue;
      }
      break;
    }
    while (body.length && isBlank(body[body.length - 1])) body.pop();
    if (sawBlank && i < lines.length) {
      const next = ITEM.exec(lines[i]);
      if (next && /^\d/.test(next[2]) === ordered && next[2].slice(-1) === delim) loose = true;
    }
    items.push({ body, number: ordered ? parseInt(m[2], 10) : null });
  }
  const tag = ordered ? 'ol' : 'ul';
  const startAttr = ordered && items[0].number !== 1 ? ` start="${items[0].number}"` : '';
  const html = items.map(({ body }) => {
    let task = '';
    const check = /^\[([ xX])\][ \t]+/.exec(body[0] || '');
    if (check) {
      body[0] = body[0].slice(check[0].length);
      task = `<input type="checkbox" disabled${check[1] === ' ' ? '' : ' checked'}> `;
    }
    return `<li${task ? ' class="task"' : ''}>${task}${renderBlocks(body, ctx, !loose)}</li>`;
  });
  out.push(`<${tag}${startAttr}>${html.join('')}</${tag}>`);
  return i;
}

/** Some markdown as HTML, safe to put in the page. */
export function renderMarkdown(src: string | null | undefined, opts: MarkdownOptions = {}): string {
  const lines = String(src || '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map(expandTabs);
  // Reference definitions are read first, so a link may come before its definition.
  const refs: Refs = {};
  const kept: string[] = [];
  let fence: ReturnType<typeof openFence> = null;
  for (const line of lines) {
    if (fence) {
      if (closesFence(line, fence)) fence = null;
      kept.push(line);
      continue;
    }
    fence = openFence(line);
    const def = !fence && DEFINITION.exec(line);
    if (def) {
      const key = normalizeLabel(def[1]);
      if (!refs[key]) refs[key] = { dest: def[2], title: def[3] || def[4] || def[5] || null };
    } else {
      kept.push(line);
    }
  }
  return renderBlocks(kept, { opts, refs }, false);
}

/** One line of markdown's inlines as HTML, as a tool's line in the chat is. */
export function renderInlineMarkdown(src: string, opts: MarkdownOptions = {}): string {
  return renderInline(src, opts, {}, false);
}

/** The text of some markdown with its marks taken out, for search. */
export function plainText(src: string | null | undefined): string {
  return String(src || '')
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[`*_~#>|]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}
