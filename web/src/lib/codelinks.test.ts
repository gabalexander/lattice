// Where links into the code go: to the editor when lattice serves the page, to the forge at the wiki's commit
// otherwise, and nowhere without a forge.

import { describe, expect, test } from 'vitest';
import { clickAction, codeTarget, codeUrl, commitUrl, forgeOf, openQuery, shortSha } from './codelinks';
import { renderMarkdown } from './markdown';
import type { WikiRepo } from './types';

const github: WikiRepo = {
  name: 'gabalexander/crystal',
  commit: '22ee18c82b0956c41604769ec94cf6dcb6580ccd',
  web_url: 'https://github.com/gabalexander/crystal',
  code_url: 'https://github.com/gabalexander/crystal/blob/{commit}/{path}',
};
const gitlab: WikiRepo = {
  name: 'team/app',
  commit: 'abc1234def',
  web_url: 'https://gitlab.example.com/team/app/',
  code_url: 'https://gitlab.example.com/team/app/-/blob/{commit}/{path}',
};
const bare: WikiRepo = { name: 'notes', commit: 'abc', web_url: null, code_url: null };

describe('codeUrl', () => {
  test('fills the commit and path in and appends the lines', () => {
    expect(codeUrl(github, 'src/daemon.rs', 130, 275)).toBe(`https://github.com/gabalexander/crystal/blob/${github.commit}/src/daemon.rs#L130-L275`);
    expect(codeUrl(github, 'src/daemon.rs', 7)).toBe(`https://github.com/gabalexander/crystal/blob/${github.commit}/src/daemon.rs#L7`);
    expect(codeUrl(github, 'src/daemon.rs', 7, 7)).toMatch(/#L7$/);
    expect(codeUrl(github, 'README.md')).toMatch(/\/README\.md$/);
  });

  test('encodes each part of the path, keeping its slashes', () => {
    expect(codeUrl(github, 'docs/a b/#c?.md')).toMatch(/\/docs\/a%20b\/%23c%3F\.md$/);
  });

  test('is null without a forge', () => {
    expect(codeUrl(bare, 'src/a.rs', 1)).toBeNull();
  });
});

describe('the forge', () => {
  test('is told apart by its address', () => {
    expect(forgeOf('https://github.com/a/b')).toBe('github');
    expect(forgeOf('https://gitlab.example.com/a/b')).toBe('gitlab');
    expect(forgeOf('https://git.example.com/a/b')).toBe('other');
  });

  test("links the commit to its tree, in each forge's way", () => {
    expect(commitUrl(github)).toBe(`https://github.com/gabalexander/crystal/tree/${github.commit}`);
    expect(commitUrl(gitlab)).toBe('https://gitlab.example.com/team/app/-/tree/abc1234def');
    expect(commitUrl(bare)).toBeNull();
    expect(shortSha(github.commit)).toBe('22ee18c');
  });
});

describe('codeTarget', () => {
  test('served, a link opens the editor and carries the forge for a modifier-click', () => {
    const target = codeTarget(github, 'serve', true)('src/a.rs', 3, null);
    expect(target?.href).toMatch(/\/src\/a\.rs#L3$/);
    expect(target?.title).toBe('Open src/a.rs:3 in your editor; ⌘-click opens it on the forge');
    expect(codeTarget(github, 'serve', false)('src/a.rs', null, null)?.title).toBe('Open src/a.rs in your editor; Ctrl-click opens it on the forge');
  });

  test('served without a forge, a link only opens the editor', () => {
    expect(codeTarget(bare, 'serve', true)('src/a.rs', 3, null)).toEqual({ href: null, title: 'Open src/a.rs:3 in your editor' });
  });

  test('exported, a link goes to the forge, or is plain code without one', () => {
    expect(codeTarget(github, 'static', true)('src/a.rs', 3, 9)).toEqual({ href: `${github.code_url!.replace('{commit}', github.commit).replace('{path}', 'src/a.rs')}#L3-L9`, title: 'src/a.rs:3 at 22ee18c' });
    expect(codeTarget(bare, 'static', true)('src/a.rs', 3, null)).toBeNull();
  });

  test('comes out of the renderer as the page needs it', () => {
    const html = renderMarkdown('[`run`](code:src/daemon.rs#L130)', { code: codeTarget(bare, 'serve', true) });
    expect(html).toBe('<p><a class="code-link" data-path="src/daemon.rs" data-line="130" role="link" tabindex="0" title="Open src/daemon.rs:130 in your editor"><code>run</code></a></p>');
    expect(renderMarkdown('[`run`](code:src/daemon.rs#L130)', { code: codeTarget(bare, 'static', true) })).toBe('<p><code>run</code></p>');
  });
});

describe('a click', () => {
  const none = { meta: false, ctrl: false, shift: false, alt: false };
  test('served, opens the editor; with a modifier, the forge', () => {
    expect(clickAction('serve', none, true)).toBe('editor');
    expect(clickAction('serve', none, false)).toBe('editor');
    expect(clickAction('serve', { ...none, meta: true }, true)).toBe('browser');
    expect(clickAction('serve', { ...none, ctrl: true }, false)).toBe('nothing');
  });

  test('exported, follows the link when there is one', () => {
    expect(clickAction('static', none, true)).toBe('browser');
    expect(clickAction('static', none, false)).toBe('nothing');
  });

  test("asks lattice to open the file at its line", () => {
    expect(openQuery('src/a b.rs', '12')).toBe('path=src%2Fa%20b.rs&line=12');
    expect(openQuery('src/a.rs', undefined)).toBe('path=src%2Fa.rs');
  });
});
