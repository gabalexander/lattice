# The command line

| Command | What it does |
| --- | --- |
| `lattice open [repo]` | Opens a repository's wiki in the browser, adding the repository when it's new, or the home page with none. It starts a server in the background the first time and leaves it running; over ssh it prints the `ssh -L` line that reaches it from your machine. |
| `lattice serve [--port N]` | Serves the web app and its API on 127.0.0.1 in the foreground, on port 7347 or a free one, until ctrl+c, which cancels the jobs running first. One server runs per data directory. |
| `lattice serve --stop` | Stops the server that's running, in the background or not. |
| `lattice build <repo> [--model M]` | Writes a new version of a repository's wiki here, in the terminal, adding the repository when it's new. ctrl+c stops it, and the page's Resume carries on from where it got. |
| `lattice sync <repo>` | Writes again what changed since the wiki's latest version: a new version. |
| `lattice status [repo]` | Each repository with its versions, whether its code has moved on since the latest, and its latest job. |
| `lattice export <repo> <dir> [--version N]` | Writes a version of the wiki, the latest by default, as a static site that works from `file://` and on GitHub Pages. |
| `lattice index <dir> [span...]` | Indexes the checkout `<dir>` is in as a build does, and says how each of its languages was indexed (by a SCIP indexer, its grammar or its keywords) and why, or what each code span given links to; `--near`, `--commit` and `--json` as [index.md](index.md) says. |
| `lattice doctor [--model M]` | Checks the settings, the data directory, git and Claude Code (see [claude.md](claude.md)). |

A repository is named by its key (`go`), by its path (`.`, `~/code/app`), by a git URL, or by GitHub's
`owner/repo`. A local repository is read where it is. A git one is cloned into lattice's data directory with your own
git, so with your SSH keys and credential helpers, and with `gh` for GitHub when it's installed and logged in. git
is never left waiting for a password: a clone that needs one fails, saying so, in the job's log.

A build and a sync each need the repository to themselves. One started while another runs, from the
command line or from the server, is refused, saying which.
