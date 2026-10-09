# The eval

How good a wiki lattice writes is, in numbers, so a change to the generator that "should make wikis better" is
shown to, or not. It runs lattice's own build on golden repositories pinned to commits, then scores what it
wrote. Every model it asks, to answer and to judge, goes through Claude Code like the rest of lattice, so it
spends from your subscription too: a build of a large repository costs what any build of it does
([generation.md](generation.md)), and scoring one about a dollar.

## Running it

```sh
cargo build --release
cargo run --release --example eval -- build crystal crystal-a --from ~/code/crystal   # or from its URL
cargo run --release --example eval -- score crystal-a
cargo run --release --example eval -- pairwise crystal-a crystal-b                   # by topic
cargo run --release --example eval -- pairwise crystal-a crystal-b --same            # same outline, part by part
cargo run --release --example eval -- report crystal-a crystal-b
```

`build` clones the golden repository at its pinned commit into `evals/.data/<label>/`, runs `lattice build` on
it (`--lattice` names another binary, the default `target/release/lattice`) with a data directory of its own
there, and keeps the wiki, its `build.json` and its log in `evals/results/<label>/`. Both directories are left
out of git: they can be made again.

## The golden repositories

`evals/config.json` pins each to a commit, and gives the files a good wiki of it must discuss (`coreFiles`) and
the topics two wikis of it are compared on (`pairwiseTopics`, each with the words its page would use).
`evals/questions/<repo>.json` holds its 20 golden questions, each with an answer key checked against the code at
the pin, and the lines it rests on: 12 tagged `floor`, what any decent wiki answers (what a part does, where it
lives, how it's configured), and 8 tagged `depth`, behaviour only close reading shows (a default, a limit, a
retry, an edge case, an ordering). When a pin moves, every answer is checked again at the new commit.

## What's measured

- **Deterministic.** Links into the code that land (a file the commit has, at lines it has); of the links whose
  label names a symbol, those whose lines have it; diagrams that read with lattice's mermaid reader; core files
  the wiki names; and its size: sections, subsections, words, diagrams, links.
- **Questions.** A model answers every question from the wiki alone, told to be exhaustive and to say when the
  wiki doesn't answer, and a second call grades each answer against its key: correct, partial or incorrect.
  The score is (correct + half the partial) over all, overall and by tag. The judge reads the wiki's text with
  each link into the code written as its label and file, and each diagram as its caption; a wiki of more than
  300,000 characters of that is answered from in parts, whole sections each, and a question's answer is what
  every part that could answer it said.
- **Pairwise.** Two wikis of one repository compared part by part: by topic, each wiki's subsection where the
  topic's words come up most; or with `--same`, for two runs of one outline, each subsection the two wrote
  differently. A judge picks the part that better explains how the system works (behaviour, grounding,
  connections, clarity, not length), twice, the order swapped; when the two disagree it's a tie, which cancels
  the judge's leaning to one side.

A single run is noisy: one question's verdict moves with the judge. Trust a difference that shows in more than
one measure, and run a close one again.

## Results

What decided something is in [eval-results/](eval-results/), dated.

- [2026-10-09: the generator's prompts](eval-results/2026-10-09-prompts.md): the writer's prompts adapted from
  deepwiki-by-cc against crystal's first ones, on crystal; and the score of lattice's build of crystal and of
  docket.

Adapted from deepwiki-by-cc's eval harness (MIT; see [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)).
