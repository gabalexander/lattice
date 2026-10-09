// Where the wiki's links into the code go. Served by lattice, a click opens the file at the line in the user's
// editor (`GET /api/repos/<key>/open`) and ⌘ or Ctrl-click opens it on the forge, at the commit the wiki was
// written from; exported, every link goes to the forge, and with no forge a name is plain code.

import type { CodeTarget } from './markdown';
import type { WikiRepo } from './types';

export type Mode = 'serve' | 'static';

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

/** What a `code:` link becomes on the page, for the markdown renderer. */
export function codeTarget(repo: WikiRepo, mode: Mode, mac: boolean) {
  return (path: string, line: number | null, end: number | null): CodeTarget | null => {
    const where = `${path}${line ? `:${line}` : ''}`;
    const href = codeUrl(repo, path, line, end);
    if (mode === 'serve') {
      const forge = href ? `; ${mac ? '⌘' : 'Ctrl'}-click opens it on the forge` : '';
      return { href, title: `Open ${where} in your editor${forge}` };
    }
    return href ? { href, title: `${where} at ${shortSha(repo.commit)}` } : null;
  };
}

/** What a click on a link into the code does: open it in the editor, or leave it to the browser. */
export function clickAction(mode: Mode, modifiers: { meta: boolean; ctrl: boolean; shift: boolean; alt: boolean }, hasHref: boolean): 'editor' | 'browser' | 'nothing' {
  if (mode !== 'serve') return hasHref ? 'browser' : 'nothing';
  if (modifiers.meta || modifiers.ctrl || modifiers.shift || modifiers.alt) return hasHref ? 'browser' : 'nothing';
  return 'editor';
}

/** The query `GET /api/repos/<key>/open` takes for a link's `data-path` and `data-line`. */
export function openQuery(path: string, line: string | number | null | undefined): string {
  return `path=${encodeURIComponent(path)}${line ? `&line=${encodeURIComponent(String(line))}` : ''}`;
}
