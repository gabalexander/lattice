// Fences as CommonMark reads them, which the generator's check of diagrams reads the same way.

import { expect, test } from 'vitest';
import { closesFence, findMermaidFences, openFence } from './fences';

test('a fence opens with three or more backticks or tildes, indented three spaces at most', () => {
  expect(openFence('```rust')).toEqual({ indent: 0, marker: '```', lang: 'rust' });
  expect(openFence('   ~~~~ python title="x"')).toEqual({ indent: 3, marker: '~~~~', lang: 'python' });
  expect(openFence('```{.mermaid}')).toEqual({ indent: 0, marker: '```', lang: 'mermaid' });
  expect(openFence('    ```')).toBeNull();
  expect(openFence('``')).toBeNull();
  expect(openFence('``` a`b')).toBeNull();
});

test('a fence closes on its own character, at least as long, with nothing after', () => {
  const open = openFence('````')!;
  expect(closesFence('````', open)).toBe(true);
  expect(closesFence('`````  ', open)).toBe(true);
  expect(closesFence('```', open)).toBe(false);
  expect(closesFence('~~~~', open)).toBe(false);
  expect(closesFence('```` x', open)).toBe(false);
});

test('finds the mermaid fences at the top level, in order, with where they are', () => {
  const md = 'Intro\n\n```mermaid\nflowchart TD\n  a --> b\n```\n\nThen\n\n~~~mermaid\nsequenceDiagram\n~~~\n';
  const found = findMermaidFences(md);
  expect(found.map((f) => f.code)).toEqual(['flowchart TD\n  a --> b', 'sequenceDiagram']);
  expect(md.slice(found[0].start, found[0].end)).toBe('```mermaid\nflowchart TD\n  a --> b\n```');
});

test('a mermaid fence quoted inside a longer one is text, not a diagram', () => {
  expect(findMermaidFences('````md\n```mermaid\nflowchart TD\n```\n````')).toEqual([]);
});

test('an empty or unclosed fence makes no diagram', () => {
  expect(findMermaidFences('```mermaid\n\n```')).toEqual([]);
  expect(findMermaidFences('```mermaid\nflowchart TD')).toEqual([]);
});
