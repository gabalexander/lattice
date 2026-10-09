# Configuration

lattice reads its settings from `~/.config/lattice/config.toml` (`$XDG_CONFIG_HOME/lattice/config.toml` when
that's set). The file is optional, and so is every setting in it; one left out has its default. A setting
lattice doesn't know, or a value that makes no sense, is an error that names it, rather than something
quietly skipped: `lattice doctor` says which.

```toml
model = "sonnet"          # the model that plans and writes a wiki
concurrency = 4           # how many of a build's writers write at once, 1 to 16
budget_usd = 30.0         # the most one build may spend, in US dollars
ask_model = "sonnet"      # the model that answers the chat
ask_budget_usd = 0.5      # the most one question may spend
exclude = ["vendor/**"]   # files left out of every wiki, as .gitignore writes them

[index]                   # the symbol index the wiki's code links come from
precise = true            # run the SCIP indexers installed here
indexer_timeout_secs = 900
indexer_memory_mb = 8192
max_file_kb = 1024
paths_only = ["vendor/", "third_party/", "node_modules/", "testdata/"]
```

| Setting | Default | What it does |
| --- | --- | --- |
| `model` | `sonnet` | The model that writes a wiki, as `claude --model` takes it: `sonnet`, `opus`, or a full name like `claude-opus-5-5`. A build can be given another. |
| `concurrency` | `4` | How many subsections a build writes at once. |
| `budget_usd` | `30.0` | The most one build may spend, by Claude Code's own count. A build that reaches it stops, keeping what it wrote, for a resume to carry on from. |
| `ask_model` | `sonnet` | The model that answers questions in the chat. |
| `ask_budget_usd` | `0.5` | The most one question may spend. |
| `exclude` | `[]` | Globs of files no wiki reads or links to, beyond what `.gitignore` leaves out. |
| `[index] precise` | `true` | Whether the SCIP indexers installed here (rust-analyzer, scip-go, scip-typescript, scip-python, scip-java, scip-clang) run for the [symbol index](index.md); without them every language is read by its grammar. |
| `[index] indexer_timeout_secs` | `900` | The most one indexer may run, in seconds, before it's stopped and its languages left to their grammars. |
| `[index] indexer_memory_mb` | `8192` | The most memory one indexer may take, every process under it counted, in megabytes, before it's stopped. |
| `[index] max_file_kb` | `1024` | Files bigger than this aren't read for definitions, most often generated or minified; their paths still link. |
| `[index] paths_only` | `["vendor/", "third_party/", "node_modules/", "testdata/"]` | Globs, as `.gitignore` writes them, of the files that link by their paths only, none of their definitions read: code a project carries but didn't write. |

The budgets are in dollars because that's how Claude Code counts what a run uses; on a subscription, they
cap how much of it one build or one question takes.

## Where lattice keeps things

| Where | What |
| --- | --- |
| `~/.config/lattice/config.toml` | The settings (`$XDG_CONFIG_HOME`) |
| `~/.local/share/lattice/lattice.db` | The repositories, their wikis' versions and the jobs that build them, in SQLite (`$XDG_DATA_HOME`) |
| `~/.local/share/lattice/repos/<key>/checkout/` | The clone of a git repository; a local one is read where it is |
| `~/.local/share/lattice/repos/<key>/v<n>/` | Version `n` of its wiki: `wiki.json`, `build.json` (the build's bookkeeping) and `build.log` |
| `~/.local/share/lattice/repos/<key>/work/` | The build going on, or the one that was cut short, which a resume carries on |
| `~/.local/share/lattice/repos/<key>/tree/` | A clean clone of the commit a build reads, for a local repository with changes not committed (see [generation.md](generation.md)) |
| `~/.cache/lattice/downloads/` | mermaid, which draws the diagrams, downloaded once at a pinned version and checked against its SHA-256 (`$XDG_CACHE_HOME`) |
| `~/.cache/lattice/index/<name>-<hash>/` | What the [symbol index](index.md) read of a checkout, by each file's blob, and what its indexers said, so the next build reads only what changed; it's read again when it's gone |

A repository's key is its page's address: its name, `go` for `golang/go`, or its owner's and its name when
another has taken that, `acme-go`.
