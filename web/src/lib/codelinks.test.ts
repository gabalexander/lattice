// Where links into the code go: where the settings say when lattice serves the page (an editor by its link,
// lattice's editor or the forge), to the forge at the wiki's commit otherwise, and nowhere without a forge.

import { describe, expect, test } from 'vitest';
import { OPEN_IN, clickAction, codeTarget, codeUrl, commitUrl, editorUrl, forgeOf, openQuery, placeName, shortSha } from './codelinks';
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

describe('the editors', () => {
  const home = { root: '/Users/me/src/crystal', origin: 'https://github.com/gabalexander/crystal' };

  test('VS Code, Cursor and Zed take the absolute path and the line', () => {
    expect(editorUrl('vscode', home, 'src/main.rs', 12)).toBe('vscode://file/Users/me/src/crystal/src/main.rs:12');
    expect(editorUrl('cursor', home, 'src/main.rs', 12)).toBe('cursor://file/Users/me/src/crystal/src/main.rs:12');
    expect(editorUrl('zed', home, 'src/main.rs', 12)).toBe('zed://file/Users/me/src/crystal/src/main.rs:12');
    expect(editorUrl('vscode', home, 'README.md', null)).toBe('vscode://file/Users/me/src/crystal/README.md');
  });

  test("encode each part of the path, keeping its slashes, and don't mind a root's slash or a path's ./", () => {
    const odd = { root: '/Users/me/My Code/', origin: null };
    expect(editorUrl('vscode', odd, './docs/a b#?.md', 3)).toBe('vscode://file/Users/me/My%20Code/docs/a%20b%23%3F.md:3');
  });

  test('a JetBrains IDE is reached through the Toolbox App, by its project, with lines from 0', () => {
    const url = new URL(editorUrl('goland', home, 'src/main.rs', 12)!);
    expect(url.protocol).toBe('jetbrains:');
    expect(url.host + url.pathname).toBe('goland/navigate/reference');
    expect(Object.fromEntries(url.searchParams)).toEqual({ project: 'crystal', origin: home.origin, path: 'src/main.rs:11' });
    expect(editorUrl('webstorm', home, 'a.ts', null)).toBe('jetbrains://web-storm/navigate/reference?project=crystal&origin=https%3A%2F%2Fgithub.com%2Fgabalexander%2Fcrystal&path=a.ts');
    expect(editorUrl('rider', { root: '/r/app' }, 'a.cs', 1)).toBe('jetbrains://rd/navigate/reference?project=app&path=a.cs%3A0');
  });

  test("every JetBrains IDE offered has the Toolbox App's name for it", () => {
    for (const { value } of OPEN_IN.find((g) => g.group.startsWith('JetBrains'))!.places) {
      expect(editorUrl(value, home, 'a', 1), value).toMatch(/^jetbrains:\/\/[a-z-]+\/navigate\/reference\?/);
    }
  });

  test("lattice's editor and the forge aren't opened by a link of their own", () => {
    expect(editorUrl('editor', home, 'a', 1)).toBeNull();
    expect(editorUrl('forge', home, 'a', 1)).toBeNull();
  });

  test('are named in a sentence', () => {
    expect(placeName('vscode')).toBe('VS Code');
    expect(placeName('intellij')).toBe('IntelliJ IDEA');
    expect(placeName('editor')).toBe('your editor');
    expect(placeName('forge')).toBe('the forge');
  });
});

describe('codeTarget', () => {
  test('served, a link says where it opens and carries the forge for a modifier-click', () => {
    const target = codeTarget(github, 'serve', true)('src/a.rs', 3, null);
    expect(target?.href).toMatch(/\/src\/a\.rs#L3$/);
    expect(target?.title).toBe('Open src/a.rs:3 in VS Code; ⌘-click opens it on the forge');
    expect(codeTarget(github, 'serve', false, 'editor')('src/a.rs', null, null)?.title).toBe('Open src/a.rs in your editor; Ctrl-click opens it on the forge');
    expect(codeTarget(github, 'serve', true, 'forge')('src/a.rs', 3, null)?.title).toBe('Open src/a.rs:3 on the forge, at 22ee18c');
  });

  test('served without a forge, a link only opens the editor', () => {
    expect(codeTarget(bare, 'serve', true, 'zed')('src/a.rs', 3, null)).toEqual({ href: null, title: 'Open src/a.rs:3 in Zed' });
    expect(codeTarget(bare, 'serve', true, 'forge')('src/a.rs', 3, null)?.title).toBe("src/a.rs:3: there's no forge to open it on");
  });

  test('exported, a link goes to the forge, or is plain code without one', () => {
    expect(codeTarget(github, 'static', true)('src/a.rs', 3, 9)).toEqual({ href: `${github.code_url!.replace('{commit}', github.commit).replace('{path}', 'src/a.rs')}#L3-L9`, title: 'src/a.rs:3 at 22ee18c' });
    expect(codeTarget(bare, 'static', true)('src/a.rs', 3, null)).toBeNull();
  });

  test('comes out of the renderer as the page needs it', () => {
    const html = renderMarkdown('[`run`](code:src/daemon.rs#L130)', { code: codeTarget(bare, 'serve', true, 'editor') });
    expect(html).toBe('<p><a class="code-link" data-path="src/daemon.rs" data-line="130" role="link" tabindex="0" title="Open src/daemon.rs:130 in your editor"><code>run</code></a></p>');
    expect(renderMarkdown('[`run`](code:src/daemon.rs#L130)', { code: codeTarget(bare, 'static', true) })).toBe('<p><code>run</code></p>');
  });
});

describe('a click', () => {
  const none = { meta: false, ctrl: false, shift: false, alt: false };
  test('served, opens it where the settings say; with a modifier, on the forge', () => {
    expect(clickAction('serve', 'vscode', none, true)).toBe('open');
    expect(clickAction('serve', 'editor', none, false)).toBe('open');
    expect(clickAction('serve', 'vscode', { ...none, meta: true }, true)).toBe('browser');
    expect(clickAction('serve', 'vscode', { ...none, ctrl: true }, false)).toBe('nothing');
  });

  test('served, set to the forge, follows the link, and says so when there is none', () => {
    expect(clickAction('serve', 'forge', none, true)).toBe('browser');
    expect(clickAction('serve', 'forge', none, false)).toBe('open');
  });

  test('exported, follows the link when there is one', () => {
    expect(clickAction('static', 'vscode', none, true)).toBe('browser');
    expect(clickAction('static', 'editor', none, false)).toBe('nothing');
  });

  test("asks lattice to open the file at its line", () => {
    expect(openQuery('src/a b.rs', '12')).toBe('path=src%2Fa%20b.rs&line=12');
    expect(openQuery('src/a.rs', undefined)).toBe('path=src%2Fa.rs');
  });
});
