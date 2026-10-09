// The markdown renderer: what it lets through, how links into the code come out, and CommonMark's blocks and
// inlines as a wiki uses them. Carried over from crystal's wiki page (assets/wiki/test/markdown.test.js).

import { describe, expect, test } from 'vitest';
import { parseCodeTarget, plainText, renderInlineMarkdown, renderMarkdown, type MarkdownOptions } from './markdown';

const served: MarkdownOptions['code'] = (p, line, end) => ({
  href: `https://example.com/blob/abc/${p}${line ? `#L${line}${end ? `-L${end}` : ''}` : ''}`,
  title: `Open ${p}`,
});
const render = (src: string, opts: MarkdownOptions = {}) => renderMarkdown(src, { code: served, ...opts });

describe('safety', () => {
  test('text is escaped, raw HTML included', () => {
    expect(render('a <script>alert(1)</script> & b')).toBe('<p>a &lt;script&gt;alert(1)&lt;/script&gt; &amp; b</p>');
    expect(render('<img src=x onerror=alert(1)>')).toBe('<p>&lt;img src=x onerror=alert(1)&gt;</p>');
    expect(render('line<br>next')).toBe('<p>line<br>next</p>');
  });

  test('unsafe schemes are dropped, the text kept', () => {
    expect(render('[x](javascript:alert(1))')).toBe('<p>x</p>');
    expect(render('[x](data:text/html,hi)')).toBe('<p>x</p>');
    expect(render('[x](vbscript:msgbox)')).toBe('<p>x</p>');
    expect(render('<javascript:alert(1)>')).toBe('<p>&lt;javascript:alert(1)&gt;</p>');
    expect(render('[x](//evil.example/a)')).toBe('<p>x</p>');
  });

  test('attributes can not be broken out of', () => {
    expect(render('[x](https://a.b/"onmouseover="alert(1) "t\\"itle")')).not.toMatch(/"onmouseover=/);
    expect(render('[x](https://a.b "say \\"hi\\"")')).toMatch(/title="say &quot;hi&quot;"/);
    expect(render('[x](code:src/"a.rs#L1)')).toMatch(/data-path="src\/&quot;a.rs"/);
  });

  test('an image is a link to it, never loaded', () => {
    expect(render('![chart](https://x/y.png)')).toBe('<p><a href="https://x/y.png" target="_blank" rel="noopener noreferrer">chart</a></p>');
  });
});

describe('links into the code', () => {
  test('carry their path and lines', () => {
    const html = render('See [`Session`](code:src/session.rs#L10-L20).');
    expect(html).toMatch(
      /<a class="code-link" data-path="src\/session.rs" data-line="10" data-end="20" href="https:\/\/example.com\/blob\/abc\/src\/session.rs#L10-L20"/,
    );
    expect(html).toMatch(/><code>Session<\/code><\/a>/);
    expect(render('[x](code:src/a.rs#L7)')).toMatch(/data-line="7" href="[^"]*#L7"/);
    expect(render('[x](code:src/a.rs)')).toMatch(/data-path="src\/a.rs" href="[^"]*src\/a.rs"/);
  });

  test('with nowhere to go are their label alone', () => {
    expect(renderMarkdown('[`x`](code:src/a.rs#L1)', { code: () => null })).toBe('<p><code>x</code></p>');
    expect(renderMarkdown('[`x`](code:src/a.rs#L1)', {})).toBe('<p><code>x</code></p>');
  });

  test('that only the editor opens are focusable links', () => {
    const html = renderMarkdown('[y](code:src/a.rs#L3)', { code: () => ({ href: null }) });
    expect(html).toBe('<p><a class="code-link" data-path="src/a.rs" data-line="3" role="link" tabindex="0">y</a></p>');
  });

  test('a relative link is a path in the repository', () => {
    expect(render('[readme](docs/guide.md)')).toMatch(/class="code-link" data-path="docs\/guide.md"/);
  });

  test('a citation of lines is a pill', () => {
    expect(render('[worker.ts:61-118](code:src/worker.ts#L61-L118)')).toMatch(/^<p><a class="code-link citation"/);
    expect(render('[`worker.ts:61`](code:src/worker.ts#L61)')).toMatch(/class="code-link citation"/);
    expect(render('[the worker](code:src/worker.ts#L61)')).toMatch(/class="code-link" /);
  });

  test('targets read into a path and lines', () => {
    expect(parseCodeTarget('src/x.rs#L10-L20')).toEqual({ path: 'src/x.rs', line: 10, end: 20 });
    expect(parseCodeTarget('src/x.rs#L10-20')).toEqual({ path: 'src/x.rs', line: 10, end: 20 });
    expect(parseCodeTarget('./src/x.rs#L3')).toEqual({ path: 'src/x.rs', line: 3, end: null });
    expect(parseCodeTarget('src/a%20b.rs')).toEqual({ path: 'src/a b.rs', line: null, end: null });
    expect(parseCodeTarget('src/x.rs#intro')).toEqual({ path: 'src/x.rs', line: null, end: null });
    expect(parseCodeTarget('src/%E0%A4%A.rs')).toEqual({ path: 'src/%E0%A4%A.rs', line: null, end: null });
  });
});

describe('other links', () => {
  test('anchors, web links and references', () => {
    expect(render('[Daemon](#the-daemon)')).toBe('<p><a href="#the-daemon">Daemon</a></p>');
    expect(render('<https://ex.com>')).toBe('<p><a href="https://ex.com" target="_blank" rel="noopener noreferrer">https://ex.com</a></p>');
    expect(render('go to https://ex.com/a.')).toBe('<p>go to <a href="https://ex.com/a" target="_blank" rel="noopener noreferrer">https://ex.com/a</a>.</p>');
    expect(render('[r][1]\n\n[1]: https://x.y "T"')).toMatch(/<a href="https:\/\/x.y" target="_blank" rel="noopener noreferrer" title="T">r<\/a>/);
  });

  test('a reference definition inside a fence is code, not a definition', () => {
    expect(render('```\n[a]: https://x.y\n```\n\n[a]')).toBe('<pre class="code-block"><code>[a]: https://x.y</code></pre><p>[a]</p>');
  });
});

describe('inlines', () => {
  test('emphasis', () => {
    expect(render('*a* **b** ***c*** ~~d~~')).toBe('<p><em>a</em> <strong>b</strong> <em><strong>c</strong></em> <del>d</del></p>');
    expect(render('snake_case_name and _em_')).toBe('<p>snake_case_name and <em>em</em></p>');
    expect(render('\\*literal\\* 2 * 3 * 4')).toBe('<p>*literal* 2 * 3 * 4</p>');
    expect(render('**bold `code` here**')).toBe('<p><strong>bold <code>code</code> here</strong></p>');
  });

  test('code spans', () => {
    expect(render('`a <b>`')).toBe('<p><code>a &lt;b&gt;</code></p>');
    expect(render('`` a`b ``')).toBe('<p><code>a`b</code></p>');
    expect(render('`open')).toBe('<p>`open</p>');
  });

  test('breaks and entities', () => {
    expect(render('one  \ntwo\\\nthree')).toBe('<p>one<br>two<br>three</p>');
    expect(render('&lt;b&gt; &amp; &copy; &#65; &bogus;')).toBe('<p>&lt;b&gt; &amp; © A &amp;bogus;</p>');
  });

  test('a line alone, as the chat shows a file it read', () => {
    expect(renderInlineMarkdown('[`src/a.rs`](code:src/a.rs)', { code: served })).toMatch(/^<a class="code-link" data-path="src\/a.rs"/);
  });
});

describe('blocks', () => {
  test("headings, shifted under the page's own", () => {
    expect(render('# One\n## Two')).toBe('<h1>One</h1><h2>Two</h2>');
    expect(render('# One\n###### Six', { headingOffset: 3 })).toBe('<h4>One</h4><h6>Six</h6>');
    expect(render('Title\n===\n\ntext')).toBe('<h1>Title</h1><p>text</p>');
  });

  test('lists', () => {
    expect(render('- a\n- b\n  - c\n- d')).toBe('<ul><li>a</li><li>b\n<ul><li>c</li></ul></li><li>d</li></ul>');
    expect(render('1. a\n2. b')).toBe('<ol><li>a</li><li>b</li></ol>');
    expect(render('3. c\n4. d')).toBe('<ol start="3"><li>c</li><li>d</li></ol>');
    expect(render('- a\n\n- b')).toBe('<ul><li><p>a</p></li><li><p>b</p></li></ul>');
    expect(render('- [x] done\n- [ ] not')).toBe(
      '<ul><li class="task"><input type="checkbox" disabled checked> done</li><li class="task"><input type="checkbox" disabled> not</li></ul>',
    );
    expect(render('text\n- item')).toBe('<p>text</p><ul><li>item</li></ul>');
  });

  test('tables', () => {
    expect(render('| Name | What |\n|:--|--:|\n| `a|b` | **x** |')).toBe(
      '<div class="table-wrap"><table><thead><tr><th style="text-align:left">Name</th><th style="text-align:right">What</th></tr></thead>' +
        '<tbody><tr><td style="text-align:left"><code>a|b</code></td><td style="text-align:right"><strong>x</strong></td></tr></tbody></table></div>',
    );
    expect(render('a | b\n--- | ---\n1 | 2')).toBe('<div class="table-wrap"><table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table></div>');
  });

  test('fenced code is escaped and marked with its language, for highlight.js to colour', () => {
    expect(render('```rust\nfn x() -> &str { "<b>" }\n```')).toBe(
      '<pre class="code-block"><code class="language-rust" data-lang="rust">fn x() -&gt; &amp;str { &quot;&lt;b&gt;&quot; }</code></pre>',
    );
    expect(render('```\n<plain>\n```')).toBe('<pre class="code-block"><code>&lt;plain&gt;</code></pre>');
    expect(render('~~~sh title="x"\necho 1\n~~~')).toBe('<pre class="code-block"><code class="language-sh" data-lang="sh">echo 1</code></pre>');
  });

  test('a fence closes only on a run as long as its own, so a quoted fence stays inside', () => {
    expect(render('````md\n```mermaid\nflowchart TD\n```\n````')).toBe(
      '<pre class="code-block"><code class="language-md" data-lang="md">```mermaid\nflowchart TD\n```</code></pre>',
    );
  });

  test('quotes and rules', () => {
    expect(render('> quoted\n> on')).toBe('<blockquote><p>quoted\non</p></blockquote>');
    expect(render('a\n\n---\n\nb')).toBe('<p>a</p><hr><p>b</p>');
  });

  test("a Sources line is the part's quiet footer", () => {
    expect(render('Sources: [a.rs:1-9](code:a.rs#L1-L9)')).toMatch(/^<p class="sources">Sources: <a class="code-link citation"/);
    expect(render('**Sources:** a.rs')).toMatch(/^<p class="sources">/);
    expect(render('The sources: a.rs')).toMatch(/^<p>/);
  });
});

test('plain text, for search', () => {
  expect(plainText('See [`x`](code:a.rs#L1) and **bold**.\n\n```\ncode\n```')).toBe('See x and bold .');
});
