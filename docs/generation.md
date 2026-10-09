# How a wiki is written

A wiki is one page about a repository at a commit: an overview, then sections, each a part of what the system
does, each with subsections about one component, mechanism or flow. Every part has a diagram, and every name
the prose gives in backticks is a link to the line the code defines it on. lattice writes it with Claude Code,
on your machine, as a job: `lattice build`, `sync` and the web app's Generate, Sync, Resume and Regenerate all
run one (see [server.md](server.md)). This page says how the generator, `src/gen/`, goes about it, what it costs,
and what it sends where.

## A build

A build reads the repository at the job's commit (a repository on this machine that has uncommitted changes, or
isn't at the commit, is read from a clean clone of the commit of its own, `repos/<key>/tree/`), lists its files
and indexes what they define, and then goes through four phases, which the job's page shows as they go.

1. **Plan.** One Claude reads the README, the file tree and as much of the code as it needs, and plans the
   outline: sections named for what the system does, ordered from the entry points in to the core and out to
   configuration, building and testing; each section's subsections, each with what its writer is to explain
   (and what it leaves to its siblings), its kind, and the files its writer starts from. A repository is
   planned by its size: one of a few thousand lines of source gets a section or two, one of 200,000 lines six
   to sixteen sections of three to nine subsections, about one subsection for every 3,500 lines. What the
   planner answers is put in order before it's kept: paths the repository doesn't have are dropped, a
   subsection's files are capped at 25, sections and subsections past what the repository's size allows are
   trimmed, and source files no subsection covers go to the subsection nearest them by path. An outline that
   leaves out more than 15% of the source, or is less than half the size it should be, is sent back once.
2. **Write.** One writer a subsection, `concurrency` of them at once. A writer starts from its subsection's
   files, with what they define and on which lines, and the commits that touched them lately, and explores
   from there with Read, Grep and Glob: it follows calls to their definitions, greps for callers, reads the
   configuration and the defaults, and says only what it read. It opens with what a reader of its kind of part
   looks for first: a numbered list of the steps of a pipeline, a table of a data model's fields, a
   Parameter | Default | Effect table for settings, a map of an API's symbols. It links each name into the
   code, cites the lines behind each claim that isn't obvious, `[retry.go:52-58](code:internal/retry.go#L52-L58)`,
   and ends with a Sources line. A writer that can't read the code says so in its answer, never in the page.
3. **Link.** Every subsection is checked once more against the page as it turned out (a link to a part that
   couldn't be written goes), and every code span is linked.
4. **Overview.** Each section's summary, a hub: what the section does, a map of its components, a line on
   each subsection, and how they work together. Then the overview, from the sections: what the repository is,
   its architecture, how a run goes through it, and a link to every section.

What's written is kept in the job's `build.json` as it's written. The wiki is written last, `wiki.json`
(the contract in [server.md](server.md) and `src/wiki.rs`), with the build's bookkeeping beside it: the
outline, and the hash of every file each subsection covers, which a sync reads.

## What's checked before it's kept

Each text a writer gives back is checked before it's kept, and what can't be put right here is sent back to the
writer once, with what's wrong; what's still wrong then is dropped. A wrong link is worse than none.

- **Links into the code.** A `code:` link must point at a file the commit has, at lines it has. When its label
  names a symbol, the symbol must be on or near those lines; when it isn't, the link moves to where the symbol
  is defined, or to the one line of the file that has it, and when that can't be told, it's a problem. A
  citation's label must name the file it points into, and is made to say the lines it points at.
- **Links on the page.** A `#id` must be a part of the page.
- **Diagrams.** A diagram must read with lattice's own mermaid reader once its styles are dropped, the slips
  models make fixed (`->>>`, a `<placeholder>`, spaces inside a shape) and a flowchart's labels quoted: the
  subset the web app's mermaid draws. A subsection or a section may draw one diagram in its text besides its
  card, the overview three; one that says what another on the same text says is dropped.
- **A text at all.** An empty answer, a failure the writer reported, or a text that is really a refusal or a
  report of its tools failing has the writer run again, once.
- **Where it was made.** Paths into the repository on this machine, or into your home, are taken out, and
  anything that looks like a credential is redacted.

Then the linker makes each code span that names one thing the repository defines a link to it: a type, a
function, a method (`Session::stop`), a field, a flag (`--model`), a config key (`[sessions] stop_idle_after`),
a command (`lattice build`), a file or a directory. A name several things have, with none of them nearer the
text than the others, stays unlinked. The linker, the link checks and each writer's list of what its files
define all read the symbol index ([index.md](index.md)): SCIP where an indexer is installed, tree-sitter
everywhere else.

## Sync, resume and regenerate

- **Sync** writes a new version from the latest one, after the code moved. It writes again only the subsections
  whose files changed (by their hashes), and those the last version couldn't write; each writer is given its
  text before, the commits since and the diff of its files, reads the code as it is now, and rewrites the text
  or says nothing needs changing. Summaries and the overview are written again only where a writer says what
  its part does changed. When more than half the subsections changed, or more than twelve new source files
  have no subsection, the outline is planned again, keeping every subsection whose files are as they were. The
  links of what isn't written again move with the lines they point at.
- **Resume** carries on a build or a sync that stopped, cancelled, out of budget or failed, from what its
  `build.json` kept: nothing written is written again.
- **Regenerate**, like a first build, writes a new version from the start, a new outline and all; the
  versions before stay.

A build fails, for a resume to finish, when more than a fifth of its subsections couldn't be written; with
fewer, the version is made without them, the log says which, and the next sync tries them again.

## Caps

Each run of Claude has its own caps, and the build has the settings' budget and a time limit: past either, it
starts no more runs and stops, keeping what it wrote, for a resume to carry on.

| Run | Spend | Turns | Time |
| --- | --- | --- | --- |
| The planner | $3.00 | 60 | 20 min |
| A subsection's writer | $1.20 | 40 | 15 min |
| A writer sent back to put its text right (or the planner, its outline) | $0.60 | 16 | 8 min |
| A section's summary | $0.80 | 16 | 10 min |
| The overview | $1.20 | 20 | 15 min |
| The whole build | `budget_usd`, none by default | | 4 hours |

A writer that fails, or gives back no text, is run once more with half again its turns and spend. A run that
fails for a reason that passes, Claude overloaded or rate limited, is tried again twice, waiting longer each
time ([claude.md](claude.md)). Before the first run that spends, a build checks Claude Code can read a file with
the flags it's given, for a cent or two.

## Cost and time

Measured with Sonnet, four writers at once, on a Mac, in October 2026:

| Repository | Source | Outline | Cost | Time |
| --- | --- | --- | --- | --- |
| go-pubsub (Go) | 1,509 lines in 10 files | 3 sections, 4 subsections, 9 diagrams, 619 links | $1.07 | 2 min |
| crystal (Rust) | 185,255 lines in 201 files | 13 sections, 61 subsections, 81 diagrams, 6,508 links | $16.93 | 16 min |
| docket (Rust) | 174,674 lines in 271 files | 14 sections, 55 subsections, 70 diagrams, 5,575 links | $21.59 | 19 min |
| crystal, a sync after a commit that changed one file | | 1 subsection written again | $0.17 | 34 s |

A subsection's writer costs $0.10 to $0.60, about $0.30 on average; the planner $0.10 to $0.40; a section's
summary and the overview $0.08 to $0.25 each. A build has no budget by default: a repository of about 200,000
lines costs about $30 with Sonnet, and Opus costs more a run. Set `budget_usd` to cap it: a build that
reaches its budget stops, keeping what it wrote, and a resume carries on with a budget of its own.

## What goes where

- The code is read by Claude through Claude Code, under your login and your subscription: what a writer reads
  with its tools, and what lattice puts in its message (the file tree, the README, the outline, the commits
  that touched its files, a diff for a sync), goes to Anthropic as any Claude Code session's does. Nothing
  goes anywhere else: lattice calls no other model, provider or service.
- Each run is locked down ([claude.md](claude.md)): it reads the repository's tree and nothing outside it, runs
  no command, fetches nothing from the web, and reads none of your Claude Code settings, permissions or hooks.
- Files that hold credentials by their name (`.env`, `credentials.json`, `id_rsa` and their like), lock files,
  binaries, vendored and built directories, and what the `exclude` setting names, are never listed for a
  writer; a writer could still Read a tracked file it finds, so keep secrets out of the repository, as ever.
- What's kept, the wiki, its build's log and bookkeeping, has credentials redacted, and stays in lattice's data
  directory ([configuration.md](configuration.md)) until you export it.

The eval that measures how good the wikis are, and the experiments that decided the prompts, are in
[evals.md](evals.md).
