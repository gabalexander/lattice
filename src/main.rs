//! lattice's command line: `lattice serve`, `open`, `export`, `build`,
//! `sync`, `status`, `index` and `doctor`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use lattice::cancel::Cancel;
use lattice::config::{self, Config};
use lattice::db::Db;
use lattice::index::{Index, Lookup};
use lattice::{claude, outln, output, paths, printable, shell};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};

/// A code wiki for your own repositories, written by Claude Code and kept
/// up to date.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the wikis and the web app on 127.0.0.1 until it's stopped.
    Serve {
        /// The port [default: 7347, or a free one when that's taken]
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Open a repository's wiki in the browser, or the home page, starting
    /// a server in the background when none is running.
    Open {
        /// The repository: a path, a git URL or owner/repo on GitHub, or
        /// its key [default: the home page]
        repo: Option<String>,
    },
    /// Write a repository's wiki as a static site, for file:// or GitHub
    /// Pages.
    Export {
        /// The repository: a path, a git URL or owner/repo, or its key.
        repo: String,
        /// The directory to write it in.
        dir: PathBuf,
    },
    /// Write a repository's wiki: a new version.
    Build {
        /// The repository: a path, a git URL or owner/repo, or its key.
        repo: String,
        /// The model that writes it [default: the settings' `model`]
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Write again what changed in a repository since its wiki's latest
    /// version.
    Sync {
        /// The repository: a path, a git URL or owner/repo, or its key.
        repo: String,
    },
    /// Say how a repository's wiki stands, or every one's.
    Status {
        /// The repository [default: all of them]
        repo: Option<String>,
    },
    /// Index a repository's code as a build does, and say how each of its
    /// languages was indexed (by a SCIP indexer, its grammar or its
    /// keywords) and why; or, given code spans, what each links to.
    Index {
        /// The repository: a directory in a git checkout.
        repo: PathBuf,
        /// Code spans to look up, as a wiki writes them: `Session::stop`,
        /// `src/db.rs`, `[index] precise`; after `--` for one starting with
        /// `-`.
        spans: Vec<String>,
        /// A file the spans are about, preferred where a span could name
        /// several things.
        #[arg(long, value_name = "PATH")]
        near: Vec<String>,
        /// The commit to index [default: the checkout's HEAD]
        #[arg(long, value_name = "REV")]
        commit: Option<String>,
        /// Print the report and the lookups as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Check that lattice can do its work here: its settings, its data,
    /// git, and Claude Code, which it asks to read a file (a few cents).
    Doctor {
        /// The model to check [default: the settings' `model`]
        #[arg(short, long)]
        model: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return match err.use_stderr() {
                true => ExitCode::FAILURE,
                false => ExitCode::SUCCESS,
            };
        }
    };
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        // Whatever read what it printed had what it wanted.
        Err(err) if output::closed(&err) => ExitCode::SUCCESS,
        Err(err) => {
            // It may quote what git or Claude said; and standard error may
            // be a pipe that has gone too.
            let said = format!("{err:#}");
            let _ = writeln!(std::io::stderr(), "lattice: {}", printable::text(&said));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Serve { .. } | Command::Open { .. } | Command::Export { .. } => {
            bail!("not yet: the server comes in lattice's next release")
        }
        Command::Build { .. } | Command::Sync { .. } | Command::Status { .. } => {
            bail!("not yet: the generator comes in lattice's next release")
        }
        Command::Index {
            repo,
            spans,
            near,
            commit,
            json,
        } => index(&repo, &spans, &near, commit.as_deref(), json),
        Command::Doctor { model } => doctor(model),
    }
}

/// `lattice index`: the checkout `repo` is in indexed at `commit`, with its
/// cache, and either how each language was indexed or what each of `spans`
/// links to.
fn index(
    repo: &Path,
    spans: &[String],
    near: &[String],
    commit: Option<&str>,
    json: bool,
) -> Result<()> {
    let config = Config::load()?;
    let root = git_line(repo, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("{} isn't in a git checkout", repo.display()))?;
    let root = PathBuf::from(root);
    let rev = format!("{}^{{commit}}", commit.unwrap_or("HEAD"));
    let commit = git_line(&root, &["rev-parse", "--verify", "--end-of-options", &rev])
        .with_context(|| format!("there's no commit {} to index", commit.unwrap_or("HEAD")))?;
    let cache = paths::index_dir(&root);
    let index = Index::build(&root, &commit, &cache, &config.index_settings())?;
    let looked: Vec<(&String, Lookup)> = (spans.iter())
        .map(|span| (span, index.lookup(span, near)))
        .collect();
    if json {
        let lookups: Vec<serde_json::Value> = (looked.iter())
            .map(|(span, found)| {
                let (found, defs) = match found {
                    Lookup::Unique(def) => ("unique", vec![def]),
                    Lookup::Ambiguous(defs) => ("ambiguous", defs.iter().collect()),
                    Lookup::Missing => ("missing", Vec::new()),
                };
                serde_json::json!({"span": span, "found": found, "defs": defs})
            })
            .collect();
        let report = serde_json::json!({
            "commit": commit,
            "files": index.files(),
            "definitions": index.definitions(),
            "seconds": index.took().as_secs_f64(),
            "languages": index.report(),
            "lookups": lookups,
        });
        return outln!("{}", serde_json::to_string_pretty(&report)?);
    }
    if looked.is_empty() {
        for line in index.summary() {
            outln!("{}", printable::line(&line))?;
        }
    }
    for (span, found) in &looked {
        let line = match found {
            Lookup::Unique(def) => {
                format!("{span} → {} ({} {})", def.target(), def.kind, def.qualified)
            }
            Lookup::Ambiguous(defs) => {
                let some: Vec<String> = defs.iter().take(5).map(|def| def.target()).collect();
                let more = match defs.len() > 5 {
                    true => format!(", and {} more", defs.len() - 5),
                    false => String::new(),
                };
                format!("{span}: could be {}{more}", some.join(", "))
            }
            Lookup::Missing => format!("{span}: nothing here"),
        };
        outln!("{}", printable::line(&line))?;
    }
    Ok(())
}

/// What `git -C dir args` says on its first line, or why it failed.
fn git_line(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Process::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("couldn't run git: is it installed and on the PATH?")?;
    let said = String::from_utf8_lossy(&out.stdout);
    let line = said.lines().next().unwrap_or_default().trim().to_string();
    if !out.status.success() || line.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("git {}: {}", args[0], err.trim());
    }
    Ok(line)
}

/// `lattice doctor`: each check on a line of its own, and an error at the
/// end when one failed.
fn doctor(model: Option<String>) -> Result<()> {
    let mut failed = 0;
    let mut say = |what: &str, checked: Result<String>| -> Result<()> {
        match checked {
            Ok(said) => outln!("ok    {what}: {said}"),
            Err(err) => {
                failed += 1;
                let err = format!("{err:#}");
                outln!("FAIL  {what}: {}", printable::line(&err))
            }
        }
    };
    let config = Config::load();
    let file = shell::home_relative(&paths::config_file());
    let read = match &config {
        Ok(_) if paths::config_file().is_file() => Ok(file),
        Ok(_) => Ok(format!("{file} isn't there: the defaults")),
        Err(err) => Err(anyhow::anyhow!("{err:#}")),
    };
    say("settings", read)?;
    let data = Db::open().map(|_| shell::home_relative(&paths::data_dir()));
    say("data", data)?;
    say("git", version_of("git"))?;
    say("claude", version_of(claude::PROGRAM))?;
    let model = match model {
        Some(model) => model,
        None => config.map(|config| config.model).unwrap_or_default(),
    };
    let read = config::check_model(&model).and_then(|()| {
        let answer = claude::preflight(&model, &Cancel::new())?;
        let model = answer.model.unwrap_or(model);
        Ok(format!("{model} read a file (${:.3})", answer.cost_usd))
    });
    say("reading", read)?;
    if failed > 0 {
        bail!("{failed} of the checks failed");
    }
    Ok(())
}

/// What `program --version` says, on one line.
fn version_of(program: &str) -> Result<String> {
    let out = Process::new(program)
        .arg("--version")
        .output()
        .with_context(|| format!("couldn't run {program}: is it installed and on the PATH?"))?;
    let said = String::from_utf8_lossy(&out.stdout);
    let said = printable::line(said.trim()).into_owned();
    if !out.status.success() || said.is_empty() {
        bail!("{program} --version failed ({})", out.status);
    }
    Ok(said)
}
