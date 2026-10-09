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
  never to run lattice. `node web/mock/server.mjs` stands in for lattice's API while you work on it, and
  `--app build` serves the built app as lattice does, for screenshots and `web/scripts/perf.mjs`
  (`docs/web.md`).
- Install: `make install`: the web app and a release build, into `~/.local/bin`
- The eval (`docs/evals.md`), which spends through Claude Code: `cargo run --release --example eval -- build
  <repo> <label>`, then `score <label>`, `pairwise <a> <b>` and `report <label>...`
- The container: `docker build -t lattice:dev .`, then `docker/smoke.sh lattice:dev` tries it
  (`docs/docker.md`)

Run lint, format and tests before every commit. CI (`.github/workflows/ci.yml`) runs them on macOS and Ubuntu,
builds with the oldest Rust `Cargo.toml` promises, and runs the web app's lint, check, tests and build.

## Conventions

- lattice is a library (`src/lib.rs`, a module a part) and a binary (`src/main.rs`, the command line) that
  uses it, so the tests can drive both.
- Everything that runs Claude goes through `src/claude.rs`, locked down as `docs/claude.md` says. A new kind
  of run is a new `Ask`, never another way to start `claude`.
- A job runs through `jobs::run` whoever starts it, the server or the command line, and writes a wiki
  through the `Generator` that `generator::current()` gives.
- The API is `docs/server.md`'s: a change to it is a change there, and to the web app's `web/src/lib/api.ts`.
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

- `src/main.rs`: the command line (clap): `serve` (and `--stop`), `open`, `export`, `build` and `sync`, which
  run a job here through `jobs::run`, `status`, `index`, which indexes a checkout and says how each language
  was and what spans link to, and `doctor`, which checks the settings, the data, git and Claude Code, asking it
  to read a file
- `src/lib.rs`: the modules, for the binary and the tests
- `build.rs`: the web app's files under `web/build/`, when it's built, carried into the binary as
  `web::FILES`
- `src/claude.rs`: running Claude Code: an `Ask` (model, appended system prompt, message on standard input,
  read-only tools or none, a JSON Schema, turns, budget, a conversation forgotten, kept or resumed, text
  streamed, a time limit, retries) run locked down with stream-json, what it does told as `Event`s, its
  `Answer` or why it `Failed` with what it cost; tries again what failed for a passing reason; and
  `preflight`, the check that Claude can read a file with lattice's flags; adapted from crystal and
  deepwiki-by-cc
- `src/server.rs`: `lattice serve` and `open`: the API (`docs/server.md`) and the web app on 127.0.0.1, a
  thread a connection; a route for each address, the Host and Origin checks before it; a job's events
  followed from its feed, or from what's kept of it; one server a data directory (`serve.lock`), where it is
  in `serve.json`; `open` starting one in the background once, stopping one of another lattice, and saying
  the `ssh -L` line over ssh; adapted from crystal's `wiki_server.rs`
- `src/http.rs`: the HTTP/1.1 the server speaks: a request read with caps on its head and body, replies with
  the headers every answer carries, server-sent events, and the checks that keep other sites out (Host,
  Origin, `Sec-Fetch-Site`)
- `src/web.rs`: the web app as the binary carries it: its files by path, its page for every address of its
  own, or one saying it wasn't built; and the export bundle under `export/`
- `src/jobs.rs`: running a job, the same from the server's queue and the command line (`run`): the
  repository's build lock, `work/` and its `build.log`, the code prepared, the generator run, `work/` made
  the next version; the server's queue (`Jobs`): one job a repository, three repositories at once, cancel,
  and each job's `Feed` that its pages follow; what a job may be asked for (`submit`), refused with an HTTP
  status
- `src/generator.rs`: the `Generator` a job runs (`JobSpec` in, `Report`s along the way, `Built` out), and
  `current()`, the one this lattice has: `gen::Claude`
- `src/gen/`: the generator (`docs/generation.md`), a module named `r#gen`, `gen` being a word Rust keeps;
  adapted from crystal's wiki generator and deepwiki-by-cc
  - `mod.rs`: `Claude`, the `Generator` that writes with Claude Code
  - `build.rs`: a build's phases (plan, write, link, overview), a sync from a version and a resume of a
    build that stopped, the writers `concurrency` at once, the caps on each run of Claude and on the build,
    what's written kept in `build.json` as it goes
  - `plan.rs`: the outline: the planner's prompt and schema, its answer read and put in order (paths it
    doesn't have dropped, overshoot trimmed, every source file covered), a subsection's kind, the entry
    points
  - `write.rs`: what each writer is told (subsection, section hub, overview, sync, fix) and the shape of its
    answer; the files a writer starts from, with what they define and the commits that touched them
  - `check.rs`: links into the code and citations checked against the commit, moved where they belong or
    dropped, paths and credentials taken out; and the linker, a code span to the one thing it names
  - `diagram.rs`: a diagram made what mermaid draws and checked with `crate::mermaid`, and how many a text
    may draw, near-duplicates out; `fences.rs`: fences by CommonMark's rules, as the web app reads them
  - `validate.rs`: whether an answer is a text at all (a failure reported, a refusal, a tool failure written
    into the page) and its tidying
  - `files.rs`: the files at the commit, less what nobody reads (vendored, built, lock files, binaries,
    credentials; generated and minified marked), which are source, and the `exclude` globs; what they
    define, and where, is the symbol index's (`src/index/`)
  - `prose.rs`: links and code spans in markdown with where they are; `repo.rs`: git (a clean tree of the
    commit, logs, diffs, where a line moved to, the forge's addresses); `book.rs`: `build.json`
- `src/wiki.rs`: `wiki.json`, version 1 of the contract between the generator, the server and the web app,
  and `CodeLink`, a `code:` link read
- `src/repos.rs`: the repositories: added by what the user typed and found again by it, where their code is,
  cloned (with `gh` for GitHub when it's there) or fetched with the user's git and never a password prompt,
  at the remote's default branch, where a branch is now, and removed with their files
- `src/ask.rs`: the chat: a question checked, Claude told the wiki's outline and the section being read,
  run read-only in the repository, its answer streamed as `delta`, `tool`, `done` and `error` events, and
  stopped when the page goes; adapted from crystal's `wiki_ask.rs`
- `src/editor.rs`: a file of a repository opened in the user's editor at a line (`$VISUAL`, `$EDITOR`), only
  one with a window of its own, and only a file in the repository
- `src/export.rs`: `lattice export`: a version of a wiki as a static site, the web app's bundle with the
  wiki inlined in its page and mermaid beside its script
- `src/signals.rs`: ctrl+c and SIGTERM caught, so a build or a server stops its `claude` runs before it
  exits
- `src/cancel.rs`: `Cancel`, how a job or a chat is asked to stop, shared by its clones, and a wait that ends
  when it is
- `src/config.rs`: the settings in `config.toml`: model, concurrency, budget, the chat's model and budget,
  excludes, and `[index]`; read, checked, a model's name never read as an option
- `src/db.rs`: the SQLite database (WAL, migrations by `user_version`): repositories under their keys, the
  versions of each one's wiki, and the jobs that build them, queued, running and over, with their progress;
  a job left running by a server that stopped is failed as it starts
- `src/download.rs`: mermaid, downloaded once at a pinned version with curl, checked against its SHA-256 and
  kept in the cache
- `src/glob.rs`: globs as `.gitignore` writes them, for `exclude` and `[index] paths_only`
- `src/index/`: the symbol index the wiki's `code:` links come from (`docs/index.md`): `Index::build` reads a
  checkout at a commit in three tiers, `lookup` resolves a code span to the one definition it names or says it
  can't tell, `defined_at` and `outline` serve the writers; adapted from crystal's `src/wiki/index/`
  - `mod.rs`: `Def`, `DefKind`, `Lookup`, `IndexSettings`, `Index`, each language's tier and why
  - `files.rs`: the files at the commit from `git ls-tree`, their blobs from one `git cat-file --batch`, a
    file's language, its module or package, the programs a manifest installs
  - `grammar.rs` (and `grammar/tests.rs`): the syntactic tier, tree-sitter's Rust, Go, Python, TypeScript and
    JavaScript, Java, C and C++, walked by rules of each, with serde keys, clap and cobra commands and flags,
    struct tags and the names constants hold
  - `keywords.rs`: the languages with no grammar compiled in, read by their keywords
  - `precise.rs`: the precise tier, the SCIP indexers installed here, held to a time and a memory, in the
    checkout or a copy of the commit, what each said kept by its files' blobs
  - `scip.rs`: a reader of SCIP's protobuf, a document at a time, and its symbols named as lattice names them
  - `cache.rs`: what the grammars read, kept by blob in `syntax.json`, parsed on a few threads
  - `lookup.rs`: a span read as a path, a command line, a flag, a config key, a name or a value, and several
    definitions narrowed to the one meant, or left unlinked
  - `tests.rs`: the lookup rules on repositories held in memory
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
- `web/`: the web app (SvelteKit, a single-page app built to `web/build/`; `docs/web.md`)
  - `src/routes/`: its pages: `+page.svelte` the wikis and the box a wiki starts from, `jobs/[id=n]/` a job,
    `[key=key]/+layout.svelte` a repo's wiki at `/<key>` and `/<key>/v/<n>` with its versions and jobs,
    `settings/`; `src/params/` the keys and numbers they take
  - `src/lib/api.ts`: the HTTP API, a function a call; `types.ts`: what it and wiki.json say
  - `src/lib/markdown.ts`: the wiki's markdown, rendered small and strict; `fences.ts`: fences by CommonMark's
    rules, shared with the generator's check; `codelinks.ts`: where a link into the code goes
  - `src/lib/mermaid.ts`: mermaid, loaded and its diagrams drawn as they come near; `mermaid-sanitize.ts`: the
    repairs made to a diagram first; `highlight.ts`: code blocks coloured by highlight.js, loaded when needed
  - `src/lib/jobs.svelte.ts`: a job followed through its event stream; `sse.ts`: event streams read from a
    response; `theme.svelte.ts`, `toast.svelte.ts`, `format.ts`: the theme, the toast, how things are said
  - `src/lib/components/`: the app's parts: the header, the logo, icons, the theme menu, the source box, a
    wiki's card, a job's progress, a confirmation
  - `src/lib/wiki/`: a wiki's page, as Code Wiki lays it out: `WikiView.svelte` the page, `Outline`, `Prose`,
    `DiagramCard`, `ZoomDialog`, `Chat`, `FindBox`, `HelpDialog`, `VersionMenu`, and `wiki.css`
  - `src/export/`: the page alone, for `lattice export`, built by `vite.export.config.ts` to `build/export/`
  - `static/`: the fonts with their licences, the theme set before the first paint, the icon
  - `mock/`: a stand-in for lattice's API with its fixtures, and the synthetic wiki; `scripts/perf.mjs`: the
    page measured in headless Chrome
- `tests/cli.rs`: the binary and the runner end to end, with a fake `claude`: `doctor`, `build` and `status`,
  `serve` one a data directory, `open` starting one in the background; and the index on each fixture
  repository, with a fake `rust-analyzer`
- `tests/index/`: a small repository a language, each with the spans that must link (`spans.tsv`), and a
  `.scip` that rust-analyzer wrote of the Rust one
- `tests/server.rs`: the API end to end, against a server of the test's own, with a fake generator, a fake
  `claude` and a fake editor
- `tests/gen.rs`: the generator end to end, with a fake `claude` answering from fixtures: a build, a resume,
  a budget reached, a sync, and a build and a sync through the job API
- `tests/release.rs`: the release's archives named alike everywhere
- `tests/mermaid/`: diagrams the mermaid tests read
- `evals/`: the eval (`docs/evals.md`): `config.json` the golden repositories pinned to commits, their core
  files and topics; `questions/` their golden questions; `eval.rs` the runner, `cargo run --example eval`
- `docs/`: a page for each part: `cli.md`, `server.md`, `configuration.md`, `claude.md`, `web.md`,
  `index.md`, `generation.md`, `evals.md`; `eval-results/` the evals that decided something
- `install.sh`, `packaging/homebrew/`, `.github/workflows/`: installing and releasing
- `Dockerfile`, `docker-compose.yml`, `docker/`: the container: lattice, git and Claude Code on Alpine, built in
  stages, with its entrypoint (ssh set up for private repositories), `git-credential-env` (a
  token from the environment, over HTTPS alone), and `smoke.sh`, which CI's `docker.yml` runs on the image
