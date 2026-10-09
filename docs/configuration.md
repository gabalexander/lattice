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
```

| Setting | Default | What it does |
| --- | --- | --- |
| `model` | `sonnet` | The model that writes a wiki, as `claude --model` takes it: `sonnet`, `opus`, or a full name like `claude-opus-5-5`. A build can be given another. |
| `concurrency` | `4` | How many subsections a build writes at once. |
| `budget_usd` | `30.0` | The most one build may spend, by Claude Code's own count. A build that reaches it stops, keeping what it wrote, for a resume to carry on from. |
| `ask_model` | `sonnet` | The model that answers questions in the chat. |
| `ask_budget_usd` | `0.5` | The most one question may spend. |
| `exclude` | `[]` | Globs of files no wiki reads or links to, beyond what `.gitignore` leaves out. |

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
| `~/.cache/lattice/downloads/` | mermaid, which draws the diagrams, downloaded once at a pinned version and checked against its SHA-256 (`$XDG_CACHE_HOME`) |

A repository's key is its page's address: its name, `go` for `golang/go`, or its owner's and its name when
another has taken that, `acme-go`.
