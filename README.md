# lattice

A wiki for your codebase, written by Claude and kept up to date: one page per repository with an outline,
a diagram for every part, code names linked to the exact line they're defined on, and a chat that reads the
code to answer. It runs on your own machine and uses only your Claude subscription, through Claude Code.

> [!NOTE]
> lattice is young: the server, the web app and the generator are landing now.

## Install

lattice needs [Claude Code](https://docs.anthropic.com/en/docs/claude-code), logged in, and git.

```sh
curl -fsSL https://raw.githubusercontent.com/gabalexander/lattice/main/install.sh | sh
```

Or from source, with Rust 1.88 or later (and Node 22 for the web app): `make install`.

## Quick start

```sh
lattice doctor            # check Claude Code, git and the settings
lattice open              # the web app: give it a path, a git URL or owner/repo, and Generate
lattice build golang/go   # or write a wiki from the command line
lattice open .            # this repository's wiki
lattice sync .            # write again what changed since its last version
```

## Documentation

| Page | What's in it |
| --- | --- |
| [docs/cli.md](docs/cli.md) | The commands, and how a repository is named |
| [docs/web.md](docs/web.md) | The web app: its pages, what it asks of the server, and how to build it |
| [docs/server.md](docs/server.md) | The server, its API and its jobs, and how it keeps other sites out |
| [docs/configuration.md](docs/configuration.md) | The settings, and where lattice keeps its data |
| [docs/docker.md](docs/docker.md) | lattice in Docker: the image, Claude Code's login, private repositories |
| [docs/claude.md](docs/claude.md) | How lattice runs Claude Code: read-only, locked down, with a budget |
| [docs/index.md](docs/index.md) | The symbol index the code links come from: SCIP indexers, tree-sitter, paths |

## License

MIT. lattice adapts code from [crystal](https://github.com/gabalexander/crystal) and
[deepwiki-by-cc](https://github.com/andyhtran/deepwiki-by-cc): see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
