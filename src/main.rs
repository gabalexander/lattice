//! lattice's command line: `lattice serve`, `open`, `export`, `build`,
//! `sync`, `status`, `index` and `doctor`. docs/cli.md says what each does.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use lattice::cancel::Cancel;
use lattice::config::{self, Config};
use lattice::db::{Db, Ended, JobKind, Phase, Progress, Repo};
use lattice::generator::{self, Report};
use lattice::index::{Index, Lookup};
use lattice::jobs::{self, BuildLock};
use lattice::source::Source;
use lattice::{claude, outln, output, paths, printable, repos, server, shell, signals};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::sync::Mutex;

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
        /// The address to listen on: 127.0.0.1, but in a container, whose
        /// port is published to its host's 127.0.0.1 alone (docs/docker.md).
        #[arg(long, default_value = "127.0.0.1")]
        listen: std::net::Ipv4Addr,
        /// Stop the server that's running instead.
        #[arg(long, conflicts_with = "port")]
        stop: bool,
        /// Run as the server `lattice open` starts in the background.
        #[arg(long, hide = true)]
        helper: bool,
    },
    /// Open a repository's wiki in the browser, adding the repository when
    /// it's new, or the home page, starting a server in the background when
    /// none is running.
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
        /// The version to write [default: the latest]
        #[arg(long)]
        version: Option<u32>,
    },
    /// Write a repository's wiki, adding the repository when it's new: a
    /// new version.
    Build {
        /// The repository: a path, a git URL or owner/repo, or its key.
        repo: String,
        /// The model that writes it [default: the settings' `model`]
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Write again what changed in a repository since its wiki's latest
    /// version: a new version.
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
        Command::Serve { stop: true, .. } => server::stop(),
        Command::Serve {
            listen,
            port,
            helper,
            ..
        } => server::serve(listen, port, helper),
        Command::Open { repo } => server::open(repo),
        Command::Export { repo, dir, version } => export(&repo, &dir, version),
        Command::Build { repo, model } => build(&repo, JobKind::Build, model),
        Command::Sync { repo } => build(&repo, JobKind::Sync, None),
        Command::Status { repo } => status(repo.as_deref()),
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

/// The directory a repository typed on the command line is from.
fn cwd() -> Result<PathBuf> {
    std::env::current_dir().context("couldn't tell the current directory")
}

/// The repository `typed` names, which lattice must know already.
fn known(db: &Db, typed: &str) -> Result<Repo> {
    match repos::find(db, typed, &cwd()?)? {
        Some(repo) => Ok(repo),
        None => bail!(
            "{} isn't one of lattice's repositories: `lattice build {}` adds it",
            printable::line(typed),
            shell::quote(typed)
        ),
    }
}

/// `lattice export`.
fn export(typed: &str, dir: &std::path::Path, version: Option<u32>) -> Result<()> {
    let db = Db::open()?;
    let repo = known(&db, typed)?;
    let page = lattice::export::export(&db, &repo, version, dir)?;
    outln!(
        "wrote {}'s wiki to {}: open {}",
        repo.name,
        dir.display(),
        page.display()
    )
}

/// `lattice build` and `lattice sync`: a job of `kind` on the repository
/// `typed` names, added when it's new, run here, what it does said as it
/// does it; ctrl+c stops it.
fn build(typed: &str, kind: JobKind, model: Option<String>) -> Result<()> {
    let db = Mutex::new(Db::open()?);
    let repo = {
        let mut db = db.lock().unwrap_or_else(|err| err.into_inner());
        match repos::find(&db, typed, &cwd()?)? {
            Some(repo) => repo,
            None => repos::add(&mut db, typed, &cwd()?)?,
        }
    };
    // A job a lattice that was killed left running holds nothing now.
    db.lock()
        .unwrap_or_else(|err| err.into_inner())
        .interrupt_running(jobs::building)?;
    let Some(lock) = BuildLock::take(&repo.key)? else {
        bail!(
            "{} is being built already: `lattice status {}` says how far it got",
            repo.name,
            repo.key
        );
    };
    let job = {
        let db = db.lock().unwrap_or_else(|err| err.into_inner());
        jobs::submit(&db, &repo.key, kind, model.as_deref(), None)?
    };
    let cancel = Cancel::new();
    signals::cancel_on_stop(cancel.clone());
    let generator = generator::current();
    let mut last: Option<Progress> = None;
    let ended = jobs::run(
        &db,
        job.id,
        lock,
        generator.as_ref(),
        &cancel,
        &mut |report| {
            // Said along the way of work that must finish: a reader gone
            // doesn't stop it.
            let _ = match report {
                Report::Log(line) => outln!("{}", printable::line(&line)),
                Report::Progress(progress) => {
                    let same = last.as_ref().is_some_and(|last| {
                        (last.phase, last.done, last.total, &last.current)
                            == (
                                progress.phase,
                                progress.done,
                                progress.total,
                                &progress.current,
                            )
                    });
                    let said = (!same).then(|| progress_line(&progress));
                    last = Some(progress);
                    match said {
                        Some(said) => outln!("{said}"),
                        None => Ok(()),
                    }
                }
            };
        },
    )?;
    match ended {
        Ended::Done(n) => outln!(
            "wrote version {n} of {}'s wiki: `lattice open {}` shows it",
            repo.name,
            repo.key
        ),
        Ended::Failed(why) => bail!("{why}"),
        Ended::Cancelled => bail!("cancelled: `lattice build` or the page's Resume carries on"),
    }
}

/// A build's progress, on a line.
fn progress_line(progress: &Progress) -> String {
    let phase = match progress.phase {
        Phase::Plan => "planning",
        Phase::Write => "writing",
        Phase::Link => "linking",
        Phase::Overview => "the overview",
    };
    let current = match &progress.current {
        Some(current) => format!(": {}", printable::line(current)),
        None => String::new(),
    };
    format!(
        "{phase} {} of {}{current} (${:.2})",
        progress.done, progress.total, progress.cost_usd
    )
}

/// `lattice status`: each repository, or the one `typed` names, with its
/// versions and its latest job.
fn status(typed: Option<&str>) -> Result<()> {
    let db = Db::open()?;
    let listed = match typed {
        Some(typed) => vec![known(&db, typed)?],
        None => db.repos()?,
    };
    if listed.is_empty() {
        outln!(
            "no repositories yet: `lattice build <path, git URL or owner/repo>` writes the first \
             wiki, or `lattice open` the home page"
        )?;
    }
    for repo in listed {
        let source = match &repo.source {
            Source::Local { path } => shell::home_relative(path),
            Source::Git { url } => url.clone(),
        };
        outln!("{}  {}  {source}", repo.key, printable::line(&repo.name))?;
        let versions = db.versions(&repo.key)?;
        for version in &versions {
            let commit: String = version.commit.chars().take(10).collect();
            let branch = version.branch.as_deref().unwrap_or("-");
            outln!(
                "  v{}  {commit}  {branch}  {}  {}  ${:.2}",
                version.n,
                version.model,
                version.at,
                version.cost_usd
            )?;
        }
        if let Some(latest) = versions.last() {
            let head = repos::head(&repo, latest.branch.as_deref());
            if head.as_ref().is_some_and(|head| *head != latest.commit) {
                outln!(
                    "  the code has moved on since v{}: `lattice sync {}` writes it again",
                    latest.n,
                    repo.key
                )?;
            }
        }
        if let Some(job) = db.jobs(&repo.key)?.first() {
            let mut line = format!(
                "  job {}: {} {}",
                job.id,
                jobs::word(job.kind),
                word(&job.state)
            );
            if let Some(progress) = job.progress.as_ref().filter(|_| !job.state.is_over()) {
                line.push_str(&format!(", {}", progress_line(progress)));
            }
            if let Some(error) = &job.error {
                line.push_str(&format!(": {}", printable::line(error)));
            }
            outln!("{line}")?;
        }
    }
    Ok(())
}

/// How an enum is said, as the API says it.
fn word<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => word,
        _ => String::new(),
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
