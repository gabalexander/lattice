// The repairs made to a diagram's source before mermaid reads it.

import { expect, test } from 'vitest';
import { sanitizeMermaid } from './mermaid-sanitize';

test('a <T> placeholder becomes {T}, or ~T~ in a class diagram', () => {
  expect(sanitizeMermaid('flowchart TD\n  a["Vec<T>"] --> b')).toBe('flowchart TD\n  a["Vec{T}"] --> b');
  expect(sanitizeMermaid('classDiagram\n  class Box<T>')).toBe('classDiagram\n  class Box~T~');
});

test('line breaks and closed tags are kept for HTML labels', () => {
  const src = 'flowchart TD\n  a["Loader<br/>(src/x.rs)"] --> b["<b>bold</b>"]';
  expect(sanitizeMermaid(src)).toBe(src);
});

test('an arrow with too many heads is a sequence arrow', () => {
  expect(sanitizeMermaid('sequenceDiagram\n  A->>>B: hi')).toBe('sequenceDiagram\n  A->>B: hi');
});

test('spaces around a quoted label go', () => {
  expect(sanitizeMermaid('flowchart TD\n  a[ "label" ] --> b')).toBe('flowchart TD\n  a["label"] --> b');
});

test('an unquoted label with punctuation mermaid reads as syntax is quoted', () => {
  expect(sanitizeMermaid('flowchart TD\n  a[run (src/x.rs)] --> b')).toBe('flowchart TD\n  a["run (src/x.rs)"] --> b');
  expect(sanitizeMermaid('flowchart TD\n  a[plain words] --> b')).toBe('flowchart TD\n  a[plain words] --> b');
});

test('a name that is a mermaid keyword is renamed', () => {
  expect(sanitizeMermaid('flowchart LR\n  a --> call["Call"]\n  call --> b')).toBe('flowchart LR\n  a --> call_["Call"]\n  call_ --> b');
});
