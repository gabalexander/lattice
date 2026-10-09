// Small repairs to a diagram's source before mermaid reads it, for the slips models make most: `<T>`
// placeholders mermaid takes for tags, `->>>` arrows, spaces around a quoted label, and labels with
// punctuation mermaid reads as syntax left unquoted.
//
// Adapted from deepwiki-by-cc's src/lib/mermaid-sanitize.ts (MIT, Copyright (c) 2026 Andy Tran; see
// THIRD_PARTY_NOTICES.md).

const QUOTED_SHAPE_LABEL = /(^|[^\w"])([A-Za-z][\w-]*)([[{(])\s*"([^"\n]*)"\s*([\]})])/g;

// A lone `<word>` is a placeholder; a tag closed later, or a void one like <br>, is HTML a label may hold.
const ANGLE_TOKEN = /(?<!<)<([A-Za-z][A-Za-z0-9_-]*)>(?!>)/gi;
const VOID_TAGS = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr']);

function isHtmlElement(source: string, at: number, token: string, tag: string): boolean {
  const name = tag.toLowerCase();
  if (VOID_TAGS.has(name)) return true;
  return new RegExp(`</${name}\\s*>`, 'i').test(source.slice(at + token.length));
}

const matching = (open: string, close: string) => (open === '[' && close === ']') || (open === '{' && close === '}') || (open === '(' && close === ')');

export function sanitizeMermaid(src: string): string {
  // A class diagram writes generics as ~T~, since braces open a class's body there; {T} reads well elsewhere.
  const first = src.split('\n').find((line) => line.trim())?.trim().toLowerCase() ?? '';
  const classDiagram = first.startsWith('classdiagram');
  const placeholders = src
    .replace(/->>>+/g, '->>')
    .replace(ANGLE_TOKEN, (token, tag: string, at: number, whole: string) => (isHtmlElement(whole, at, token, tag) ? token : classDiagram ? `~${tag}~` : `{${tag}}`));
  const quoted = placeholders.replace(QUOTED_SHAPE_LABEL, (match, prefix: string, id: string, open: string, label: string, close: string) =>
    matching(open, close) ? `${prefix}${id}${open}"${label}"${close}` : match,
  );
  return quoted.replace(/(\w+)\[([^\]"]+)\]/g, (match, id: string, label: string) =>
    /[():,;{}|<>]/.test(label) ? `${id}["${label.replace(/"/g, '#quot;')}"]` : match,
  );
}
