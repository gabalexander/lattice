// Where the wiki's links into the code go. Served by lattice, a click opens the file at the line where the
// settings' `open_code_in` says: an editor on this machine by its URL scheme, the editor lattice starts
// (`GET /api/repos/<key>/open`), or the forge; ⌘ or Ctrl-click opens it on the forge, at the commit the wiki was
// written from. Exported, every link goes to the forge, and with no forge a name is plain code.

import type { CodeTarget } from './markdown';
import type { OpenIn, WikiRepo } from './types';

export type Mode = 'serve' | 'static';

/** The places code opens in, as the settings and the page offer them, in groups; `short` is the page's name for
 * one where there's little room. */
export const OPEN_IN: { group: string; places: { value: OpenIn; name: string; short?: string }[] }[] = [
  {
    group: 'Editors',
    places: [
      { value: 'vscode', name: 'VS Code' },
      { value: 'cursor', name: 'Cursor' },
      { value: 'zed', name: 'Zed' },
    ],
  },
  {
    group: 'JetBrains, through the Toolbox App',
    places: [
      { value: 'intellij', name: 'IntelliJ IDEA' },
      { value: 'pycharm', name: 'PyCharm' },
      { value: 'goland', name: 'GoLand' },
      { value: 'webstorm', name: 'WebStorm' },
      { value: 'clion', name: 'CLion' },
      { value: 'rider', name: 'Rider' },
      { value: 'phpstorm', name: 'PhpStorm' },
      { value: 'rubymine', name: 'RubyMine' },
    ],
  },
  {
    group: 'Elsewhere',
    places: [
      { value: 'editor', name: '$VISUAL or $EDITOR, where lattice runs', short: '$VISUAL or $EDITOR' },
      { value: 'forge', name: 'The forge, in the browser', short: 'The forge' },
    ],
  },
];

/** What a place is called in a sentence: "Open it in VS Code", "in your editor", "on the forge". */
export function placeName(place: OpenIn): string {
  if (place === 'editor') return 'your editor';
  if (place === 'forge') return 'the forge';
  return OPEN_IN.flatMap((g) => g.places).find((p) => p.value === place)?.name ?? 'your editor';
}

/** The JetBrains Toolbox App's name for each IDE in its links, as the IDEs' own "Copy Toolbox link" writes it. */
const TOOLBOX: Partial<Record<OpenIn, string>> = {
  intellij: 'idea',
  pycharm: 'pycharm',
  goland: 'goland',
  webstorm: 'web-storm',
  clion: 'clion',
  rider: 'rd',
  phpstorm: 'php-storm',
  rubymine: 'rubymine',
};

/** A repo's code on this machine, for the editors' links: where it is, and its forge, which JetBrains finds a
 * project of yours by. */
export interface Checkout {
  root: string;
  origin?: string | null;
}

/** The link that opens `path`, of the checkout, at `line` in an editor on this machine; null for a place that
 * isn't opened by a link: lattice's editor, or the forge. */
export function editorUrl(place: OpenIn, checkout: Checkout, path: string, line: number | null): string | null {
  const root = checkout.root.replace(/\/+$/, '');
  const relative = path.replace(/^(\.\/)+/, '');
  if (place === 'vscode' || place === 'cursor' || place === 'zed') {
    // `<scheme>://file/<absolute path>:<line>`, the path's parts encoded, as VS Code's docs give it; Cursor's
    // handler is VS Code's, and Zed's takes the same.
    const file = `${root}/${relative}`.split('/').map(encodeURIComponent).join('/');
    return `${place}://file${file}${line ? `:${line}` : ''}`;
  }
  const tool = TOOLBOX[place];
  if (!tool) return null;
  // The Toolbox App hands it to the IDE, which finds the project among those open or opened lately by its
  // directory's name or its remote, the path in it, and counts lines from 0.
  const query = new URLSearchParams({ project: root.split('/').pop() || root });
  if (checkout.origin) query.set('origin', checkout.origin);
  query.set('path', `${relative}${line ? `:${line - 1}` : ''}`);
  return `jetbrains://${tool}/navigate/reference?${query}`;
}

/** The file at `path` on the forge at the wiki's commit, at its lines; null without a forge. */
export function codeUrl(repo: WikiRepo, path: string, line: number | null = null, end: number | null = null): string | null {
  if (!repo.code_url) return null;
  const encoded = path.split('/').map(encodeURIComponent).join('/');
  let url = repo.code_url.replace('{commit}', encodeURIComponent(repo.commit || 'HEAD')).replace('{path}', encoded);
  if (line) url += `#L${line}${end && end !== line ? `-L${end}` : ''}`;
  return url;
}

export type Forge = 'github' | 'gitlab' | 'other';

export function forgeOf(webUrl: string): Forge {
  if (/^https?:\/\/(www\.)?github\.com\//i.test(webUrl)) return 'github';
  if (/gitlab/i.test(webUrl)) return 'gitlab';
  return 'other';
}

/** The repo's tree at `commit` on its forge. */
export function commitUrl(repo: WikiRepo): string | null {
  if (!repo.web_url || !repo.commit) return null;
  const base = repo.web_url.replace(/\/+$/, '');
  return forgeOf(base) === 'gitlab' ? `${base}/-/tree/${repo.commit}` : `${base}/tree/${repo.commit}`;
}

export const shortSha = (sha: string | null | undefined) => String(sha || '').slice(0, 7);

/** What a `code:` link becomes on the page, for the markdown renderer; served, it says where a click opens it. */
export function codeTarget(repo: WikiRepo, mode: Mode, mac: boolean, place: OpenIn = 'vscode') {
  return (path: string, line: number | null, end: number | null): CodeTarget | null => {
    const where = `${path}${line ? `:${line}` : ''}`;
    const href = codeUrl(repo, path, line, end);
    if (mode === 'serve') {
      if (place === 'forge') return { href, title: href ? `Open ${where} on the forge, at ${shortSha(repo.commit)}` : `${where}: there's no forge to open it on` };
      const forge = href ? `; ${mac ? '⌘' : 'Ctrl'}-click opens it on the forge` : '';
      return { href, title: `Open ${where} in ${placeName(place)}${forge}` };
    }
    return href ? { href, title: `${where} at ${shortSha(repo.commit)}` } : null;
  };
}

/** What a click on a link into the code does: open it where the settings say, or leave it to the browser, which
 * follows the link to the forge. */
export function clickAction(
  mode: Mode,
  place: OpenIn,
  modifiers: { meta: boolean; ctrl: boolean; shift: boolean; alt: boolean },
  hasHref: boolean,
): 'open' | 'browser' | 'nothing' {
  if (mode !== 'serve') return hasHref ? 'browser' : 'nothing';
  if (modifiers.meta || modifiers.ctrl || modifiers.shift || modifiers.alt) return hasHref ? 'browser' : 'nothing';
  return place === 'forge' && hasHref ? 'browser' : 'open';
}

/** The query `GET /api/repos/<key>/open` takes for a link's `data-path` and `data-line`. */
export function openQuery(path: string, line: string | number | null | undefined): string {
  return `path=${encodeURIComponent(path)}${line ? `&line=${encodeURIComponent(String(line))}` : ''}`;
}
