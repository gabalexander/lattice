# The symbol index

A wiki's prose names the code all the time: `Session::stop`, `src/db.rs`, `[sessions] stop_idle_after`,
`lattice build --model`. Each of those is a link to the file and the lines where it's defined, at the commit
the wiki was written from. Google's Code Wiki gets its links from Kythe, which hooks into a project's build;
lattice gets them from an index of its own, built from git, which needs nothing of the project but its files.
A build makes it, the writers are shown what each file defines from it, and every code span they write is
looked up in it. A span it can't tell apart is left unlinked: a wrong link is worse than none.

## Three tiers

Every language is read as well as it can be here, in three tiers, each reaching where the one above can't:

1. **Precise**: a [SCIP](https://github.com/scip-code/scip) indexer, the compiler's own view of every
   definition and every reference, for each language whose indexer is installed.
2. **Syntactic**: a tree-sitter grammar, compiled into lattice, reads every file for its definitions, each
   with its kind, its exact first and last lines and what it's in, so `Session::stop` is a name as well as
   `stop`. A language with no grammar compiled in is read by its keywords: first lines exact, last lines by
   indentation.
3. **Paths**: every file git tracks at the commit, and its directories.

| Language | Its tier |
| --- | --- |
| Rust | precise with rust-analyzer, else syntactic |
| Go | precise with scip-go, else syntactic |
| TypeScript, JavaScript | precise with scip-typescript, else syntactic (TSX's grammar reads both) |
| Python | precise with scip-python, else syntactic |
| Java | precise with scip-java, else syntactic; Kotlin and Scala precise with scip-java, else keywords |
| C, C++ | precise with scip-clang and the build's `compile_commands.json`, else syntactic |
| C#, Ruby, Swift, PHP, shell, Lua, Elixir, Dart, Zig, Perl, Groovy | keywords |
| anything else | paths |

Besides what each language defines, the grammars read the names a user types: a serde field's key (with its
`rename` and `rename_all`), a clap field's flags and a clap variant's subcommand, a Go field's struct tags,
cobra's commands (with the command each is added to, by `AddCommand` or podman's `Parent:`) and their flags
(named by a string or by a name given one), Python's `add_argument` and click's options, commander's commands
and options, and the name a constant holds when it's an environment variable's or a file's
(`const NO_DOWNLOAD: &str = "LATTICE_NO_DOWNLOAD"`).

## The indexers

| Indexer | Languages | Run on the outermost directories with | Install |
| --- | --- | --- | --- |
| `rust-analyzer scip` | Rust | `Cargo.toml` | `rustup component add rust-analyzer` |
| scip-go | Go | `go.mod` | `go install github.com/scip-code/scip-go/cmd/scip-go@latest` |
| scip-typescript | TypeScript, JavaScript | `tsconfig.json`, else `package.json` | `npm install -g @sourcegraph/scip-typescript` |
| scip-python | Python | `pyproject.toml`, `setup.py` or `setup.cfg` | `npm install -g @sourcegraph/scip-python` |
| scip-java | Java, Kotlin, Scala | `pom.xml`, `build.gradle(.kts)` or `build.sbt` | `cs install --contrib scip-java` |
| scip-clang | C, C++ | `compile_commands.json` at the top or in `build/` | its [releases](https://github.com/sourcegraph/scip-clang/releases) |

An indexer runs where the repository's files are at the wiki's commit: in the checkout itself when it's at
that commit (the files changed since are left to the grammars), or else in a copy of the commit that `git
archive` writes into the cache. It may build what the project's build scripts make, as an editor's would:
rust-analyzer leaves them in the checkout's `target/`. Each one is held to `[index] indexer_timeout_secs` and
`[index] indexer_memory_mb`, every process it starts counted, and stopped past either, or as soon as the
build it's for is cancelled. One that isn't installed, fails or is stopped leaves its languages to their
grammars, and `lattice index` says which and why, naming its log; a project of several that fails leaves its
own files to the grammars and keeps the others'.

lattice downloads no indexer. Each needs its language's toolchain to run at all, and whoever has that has the
indexer one command away, which `lattice index` prints; and an indexer runs the project's build scripts, which
lattice shouldn't do with a program it fetched itself.

## Kept between builds

What the grammars read is kept by each file's blob, and what an indexer said by a hash of its version and of
the blobs of every file it reads (its languages', its manifests' and the lock files'), under
`~/.cache/lattice/index/<name>-<hash>/`. A build reads only the blobs it doesn't have, and runs an indexer
again only once one of its files has changed: a change to the README runs nothing. Gone, it's read again.

## What a span links to

A span is read as each thing it could be, in turn, and the first that resolves wins:

| As | Like | Links to |
| --- | --- | --- |
| a path | `src/db.rs`, `src/index/`, `db.rs`, `src/db.rs:12`, `src/db.rs#L3-L9` | the file, its lines, or the directory |
| a command line | `lattice build`, `lattice doctor --model`, `doctor --model` | the subcommand, or the flag of it |
| a flag | `--model`, `-p` | the field or the call that defines it |
| a config key | `[sessions] stop_idle_after`, `sessions.stop_idle_after`, `[[profile]]` | the field that goes by it |
| a name | `Session`, `Session::stop()`, `fn stop`, `engine.Server`, `(*Server).Serve`, `out!` | the definition |
| a value | `LATTICE_NO_DOWNLOAD`, `$XDG_CACHE_HOME`, `wiki.json` | the constant that holds it |

A command line is followed from its program (from `Cargo.toml`, `package.json`, `pyproject.toml` or `cmd/`)
down through each subcommand, and a subcommand with nothing before it is a top-level one. A config key is a
field that goes by it, a serde field's key or a Go struct tag, each table followed through the field before it.

Several definitions a name could be are narrowed, a step at a time: to those of the kind the span says (`()`,
`!`, `struct`); for a capitalized name, to types; to those in the files the prose is about (a build's writer
gives its subsection's); to those that aren't tests', definitions over declarations, a method over a field of
its name in the same type, as a getter has; to the one those files refer to, when an indexer said what each
file refers to; and to a type in the file named after it, as `viewer.rs` is `Viewer`'s. What's still more than
one is left unlinked.

Some spans are left unlinked even when they name one thing, because prose most often means something else by
them: a bare word that's only a field or a variable somewhere, unless it's that field's key (a serde key, or a
Go tag written as code writes names); a Go name that's an English word and its package's own (`gid`); a
test's types and constants, unless the prose is about its file; a quoted word, which is a string; and, in a
language that writes `::`, a dotted name that isn't a member (`session.ended` is an event, not a function).

## Checking it

```sh
lattice index .                       # how each language was indexed, and why
lattice index . Session::stop src/db.rs '[sessions] stop_idle_after'
lattice index . --near src/config.rs -- new --model
lattice index . --commit v0.1.0 --json -- Index::build
```

prints each language's tier and the indexer's note, or what each span links to: its target, or what else it
could be. `--near` gives the files the spans are about, `--` comes before a span starting with `-`, and
`--json` prints the report and the lookups as JSON.

## What it costs

On a MacBook (Apple silicon, 12 cores), the release build, with nothing kept from before:

| Repository | Files | Syntactic | Its cache | Precise |
| --- | --- | --- | --- | --- |
| crystal (Rust) | 299, 183 of them Rust | 0.26 s, 87 MB, 15.7k definitions | 1.5 MB | rust-analyzer: 37 s cold (20 s with its build scripts built), 2.8 GB, 5.8 MB kept |
| podman (Go) | 10,130, 1,420 Go read (`vendor/` by paths only) | 0.25 s, 68 MB, 18.4k definitions | 1.8 MB | scip-go 0.2.7: 22 s, 681 MB, 1,045 of the Go files (it builds for the host's GOOS), 9.5 MB kept |
| dispatch (Go, TypeScript, Python) | 2,695 | 0.35 s, 92 MB, 38k definitions | 3.7 MB | scip-typescript and scip-python: 19 s, 477 MB, 13 MB kept |

Built again with the cache, the syntactic tier takes under 0.1 s, and an indexer whose files haven't changed
doesn't run.

The grammars are most of what the index adds to the binary. The release workflow's builds, before and after:

| Target | Without the index | With it | Its archive |
| --- | --- | --- | --- |
| macOS, Apple silicon | 5.0 MB | 15.5 MB | 2.2 MB, 3.9 MB |
| macOS, Intel | 5.1 MB | 15.7 MB | 2.3 MB, 4.0 MB |
| Linux x86_64, static (musl) | 5.7 MB | 16.9 MB | 2.5 MB, 4.4 MB |
| Linux ARM, static (musl) | 5.6 MB | 16.3 MB | 2.4 MB, 4.2 MB |

Measured one by one, Rust's grammar is 1.2 MB, Go's 0.3, Python's 0.5, TSX's (which reads TypeScript and
JavaScript) 1.5, Java's 0.5, C's 0.7 and C++'s 3.5; C#'s, Kotlin's, Swift's, Ruby's, PHP's and bash's together
would add 17 MB more, so those languages are read by their keywords.

## How well it links

Measured on every distinct inline code span in a repository's own docs (so with no files to prefer, harder
than a writer's subsection, which has them), and on crystal's hand-made wiki fixture, whose 128 code links were
each checked by hand to land on their symbol:

| Spans | Paths alone | Syntactic | Precise | A regex scan |
| --- | --- | --- | --- | --- |
| crystal's fixture, 128 linked by hand | 2 | 127, 123 to the same lines | 127, 123 | 122, 118 |
| crystal's docs, 2,041 | 11% | 35% | 35% | 28% |
| podman's docs, 1,225 | 1% | 13% | 13% | 4% |
| dispatch's docs, 9,023 | 11% | 23% | 23% | 17% |

Of the fixture's four that link elsewhere, three are commands, which link to their clap definition where the
fixture linked the module that runs them, and one a struct, which links to its definition where the fixture
linked its `impl`. Checked by hand, 49 of 50 links drawn at random from crystal's docs were right, 25 of 25 from
podman's, and 23 of 25 from dispatch's TypeScript and Python (the two wrong were a test's stand-ins, which
link no more). Most spans that don't link aren't the repository's at all: other programs' commands, values,
keys of other tools' files.

The precise tier links about as many spans as the grammars: for a name its value is in being certain, and in
telling apart several definitions by what the files near refer to. What a grammar can't see, an indexer adds:
a class defined in a function, a type's properties a grammar doesn't walk.
