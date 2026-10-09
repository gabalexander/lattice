// Fenced code blocks as CommonMark reads them, shared by the markdown renderer and the scan for diagrams, so
// both agree on where a fence starts and ends: a ```mermaid fence quoted inside a longer fence is the outer
// block's text, not a diagram. The generator checks diagrams with the same rules (docs/web.md).
//
// Adapted from deepwiki-by-cc's src/lib/markdown-fences.ts (MIT, Copyright (c) 2026 Andy Tran; see
// THIRD_PARTY_NOTICES.md).

/** A fence's opening line: its indent, marker and info string's first word. */
export interface FenceOpen {
  indent: number;
  marker: string;
  lang: string;
}

const OPEN = /^( {0,3})(`{3,}|~{3,})(.*)$/;

/** The fence `line` opens, if it opens one: up to three spaces, then three or more backticks or tildes; a
 * backtick fence's info string has no backtick in it. */
export function openFence(line: string): FenceOpen | null {
  const m = OPEN.exec(line);
  if (!m) return null;
  const info = m[3].trim();
  if (m[2][0] === '`' && info.includes('`')) return null;
  const lang = (info.split(/\s+/)[0] || '').replace(/[{}.]/g, '');
  return { indent: m[1].length, marker: m[2], lang };
}

/** Whether `line` closes the fence `open` opened: the same character, at least as many, nothing after. */
export function closesFence(line: string, open: FenceOpen): boolean {
  const m = OPEN.exec(line);
  return !!m && m[2][0] === open.marker[0] && m[2].length >= open.marker.length && m[3].trim() === '';
}

export interface MermaidFence {
  /** Where its opening line starts. */
  start: number;
  /** Just past its closing marker, or the end of the text when it isn't closed. */
  end: number;
  code: string;
}

/** The ```mermaid fences at the top level of some markdown, in order. */
export function findMermaidFences(markdown: string): MermaidFence[] {
  const found: MermaidFence[] = [];
  let open: (FenceOpen & { start: number; lines: string[] }) | null = null;
  let offset = 0;
  for (const line of markdown.split('\n')) {
    if (!open) {
      const fence = openFence(line);
      if (fence) open = { ...fence, start: offset, lines: [] };
    } else if (closesFence(line, open)) {
      const code = open.lines.join('\n').trim();
      if (open.lang.toLowerCase() === 'mermaid' && code) found.push({ start: open.start, end: offset + line.length, code });
      open = null;
    } else {
      open.lines.push(line);
    }
    offset += line.length + 1;
  }
  return found;
}
