// Colours the code blocks in some rendered markdown with highlight.js, loaded the first time a block needs it:
// a wiki quotes code rarely, so most pages never load it. Only the languages a codebase's wiki is likely to
// quote are bundled, to keep it light; a block in another language stays plain.

import type { HLJSApi } from 'highlight.js';

let loading: Promise<HLJSApi> | null = null;

function load(): Promise<HLJSApi> {
  loading ??= (async () => {
    const [{ default: hljs }, ...languages] = await Promise.all([
      import('highlight.js/lib/core'),
      import('highlight.js/lib/languages/bash'),
      import('highlight.js/lib/languages/c'),
      import('highlight.js/lib/languages/cpp'),
      import('highlight.js/lib/languages/css'),
      import('highlight.js/lib/languages/diff'),
      import('highlight.js/lib/languages/go'),
      import('highlight.js/lib/languages/ini'),
      import('highlight.js/lib/languages/java'),
      import('highlight.js/lib/languages/javascript'),
      import('highlight.js/lib/languages/json'),
      import('highlight.js/lib/languages/kotlin'),
      import('highlight.js/lib/languages/python'),
      import('highlight.js/lib/languages/ruby'),
      import('highlight.js/lib/languages/rust'),
      import('highlight.js/lib/languages/sql'),
      import('highlight.js/lib/languages/swift'),
      import('highlight.js/lib/languages/typescript'),
      import('highlight.js/lib/languages/xml'),
      import('highlight.js/lib/languages/yaml'),
    ]);
    const names = ['bash', 'c', 'cpp', 'css', 'diff', 'go', 'ini', 'java', 'javascript', 'json', 'kotlin', 'python', 'ruby', 'rust', 'sql', 'swift', 'typescript', 'xml', 'yaml'];
    names.forEach((name, i) => hljs.registerLanguage(name, languages[i].default));
    hljs.registerAliases(['sh', 'zsh', 'shell', 'console'], { languageName: 'bash' });
    hljs.registerAliases(['toml'], { languageName: 'ini' });
    return hljs;
  })();
  return loading;
}

/** Colours the code blocks under `root` that name a language highlight.js knows. */
export async function highlightWithin(root: ParentNode): Promise<void> {
  const blocks = [...root.querySelectorAll<HTMLElement>('pre.code-block > code[data-lang]:not(.hljs)')];
  if (!blocks.length) return;
  const hljs = await load();
  for (const block of blocks) {
    const lang = block.dataset.lang || '';
    if (!hljs.getLanguage(lang)) continue;
    block.innerHTML = hljs.highlight(block.textContent || '', { language: lang, ignoreIllegals: true }).value;
    block.classList.add('hljs');
  }
}
