# lattice

A code wiki for your own repositories: it reads a repository with Claude Code, writes one page about it (an
outline, sections and subsections with a mermaid diagram each, every name in the code linked to the line it's
defined on), keeps it up to date as the code changes, and answers questions about the code in a chat. All of it
runs on the user's machine and uses only their Claude subscription, through Claude Code (`claude -p`): never add
or require another AI provider or a hosted service. README.md says what it is and how to start; `docs/` has a
page for each part of it.

## Commands

- Build: `make build` (`cargo build`)
- Test: `make test` (`cargo test`, and the web app's tests once `web/` is there)
- Lint: `make lint` (`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`, and the web app's lint
  and type check)
- Format: `cargo fmt`
- The web app, in `web/` (SvelteKit, built to static files in `web/build/`, which the release binary carries):
  `npm --prefix web ci`, then `npm --prefix web run dev` to work on it, `run check` (svelte-check), `test`
  (vitest) and `run build`. `make web` builds it; a release build runs it first. Node is needed to build it,
  never to run lattice.
- Install: `make install`: the web app and a release build, into `~/.local/bin`

Run lint, format and tests before every commit. CI (`.github/workflows/ci.yml`) runs them on macOS and Ubuntu,
builds with the oldest Rust `Cargo.toml` promises, and runs the web app's lint, check, tests and build.

## Conventions

- lattice is a library (`src/lib.rs`, a module a part) and a binary (`src/main.rs`, the command line) that
  uses it, so the tests can drive both.
- Everything that runs Claude goes through `src/claude.rs`, locked down as `docs/claude.md` says. A new kind
  of run is a new `Ask`, never another way to start `claude`.
- What a command prints on standard output goes through `out!` and `outln!` (`src/output.rs`) with a `?`,
  never `print!` and `println!`, which `clippy.toml` refuses: a reader gone, like `head -1`'s, stops the
  command and lattice exits 0, where `println!` panics. What it says on standard error goes through `err!`
  and `errln!`, never `eprint!` and `eprintln!`.
- Text lattice didn't write (what Claude, git or a remote said) goes through `printable` before it reaches a
  terminal, and through `secrets::redact` before it's kept where others read it.
- A setting is a field of `Config` (`src/config.rs`), checked as it's read, with its default, and a row in
  `docs/configuration.md`.
- The database's tables change only by a new step at the end of `db::MIGRATIONS`, never by editing one that
  shipped.
- Prose doc comments on every module and item that isn't obvious, saying what it is and why; tests beside the
  code they test, and end to end in `tests/`. A test never reads the user's real data nor reaches the network:
  it sets `HOME` and the `XDG_*` directories to a temporary directory of its own, puts a fake `claude`
  speaking stream-json first on the `PATH` (see `tests/cli.rs`), and sets `LATTICE_NO_DOWNLOAD`.
- Code adapted from another project says so at the top of its module, and a project whose code or text is
  adapted closely is in THIRD_PARTY_NOTICES.md with its copyright line and license.
- Commits are Conventional Commits, one line under 72 characters, with no body and no trailer.

## Releasing

Releases are built by `.github/workflows/release.yml`, for macOS (Apple silicon and Intel) and Linux (x86_64
and ARM, static with musl), and installed by `install.sh`.

1. Set the new version in `Cargo.toml`, run `cargo build` so `Cargo.lock` follows, and commit both:
   `chore: release 0.2.0`.
2. Optionally try the build first: `gh workflow run release.yml`, then check the run builds every target.
3. Tag the commit with the same version and push the tag: `git tag v0.2.0 && git push origin v0.2.0`. The
   workflow checks the tag against `Cargo.toml`, builds the web app and each target, and publishes the GitHub
   release with the archives and their checksums.

`install.sh` and the Homebrew formula (`packaging/homebrew/lattice.rb`) find a release's archive and checksum by
the names the workflow gives them: change one, change all three (`tests/release.rs` checks they agree). For a
tap, `packaging/homebrew/formula.sh 0.2.0` prints the formula with that release's version and checksums filled
in.

## Layout

- `src/main.rs`: the command line (clap): `serve`, `open`, `export`, `build`, `sync`, `status`, and `doctor`,
  which checks the settings, the data, git and Claude Code, asking it to read a file
- `src/lib.rs`: the modules, for the binary and the tests
- `src/claude.rs`: running Claude Code: an `Ask` (model, appended system prompt, message on standard input,
  read-only tools or none, a JSON Schema, turns, budget, a conversation forgotten, kept or resumed, text
  streamed, a time limit, retries) run locked down with stream-json, what it does told as `Event`s, its
  `Answer` or why it `Failed` with what it cost; tries again what failed for a passing reason; and
  `preflight`, the check that Claude can read a file with lattice's flags; adapted from crystal and
  deepwiki-by-cc
- `src/cancel.rs`: `Cancel`, how a job or a chat is asked to stop, shared by its clones, and a wait that ends
  when it is
- `src/config.rs`: the settings in `config.toml`: model, concurrency, budget, the chat's model and budget,
  excludes; read, checked, a model's name never read as an option
- `src/db.rs`: the SQLite database (WAL, migrations by `user_version`): repositories under their keys, the
  versions of each one's wiki, and the jobs that build them, queued, running and over, with their progress;
  a job left running by a server that stopped is failed as it starts
- `src/download.rs`: mermaid, downloaded once at a pinned version with curl, checked against its SHA-256 and
  kept in the cache
- `src/mermaid/`: reading mermaid diagrams, to check one a model wrote: the five kinds a model explains code
  with read into a graph or a sequence, anything else refused with a reason, and a cap on how big one may be
  - `graph.rs`: the boxes, edges and frames every kind but the sequence is read into
  - `flowchart.rs`, `state.rs`, `class.rs`, `er.rs`, `sequence.rs`: each kind's parser
  - `tests.rs`: every diagram under `tests/mermaid/` reads as its kind, and garbage never panics
- `src/paths.rs`: where things are kept, by the XDG directories: the settings, the data (the database, and
  each repository's checkout, the build going on and its versions' files), the cache
- `src/source.rs`: where a repository's code is, as the user gives it (a path, a git URL, or GitHub's
  `owner/repo`), checked (no password in a URL, no scheme git would run a command for), its name, and the key
  it's kept under, never one of the web app's own words
- `src/time.rs`: `Timestamp`, seconds since the epoch, written and read as RFC 3339
- `src/output.rs`: `out!`, `outln!`, `err!` and `errln!`, and standard output closing taken as the reader
  having had enough
- `src/printable.rs`: text from elsewhere made safe for a terminal: control and bidi characters taken out
- `src/secrets.rs`: credentials taken out of text before it's kept or shown
- `src/shell.rs`: paths written with `~`, and arguments quoted, the way a shell reads them
- `src/links.rs`: a link opened in the browser, or copied over ssh
- `src/clipboard.rs`: text put on the clipboard, by the system's program or OSC 52 over ssh
- `tests/cli.rs`: the binary and the runner end to end, with a fake `claude`
- `tests/release.rs`: the release's archives named alike everywhere
- `tests/mermaid/`: diagrams the mermaid tests read
- `docs/`: a page for each part: `configuration.md`, `claude.md`
- `install.sh`, `packaging/homebrew/`, `.github/workflows/`: installing and releasing
