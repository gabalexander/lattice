// A large synthetic wiki.json, for checking the page's performance: the mock serves it as the repo `atlas`,
// and `node mock/synth.mjs [sections] [subsections] > big.json` prints one.
//
// Its text is made up, but shaped like a real wiki's: each section and subsection with a diagram (flowcharts
// top to bottom and left to right, a sequence diagram now and then) and paragraphs, lists, a table or a code
// block, with about thirty links into the code each, as Code Wiki's pages have. Carried over from crystal's
// wiki page (assets/wiki/dev/synth.py).

const WORDS = `session daemon socket worker queue buffer screen viewer listener request response event bus plugin hook
task flow step runner parser reader writer cache index store entry table record frame packet stream channel handle
state config profile layout pane tab sidebar theme key command client server process child signal timer watcher
loader linker symbol module package compiler optimizer scheduler allocator`.split(/\s+/);
const VERBS = 'reads writes owns starts stops hands parses keeps sends checks builds loads draws tells waits lays'.split(' ');
const DIRS = ['src', 'src/tui', 'src/daemon', 'src/mermaid', 'src/forge', 'src/api', 'tests', 'docs'];

/** A small seeded generator (mulberry32), so the same seed makes the same wiki. */
function rng(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    random: next,
    int: (lo, hi) => lo + Math.floor(next() * (hi - lo + 1)),
    pick: (list) => list[Math.floor(next() * list.length)],
    sample: (list, n) => [...list].sort(() => next() - 0.5).slice(0, n),
  };
}

export function synthesize({ sections: sectionCount = 16, subsections: subCount = 90, seed = 7 } = {}) {
  const r = rng(seed);
  const words = (n) => Array.from({ length: n }, () => r.pick(WORDS)).join(' ');
  const cap = (s) => s[0].toUpperCase() + s.slice(1);
  const path = () => `${r.pick(DIRS)}/${r.pick(WORDS)}_${r.pick(WORDS)}.rs`;
  const label = () => `${cap(r.pick(WORDS))} ${cap(r.pick(WORDS))}`;
  const codeLink = () => {
    const name = r.pick([`${cap(r.pick(WORDS))}${cap(r.pick(WORDS))}`, `${r.pick(WORDS)}_${r.pick(WORDS)}()`, path(), `--${r.pick(WORDS)}`]);
    const line = r.int(1, 2000);
    const end = r.random() < 0.4 ? `-L${line + r.int(1, 40)}` : '';
    return `[\`${name}\`](code:${path()}#L${line}${end})`;
  };
  const sentence = (links) => {
    const parts = [cap(words(r.int(3, 8)))];
    for (let i = 0; i < links; i++) parts.push(`${r.pick(VERBS)} ${codeLink()}`, words(r.int(2, 7)));
    return `${parts.join(' ')}.`;
  };
  const paragraph = (sentences = 4, links = 2) => Array.from({ length: sentences }, () => sentence(r.int(1, links))).join(' ');
  const flowchart = (nodes) => {
    const lines = [`flowchart ${r.pick(['TD', 'TD', 'LR'])}`];
    const ids = Array.from({ length: nodes }, (_, i) => `n${i}`);
    for (const id of ids) lines.push(`  ${id}["${label()}<br/>(${path()})"]`);
    for (let i = 1; i < nodes; i++) lines.push(`  ${ids[r.int(0, i - 1)]} ${r.random() < 0.25 ? '-.->' : '-->'}|${r.pick(VERBS)} ${r.pick(WORDS)}| ${ids[i]}`);
    if (nodes > 3 && r.random() < 0.5) lines.push(`  ${ids[nodes - 1]} -->|${r.pick(VERBS)}| ${ids[1]}`);
    return lines.join('\n');
  };
  const sequence = () => {
    const actors = Array.from({ length: r.int(3, 4) }, () => label().replace(' ', ''));
    const lines = ['sequenceDiagram', ...actors.map((a) => `  participant ${a}`)];
    for (let i = r.int(4, 7); i > 0; i--) {
      const [a, b] = r.sample(actors, 2);
      lines.push(`  ${a}${r.pick(['->>', '-->>'])}${b}: ${r.pick(VERBS)} ${r.pick(WORDS)}`);
    }
    return lines.join('\n');
  };
  const diagram = () => ({ mermaid: r.random() < 0.15 ? sequence() : flowchart(r.int(4, 8)), caption: `How the ${words(2)} fits together` });
  const body = () => {
    const blocks = [paragraph(4, 4), paragraph(3, 3)];
    blocks.push(Array.from({ length: r.int(3, 5) }, () => `- **${label()}**: ${sentence(1)}`).join('\n'));
    if (r.random() < 0.3) blocks.push(`| Name | What it does |\n|---|---|\n${Array.from({ length: 4 }, () => `| ${codeLink()} | ${words(6)} |`).join('\n')}`);
    if (r.random() < 0.3) blocks.push(`\`\`\`rust\nfn ${r.pick(WORDS)}(x: u32) -> String {\n    // ${words(5)}\n    format!("{x}")\n}\n\`\`\``);
    blocks.push(paragraph(4, 3));
    blocks.push(`Sources: ${Array.from({ length: 3 }, () => codeLink()).join(', ')}`);
    return blocks.join('\n\n');
  };
  const seen = new Set();
  const slug = (text) => {
    const base = text.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
    let out = base;
    for (let n = 2; seen.has(out); n++) out = `${base}-${n}`;
    seen.add(out);
    return out;
  };
  const sections = Array.from({ length: sectionCount }, (_, s) => {
    const count = Math.floor(subCount / sectionCount) + (s < subCount % sectionCount ? 1 : 0);
    const title = `${label()} and ${label()}`;
    const subsections = Array.from({ length: count }, () => {
      const subTitle = `The ${label()}: ${words(3)}`;
      return { id: slug(subTitle), title: subTitle, body_md: body(), diagram: diagram(), files: Array.from({ length: r.int(1, 4) }, path) };
    });
    return { id: slug(title), title, summary_md: paragraph(5, 2), diagram: diagram(), subsections };
  });
  const links = sections
    .slice(0, 6)
    .map((s) => `[${s.title}](#${s.id})`)
    .join(', ');
  return {
    version: 1,
    repo: {
      name: 'example/atlas',
      root: '/tmp/atlas',
      commit: '0123456789abcdef0123456789abcdef01234567',
      branch: 'main',
      web_url: 'https://github.com/example/atlas',
      code_url: 'https://github.com/example/atlas/blob/{commit}/{path}',
    },
    generated: { at: '2026-10-08T09:30:00Z', by: 'Claude Sonnet 5.5', model: 'claude-sonnet-5-5', cost_usd: 12.5 },
    overview: { summary_md: `${paragraph(5, 2)}\n\n${paragraph(4, 2)}\n\nRead on: ${links}.`, diagram: diagram() },
    sections,
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [sections, subsections] = process.argv.slice(2).map(Number);
  process.stdout.write(JSON.stringify(synthesize({ sections: sections || 16, subsections: subsections || 90 }), null, 1));
}
