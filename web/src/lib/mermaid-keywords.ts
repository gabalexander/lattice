// Names in a diagram that are words mermaid keeps for itself, renamed before mermaid reads it. Mermaid's lexers
// try their keywords first wherever a token starts, so a node called `call` begins a click's callback, `end`
// closes a subgraph and a participant called `Note` starts a note: each is a parse error ("got
// 'CALLBACKNAME'"). Such a name gets a `_` after its keyword, the same everywhere in the diagram (`call` ->
// `call_`, `end-x` -> `end_-x`), and still shows what it did: a flowchart's node or subgraph gets its old name as
// its label when it had none, a participant as its alias, a class as its label, a state as its description; an
// entity is quoted instead. Labels, quoted text and edges are left as they are, but for two slips of the same
// kind in a flowchart: an id starting with o or x right after an edge with no head, which mermaid takes for the
// edge's head (`a---order` is a circle-headed edge to `rder`), and a node `o` or `x` touching its edge.
//
// The words are mermaid 11.17.2's, from its flow, sequence, class, state and er grammars, where a keyword ends
// where a word does. The generator repairs a diagram the same way before it's kept (src/mermaid/keywords.rs), and
// tests/mermaid/keywords.json holds the cases both are held to.

export type Kind = 'flowchart' | 'sequence' | 'class' | 'state' | 'er';

/** The words each kind keeps for itself; the kinds whose lexer ignores case have them in lower case. */
export const KEYWORDS: Record<Kind, readonly string[]> = {
  flowchart: ['call', 'class', 'classDef', 'click', 'default', 'end', 'flowchart', 'graph', 'href', 'interpolate', 'linkStyle', 'style', 'subgraph', '_blank', '_parent', '_self', '_top'],
  sequence: ['activate', 'actor', 'alt', 'and', 'autonumber', 'box', 'break', 'create', 'critical', 'deactivate', 'destroy', 'details', 'else', 'end', 'link', 'links', 'loop', 'note', 'off', 'opt', 'option', 'over', 'par', 'par_over', 'participant', 'properties', 'rect', 'sequencediagram', 'title'],
  class: ['call', 'callback', 'class', 'classDef', 'click', 'cssClass', 'href', 'link', 'namespace', 'note', 'o', 'style', '_blank', '_parent', '_self', '_top'],
  state: ['class', 'classdef', 'click', 'default', 'href', 'note', 'scale', 'state', 'statediagram', 'style'],
  er: ['class', 'classdef', 'end', 'erdiagram', 'many', 'one', 'style', 'subgraph', 'to'],
};

const CASELESS: ReadonlySet<Kind> = new Set(['sequence', 'state', 'er']);

/** How long the keyword `name` starts with is, when it does and the word ends there; otherwise 0. */
export function reserved(kind: Kind, name: string): number {
  const folded = CASELESS.has(kind) ? name.toLowerCase() : name;
  for (const word of KEYWORDS[kind]) {
    if (folded.startsWith(word) && !/\w/.test(folded.charAt(word.length))) return word.length;
  }
  return 0;
}

/** Where a name is, on its line. */
interface Mention {
  line: number;
  start: number;
  end: number;
  /** Whether the name shows a text of its own here: a label, an alias, a description. */
  shows?: boolean;
  /** Where the old name can be given it to show, when nothing shows one: an insertion on this line. */
  give?: { at: number; text: (old: string) => string };
}

/** A text put in the place of `del` characters at `at`, on a line. */
interface Edit {
  line: number;
  at: number;
  del: number;
  text: string;
}

/** What a kind's scan found: its names, where it can declare one showing its old name, and its own repairs. */
interface Scan {
  mentions: Mention[];
  declare?: (fresh: string, old: string) => string;
  edits: Edit[];
}

/** `src` with every name that is a keyword of its kind renamed, or quoted, as the module comment says. */
export function renameKeywords(src: string): string {
  const lines = src.split('\n');
  const head = firstStatement(lines);
  if (head < 0) return src;
  const kind = kindOf(lines[head]);
  if (!kind) return src;
  const scan = SCANS[kind](lines, head);
  const name = (m: Mention) => lines[m.line].slice(m.start, m.end);
  const taken = new Set(scan.mentions.map(name));
  const byName = new Map<string, Mention[]>();
  for (const m of scan.mentions) {
    const old = name(m);
    if (reserved(kind, old)) byName.set(old, [...(byName.get(old) ?? []), m]);
  }
  const edits = [...scan.edits];
  const before = new Map<number, string[]>();
  for (const [old, mentions] of byName) {
    if (kind === 'er') {
      for (const m of mentions) edits.push({ line: m.line, at: m.start, del: m.end - m.start, text: `"${old}"` });
      continue;
    }
    const cut = reserved(kind, old);
    let mark = '_';
    while (taken.has(old.slice(0, cut) + mark + old.slice(cut))) mark += '_';
    const fresh = old.slice(0, cut) + mark + old.slice(cut);
    taken.add(fresh);
    for (const m of mentions) edits.push({ line: m.line, at: m.start, del: m.end - m.start, text: fresh });
    if (mentions.some((m) => m.shows)) continue;
    const giving = mentions.find((m) => m.give);
    if (giving?.give) {
      edits.push({ line: giving.line, at: giving.give.at, del: 0, text: giving.give.text(old) });
    } else if (scan.declare) {
      const first = mentions[0].line;
      const indent = /^\s*/.exec(lines[first])![0];
      before.set(first, [...(before.get(first) ?? []), indent + scan.declare(fresh, old)]);
    }
  }
  if (!edits.length && !before.size) return src;
  const out: string[] = [];
  lines.forEach((line, n) => {
    out.push(...(before.get(n) ?? []));
    const mine = edits.filter((e) => e.line === n).sort((a, b) => a.at - b.at || a.del - b.del);
    let text = '';
    let at = 0;
    for (const e of mine) {
      text += line.slice(at, e.at) + e.text;
      at = e.at + e.del;
    }
    out.push(text + line.slice(at));
  });
  return out.join('\n');
}

// The diagram's first statement: past blank lines, %% comments and a --- front matter block.
function firstStatement(lines: string[]): number {
  let front = false;
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n].trim();
    if (line === '---') {
      front = !front;
      continue;
    }
    if (front || !line || line.startsWith('%%')) continue;
    return n;
  }
  return -1;
}

function kindOf(header: string): Kind | null {
  const word = header.trim().split(/[\s;]/)[0];
  if (word === 'flowchart' || word === 'graph' || word === 'flowchart-elk') return 'flowchart';
  if (word === 'sequenceDiagram') return 'sequence';
  if (word === 'classDiagram' || word === 'classDiagram-v2') return 'class';
  if (word === 'stateDiagram' || word === 'stateDiagram-v2') return 'state';
  if (word === 'erDiagram') return 'er';
  return null;
}

// Each line after the header with its trimmed text and where that starts; comments and blank lines left out.
function* statements(lines: string[], head: number): Generator<{ n: number; text: string; at: number }> {
  for (let n = head + 1; n < lines.length; n++) {
    const text = lines[n].trim();
    if (!text || text.startsWith('%%')) continue;
    yield { n, text, at: lines[n].indexOf(text) };
  }
}

const SCANS: Record<Kind, (lines: string[], head: number) => Scan> = {
  flowchart: scanFlowchart,
  sequence: scanSequence,
  class: scanClass,
  state: scanState,
  er: scanEr,
};

// --- flowchart ---

// A node's id: letters, digits, `_`, `.` and anything past ASCII, with a `-` only where an edge doesn't start.
const NODE_ID = /^[A-Za-z0-9_\u0080-\uffff](?:[A-Za-z0-9_.\u0080-\uffff]|-(?=[A-Za-z0-9_]))*/;
const OPENERS = ['(((', '((', '([', '[[', '[(', '[/', '[\\', '{{', '[', '(', '{', '>'];
const CLOSERS = ['-->', '--x', '--o', '---', '==>', '===', '.->', '-.-', '.-'];
const STYLING = new Set(['style', 'classDef', 'linkStyle', 'class', 'click']);

function scanFlowchart(lines: string[], head: number): Scan {
  const scan: Scan = { mentions: [], edits: [] };
  for (let n = head; n < lines.length; n++) {
    const line = lines[n];
    if (!line.trim() || line.trim().startsWith('%%')) continue;
    const parts = splitStatements(line);
    for (const [k, [at, text]] of parts.entries()) {
      if (n === head && k === 0) continue;
      flowStatement(scan, n, at + (text.length - text.trimStart().length), text.trim());
    }
  }
  return scan;
}

// A line cut at the `;` that end statements, outside quotes and brackets: where each starts, and its text.
function splitStatements(line: string): [number, string][] {
  const out: [number, string][] = [];
  let depth = 0;
  let quoted = false;
  let start = 0;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === '"') quoted = !quoted;
    else if (quoted) continue;
    else if ('[({'.includes(c)) depth++;
    else if (')]}'.includes(c)) depth--;
    else if (c === ';' && depth <= 0) {
      out.push([start, line.slice(start, i)]);
      start = i + 1;
    }
  }
  out.push([start, line.slice(start)]);
  return out.filter(([, text]) => text.trim());
}

function flowStatement(scan: Scan, n: number, off: number, t: string) {
  const word = /^[A-Za-z_]\w*/.exec(t)?.[0] ?? '';
  const after = t.slice(word.length);
  if (word === 'end' && !after.trim()) return;
  if (word === 'direction' && /^\s+(TB|TD|BT|LR|RL)\s*$/.test(after)) return;
  if ((word === 'accTitle' || word === 'accDescr') && /^\s*[:{]/.test(after)) return;
  // A statement word, unless an edge, a `&` or a shape follows it: then it names a node.
  const statement = /^\s+[^\s&<~=.-]/.test(after);
  if (word === 'subgraph' && (!after.trim() || statement)) {
    subgraph(scan, n, off + word.length + (after.length - after.trimStart().length), after.trim());
    return;
  }
  if (STYLING.has(word) && statement) {
    // `style id css`, `class a,b name`, `click id ...`: the ids are renamed with the nodes'.
    if (word === 'classDef' || word === 'linkStyle') return;
    const rest = after.trimStart();
    const at = off + word.length + (after.length - rest.length);
    const ids = /^[^\s]+/.exec(rest)![0];
    const list = word === 'class' ? ids.split(',') : [ids];
    let from = at;
    for (const id of list) {
      if (id) scan.mentions.push({ line: n, start: from, end: from + id.length });
      from += id.length + 1;
    }
    return;
  }
  nodes(scan, n, off, t);
}

// `subgraph id`, `subgraph id [title]` or `subgraph a title of words`, the last quoted when one of its words is
// a keyword, since it's read as ids would be.
function subgraph(scan: Scan, n: number, off: number, rest: string) {
  if (!rest || rest.startsWith('"')) return;
  const id = NODE_ID.exec(rest)?.[0];
  if (!id) return;
  const after = rest.slice(id.length);
  if (after.trimStart().startsWith('[')) {
    scan.mentions.push({ line: n, start: off, end: off + id.length, shows: true });
  } else if (!after.trim()) {
    scan.mentions.push({ line: n, start: off, end: off + id.length, give: { at: off + id.length, text: (old) => `["${old}"]` } });
  } else if (rest.split(/\s+/).some((w) => reserved('flowchart', w))) {
    scan.edits.push({ line: n, at: off, del: rest.length, text: `"${rest.replace(/"/g, '#quot;')}"` });
  }
}

// A statement of nodes and edges: `a["A"] --> b & c -.->|label| d`.
function nodes(scan: Scan, n: number, off: number, t: string) {
  let i = 0;
  let node = true;
  while (i < t.length) {
    while (t[i] === ' ' || t[i] === '\t') i++;
    if (i >= t.length) return;
    if (node) {
      const id = NODE_ID.exec(t.slice(i))?.[0];
      if (!id) return;
      const start = i;
      i += id.length;
      let shows = false;
      let shaped = false;
      if (t.startsWith('@{', i)) {
        const end = closeOf(t, i + 1, '{', '}');
        if (end < 0) return;
        shows = /\blabel\s*:/.test(t.slice(i, end));
        shaped = true;
        i = end;
      } else if (OPENERS.some((open) => t.startsWith(open, i))) {
        const end = t[i] === '>' ? t.indexOf(']', i) + 1 : closeOf(t, i, t[i], { '[': ']', '(': ')', '{': '}' }[t[i]]!);
        if (end <= 0) return;
        shows = shaped = true;
        i = end;
      }
      if (t.startsWith(':::', i)) {
        i += 3;
        while (i < t.length && /[\w-]/.test(t[i])) i++;
      }
      const end = off + start + id.length;
      scan.mentions.push({ line: n, start: off + start, end, shows, give: shaped ? undefined : { at: end, text: (old) => `["${old}"]` } });
      // A node `o` or `x` touching its edge is read as the edge's tail: `o-->b`.
      if ((id === 'o' || id === 'x') && !shaped && i === start + 1 && /[-=.]/.test(t[i] ?? '')) {
        scan.edits.push({ line: n, at: end, del: 0, text: ' ' });
      }
      node = false;
    } else if (t[i] === '&') {
      i++;
      node = true;
    } else {
      const edge = edgeAt(t, i);
      if (!edge) return;
      if (edge.fix) scan.edits.push({ line: n, at: off + edge.fix.at, del: 0, text: edge.fix.text });
      i = edge.end;
      node = true;
    }
  }
}

// Where the bracket opened at `at` closes, past brackets the label opens itself and quoted text; -1 if it doesn't.
function closeOf(t: string, at: number, open: string, close: string): number {
  let depth = 0;
  let quoted = false;
  for (let i = at; i < t.length; i++) {
    const c = t[i];
    if (c === '"') quoted = !quoted;
    else if (quoted) continue;
    else if (c === open) depth++;
    else if (c === close && --depth === 0) return i + 1;
  }
  return -1;
}

// The edge at `i`, as the generator's reader takes it (src/mermaid/flowchart.rs): where it ends, past its label,
// and the space it needs when an id starting with o or x touches a headless one.
function edgeAt(t: string, i: number): { end: number; fix?: { at: number; text: string } } | null {
  if (t.startsWith('~~~', i)) {
    while (t[i] === '~') i++;
    return { end: i };
  }
  let tail = false;
  if (t[i] === '<') {
    tail = true;
    i++;
  } else if ((t[i] === 'x' || t[i] === 'o') && /[-=.]/.test(t[i + 1] ?? '') && /[-=.]/.test(t[i + 2] ?? '')) {
    tail = true;
    i++;
  }
  const from = i;
  while (/[-=.]/.test(t[i] ?? '')) i++;
  const body = t.slice(from, i);
  if (body.length < 2) return null;
  let head = false;
  let fix: { at: number; text: string } | undefined;
  if (t[i] === '>') {
    head = true;
    i++;
  } else if (t[i] === 'x' || t[i] === 'o') {
    if (/\w/.test(t[i + 1] ?? '')) {
      if (/^(-{2,}|={2,}|-?\.+-)$/.test(body)) fix = { at: i, text: (body === '--' ? '-' : body === '==' ? '=' : '') + ' ' };
    } else {
      head = true;
      i++;
    }
  }
  // `-- text -->`: an opener, the text, then the edge that closes it.
  if (!head && !tail && body.length === 2 && (t[i] === ' ' || t[i] === '\t')) {
    let best = -1;
    for (const closer of CLOSERS) {
      const k = closerAt(t.slice(i), closer);
      if (k >= 0 && (best < 0 || k < best)) best = k;
    }
    if (best >= 0) {
      i += best;
      while (/[-=.]/.test(t[i] ?? '')) i++;
      if (t[i] && '>xo'.includes(t[i])) i++;
      return { end: i };
    }
  }
  let j = i;
  while (t[j] === ' ' || t[j] === '\t') j++;
  if (t[j] === '|') {
    const close = t.indexOf('|', j + 1);
    return { end: close < 0 ? t.length : close + 1, fix };
  }
  return { end: i, fix };
}

// Where `closer` starts in `rest` after a space or an edge's own character, so `step-by-step` isn't an edge.
function closerAt(rest: string, closer: string): number {
  for (let k = rest.indexOf(closer); k >= 0; k = rest.indexOf(closer, k + 1)) {
    if (k > 0 && ' -=.'.includes(rest[k - 1])) return k;
  }
  return -1;
}

// --- sequenceDiagram ---

const ARROWS = ['<<-->>', '<<->>', '-->>', '->>', '--x', '-x', '--)', '-)', '-->', '->'];
// The words whose statement runs to the end of the line, arrows and all: a frame's text, a title.
const FREE_TEXT = new Set(['alt', 'and', 'box', 'break', 'critical', 'else', 'loop', 'opt', 'option', 'par', 'par_over', 'rect', 'title']);

function scanSequence(lines: string[], head: number): Scan {
  const scan: Scan = { mentions: [], edits: [], declare: (fresh, old) => `participant ${fresh} as ${old}` };
  const name = (n: number, at: number, text: string) => {
    const id = text.trim();
    const from = at + text.indexOf(id);
    if (id) scan.mentions.push({ line: n, start: from, end: from + id.length });
  };
  for (const { n, text: t, at } of statements(lines, head)) {
    const colon = t.indexOf(':');
    const before = colon < 0 ? t : t.slice(0, colon);
    const first = t.split(/\s/)[0];
    const word = first.toLowerCase();
    const rest = before.slice(first.length);
    const arrow = arrowIn(before);
    // `Loop->>B` and `Loop ->> B` are messages, `loop every 5 -> 10s` a frame.
    const statement = FREE_TEXT.has(word) && (!arrow || before.slice(first.length, arrow.at).trim());
    const note = word === 'note' && /^\s+(over|left of|right of)\s/i.exec(rest);
    if (arrow && !statement && !note) {
      name(n, at, before.slice(0, arrow.at));
      let k = arrow.at + arrow.len;
      while (before[k] === ' ') k++;
      if (before[k] === '+' || before[k] === '-') k++;
      name(n, at + k, before.slice(k));
      continue;
    }
    const declared = /^(?:create\s+)?(?:participant|actor)\s+/i.exec(t);
    if (declared) {
      const decl = t.slice(declared[0].length);
      const alias = /\s+as\s+/i.exec(decl);
      const config = decl.indexOf('@{');
      const id = decl.slice(0, alias ? alias.index : config >= 0 ? config : decl.length).trimEnd();
      const from = at + declared[0].length;
      if (id) scan.mentions.push({ line: n, start: from, end: from + id.length, shows: !!alias || config >= 0, give: { at: from + id.length, text: (old) => ` as ${old}` } });
    } else if (word === 'destroy' || word === 'activate' || word === 'deactivate') {
      name(n, at + first.length, rest);
    } else if (note) {
      let from = first.length + note[0].length;
      for (const who of before.slice(from).split(',')) {
        name(n, at + from, who);
        from += who.length + 1;
      }
    } else if (word === 'links' || word === 'link' || word === 'properties' || word === 'details') {
      name(n, at + first.length, rest);
    }
  }
  return scan;
}

// The first message arrow in `text`, longest first where two start at once.
function arrowIn(text: string): { at: number; len: number } | null {
  for (let i = 0; i < text.length; i++) {
    if (text[i] !== '-' && text[i] !== '<') continue;
    const arrow = ARROWS.find((a) => text.startsWith(a, i));
    if (arrow) return { at: i, len: arrow.length };
  }
  return null;
}

// --- classDiagram ---

const CLASS_NAME = /^[A-Za-z0-9_\u0080-\uffff]+/;

function scanClass(lines: string[], head: number): Scan {
  const scan: Scan = { mentions: [], edits: [], declare: (fresh, old) => `class ${fresh}["${old}"]` };
  const name = (n: number, at: number, text: string) => {
    const id = CLASS_NAME.exec(text)?.[0];
    if (id) scan.mentions.push({ line: n, start: at, end: at + id.length });
  };
  let body = false;
  for (const { n, text: t, at } of statements(lines, head)) {
    if (body) {
      if (t.startsWith('}')) body = false;
      continue;
    }
    const member = /^([A-Za-z0-9_\u0080-\uffff]+)\s*:/.exec(t);
    if (member) {
      name(n, at, t);
      continue;
    }
    const plain = t.replace(/"[^"]*"/g, (q) => ' '.repeat(q.length));
    if (/--|\.\./.test(plain)) {
      // `A "1" <|-- "*" B : label`: the names that start and end the relation.
      name(n, at, t);
      const colon = plain.indexOf(':');
      const rel = colon < 0 ? plain : plain.slice(0, colon);
      const last = /([A-Za-z0-9_\u0080-\uffff]+)(~[^~]*~)?\s*$/.exec(rel);
      if (last) scan.mentions.push({ line: n, start: at + last.index, end: at + last.index + last[1].length });
      continue;
    }
    if (/^class\s/.test(t)) {
      const from = /^class\s+/.exec(t)![0].length;
      const id = CLASS_NAME.exec(t.slice(from))?.[0];
      if (!id) continue;
      let slot = from + id.length;
      const generic = /^~[^~]*~/.exec(t.slice(slot));
      if (generic) slot += generic[0].length;
      const shows = t[slot] === '[';
      scan.mentions.push({ line: n, start: at + from, end: at + from + id.length, shows, give: { at: at + slot, text: (old) => `["${old}"]` } });
      body = t.includes('{') && !t.includes('}');
      continue;
    }
    const note = /^note\s+for\s+/.exec(t) ?? /^<<[^>]*>>\s*/.exec(t) ?? /^(style|click|link|callback)\s+/.exec(t);
    if (note) name(n, at + note[0].length, t.slice(note[0].length));
  }
  return scan;
}

// --- stateDiagram ---

const STATE_ID = /^[^\s:{<[-]+/;

function scanState(lines: string[], head: number): Scan {
  const scan: Scan = { mentions: [], edits: [], declare: (fresh, old) => `state "${old}" as ${fresh}` };
  const name = (n: number, at: number, text: string) => {
    const lead = /^\s*/.exec(text)![0].length;
    const id = STATE_ID.exec(text.slice(lead))?.[0];
    if (id) scan.mentions.push({ line: n, start: at + lead, end: at + lead + id.length });
  };
  let note = false;
  for (const { n, text: t, at } of statements(lines, head)) {
    if (note) {
      note = !/^end note$/i.test(t);
      continue;
    }
    const colon = t.indexOf(':');
    const arrow = t.indexOf('-->');
    if (arrow >= 0 && (colon < 0 || arrow < colon)) {
      name(n, at, t.slice(0, arrow));
      name(n, at + arrow + 3, t.slice(arrow + 3));
      continue;
    }
    const word = t.split(/[\s:]/)[0].toLowerCase();
    const declared = /^state\s+("[^"]*"\s+as\s+)?/i.exec(t);
    const id = declared && STATE_ID.exec(t.slice(declared[0].length))?.[0];
    const place = /^note\s+(left|right)\s+of\s+/i.exec(t);
    const styled = /^(class|style|click)\s+([^\s:]\S*)/i.exec(t);
    if (declared && id) {
      const from = at + declared[0].length;
      const fork = /^\s*(<<|\[\[)/.test(t.slice(declared[0].length + id.length));
      scan.mentions.push({ line: n, start: from, end: from + id.length, shows: !!declared[1] || fork, give: { at: from, text: (old) => `"${old}" as ` } });
    } else if (place) {
      name(n, at + place[0].length, t.slice(place[0].length));
      note = colon < 0;
    } else if (styled) {
      let from = at + styled[0].length - styled[2].length;
      for (const one of word === 'click' ? [styled[2]] : styled[2].split(',')) {
        name(n, from, one);
        from += one.length + 1;
      }
    } else if (!/^note\s+"/i.test(t) && !['classdef', 'direction', 'scale', 'hide', 'acctitle', 'accdescr', '--', '{', '}'].includes(word)) {
      name(n, at, t);
    }
  }
  return scan;
}

// --- erDiagram ---

function scanEr(lines: string[], head: number): Scan {
  const scan: Scan = { mentions: [], edits: [] };
  const name = (n: number, at: number, id: string) => {
    if (id && !id.startsWith('"')) scan.mentions.push({ line: n, start: at, end: at + id.length });
  };
  let body = false;
  for (const { n, text: t, at } of statements(lines, head)) {
    if (body) {
      body = !t.startsWith('}');
      continue;
    }
    if (t.endsWith('{')) {
      name(n, at, /^[^\s[{]+/.exec(t)?.[0] ?? '');
      body = true;
      continue;
    }
    const colon = t.indexOf(':');
    const words = [...(colon < 0 ? t : t.slice(0, colon)).matchAll(/\S+/g)];
    if (words.length === 3 && /--|\.\./.test(words[1][0])) {
      name(n, at + words[0].index, words[0][0]);
      name(n, at + words[2].index, words[2][0]);
    } else if (words.length === 1 && colon < 0 && !['style', 'classdef', 'class', 'direction'].includes(t.toLowerCase())) {
      name(n, at, words[0][0]);
    }
  }
  return scan;
}
