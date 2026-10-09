// Names that are mermaid's keywords renamed, held to the cases the generator's repair is held to too.

import { expect, test } from 'vitest';
import fixture from '../../../tests/mermaid/keywords.json';
import { KEYWORDS, renameKeywords, type Kind } from './mermaid-keywords';

const kinds = Object.keys(fixture.words) as Kind[];

test('the keywords are the ones the generator knows', () => {
  expect(KEYWORDS).toEqual(fixture.words);
});

test.each(kinds)('every %s keyword is renamed where it names something, and shows its old name', (kind) => {
  const { before, after } = fixture.templates[kind];
  for (const word of KEYWORDS[kind]) {
    expect(renameKeywords(before.replaceAll('{w}', word))).toBe(after.replaceAll('{w}', word));
  }
});

test.each(fixture.cases)('$name', ({ before, after }) => {
  expect(renameKeywords(before)).toBe(after);
});

test('a diagram with nothing to rename comes back as it was', () => {
  const src = 'flowchart TD\n  a["A"] --> b';
  expect(renameKeywords(src)).toBe(src);
});
