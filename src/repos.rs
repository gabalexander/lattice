//! The repositories lattice was given, and their code: a local one read
//! where it is, a git one cloned into lattice's data directory with the
//! user's own git (and so their SSH keys and credential helpers), or, for
//! GitHub, with `gh` when it's installed and logged in, which reaches
//! private repositories with its own login. A build fetches first, and
//! reads the remote's default branch as it is.
//!
//! git never asks for a password here: there's no terminal to ask in, and
//! a server waiting on one would wait forever. What git says, its errors,
//! goes into the job's log, credentials taken out.

use crate::cancel::Cancel;
use crate::db::{Db, Repo};
use crate::paths;
use crate::secrets;
use crate::source::{self, Source};
use anyhow::{Context, Result, bail};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

/// Adds the repository `typed` names (see [`Source::parse`]), relative to
/// `cwd`, and gives it back; or the one there already. A local one must be
/// in a git repository, which is what's added, from its top.
pub fn add(db: &mut Db, typed: &str, cwd: &Path) -> Result<Repo> {
    let source = normalized(Source::parse(typed, cwd)?)?;
    db.add_repo(&source)
}

/// The repository `typed` names: by its key, or by where its code is.
pub fn find(db: &Db, typed: &str, cwd: &Path) -> Result<Option<Repo>> {
    if source::is_key(typed)
        && let Some(repo) = db.repo(typed)?
    {
        return Ok(Some(repo));
    }
    let Ok(source) = Source::parse(typed, cwd).and_then(normalized) else {
        return Ok(None);
    };
    Ok(db.repos()?.into_iter().find(|repo| repo.source == source))
}

/// `source`, a local one made the top of the git repository it's in.
fn normalized(source: Source) -> Result<Source> {
    let Source::Local { path } = &source else {
        return Ok(source);
    };
    let top = git_output(path, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("{} isn't in a git repository", path.display()))?;
    let top = PathBuf::from(top);
    Ok(Source::Local {
        path: top.canonicalize().unwrap_or(top),
    })
}

/// Where `repo`'s code is read.
pub fn root(repo: &Repo) -> PathBuf {
    match &repo.source {
        Source::Local { path } => path.clone(),
        Source::Git { .. } => paths::checkout_dir(&repo.key),
    }
}

/// Forgets the repository called `key` and takes its files away: its
/// clone, its builds and its versions; a local repository itself is never
/// touched. Whether there was one.
pub fn remove(db: &Db, key: &str) -> Result<bool> {
    if !db.remove_repo(key)? {
        return Ok(false);
    }
    let dir = paths::repo_dir(key);
    match std::fs::remove_dir_all(&dir) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(err).with_context(|| format!("couldn't remove {}", dir.display()))
        }
        _ => Ok(true),
    }
}

/// The commit a build reads, and its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    pub commit: String,
    pub branch: Option<String>,
}

/// Has `repo`'s code ready to read, and says at which commit: a local one
/// as it is; a git one cloned the first time, and fetched again when
/// `fetch` says, at its remote's default branch. What git says goes to
/// `log`; `cancel` stops it.
pub fn prepare(
    repo: &Repo,
    fetch: bool,
    log: &mut dyn FnMut(String),
    cancel: &Cancel,
) -> Result<Checkout> {
    let root = root(repo);
    if let Source::Git { url } = &repo.source {
        if !root.join(".git").exists() {
            clone(url, &root, log, cancel)?;
        } else if fetch {
            log("fetching the latest commits".to_string());
            run_git(&root, &["fetch", "--prune", "origin"], log, cancel)?;
        }
        let branch = default_branch(&root, log, cancel)?;
        let remote = format!("refs/remotes/origin/{branch}");
        run_git(
            &root,
            &["checkout", "-q", "-f", "--detach", &remote],
            log,
            cancel,
        )?;
        let commit = git_output(&root, &["rev-parse", "HEAD"])?;
        return Ok(Checkout {
            commit,
            branch: Some(branch),
        });
    }
    if !root.is_dir() {
        bail!("{} isn't there any more", root.display());
    }
    let commit = git_output(&root, &["rev-parse", "HEAD"])
        .with_context(|| format!("{} has no commit to read", root.display()))?;
    let branch = git_output(&root, &["symbolic-ref", "--short", "-q", "HEAD"]).ok();
    Ok(Checkout { commit, branch })
}

/// The commit `branch` of `repo` is at now, as far as lattice knows: a
/// local repository's branch, or a clone's remote branch as it was last
/// fetched.
pub fn head(repo: &Repo, branch: Option<&str>) -> Option<String> {
    let reference = match (&repo.source, branch) {
        (Source::Local { .. }, Some(branch)) => format!("refs/heads/{branch}"),
        (Source::Local { .. }, None) => "HEAD".to_string(),
        (Source::Git { .. }, Some(branch)) => format!("refs/remotes/origin/{branch}"),
        (Source::Git { .. }, None) => "refs/remotes/origin/HEAD".to_string(),
    };
    let commit = format!("{reference}^{{commit}}");
    git_output(&root(repo), &["rev-parse", "--verify", "--quiet", &commit]).ok()
}

/// Fetches `repo`'s remote, saying nothing, for [`head`] to know where its
/// branch is: what the server does now and then while a page looks.
pub fn fetch_quietly(repo: &Repo) {
    let root = root(repo);
    if matches!(repo.source, Source::Git { .. }) && root.join(".git").exists() {
        let _ = run_git(
            &root,
            &["fetch", "--quiet", "origin"],
            &mut |_| {},
            &Cancel::new(),
        );
    }
}

/// Clones `url` into `into`, with `gh` for GitHub when it's there and
/// logged in, and git otherwise; nothing is left of a clone that failed.
fn clone(url: &str, into: &Path, log: &mut dyn FnMut(String), cancel: &Cancel) -> Result<()> {
    let parent = into.parent().context("a checkout has a directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("couldn't make {}", parent.display()))?;
    let _ = std::fs::remove_dir_all(into);
    let target = into.to_string_lossy().into_owned();
    log(format!("cloning {}", secrets::redact(url)));
    if let Some(name) = github_name(url)
        && has("gh")
    {
        let mut gh = Command::new("gh");
        gh.args(["repo", "clone", &name, &target]);
        if run(gh, parent, log, cancel).is_ok() {
            return Ok(());
        }
        let _ = std::fs::remove_dir_all(into);
        log("gh couldn't clone it; git tries".to_string());
    }
    let cloned = run_git(parent, &["clone", "--", url, &target], log, cancel);
    if cloned.is_err() {
        let _ = std::fs::remove_dir_all(into);
    }
    cloned
}

/// GitHub's `owner/repo` of a github.com URL over HTTPS, which `gh` clones.
fn github_name(url: &str) -> Option<String> {
    let path = url.strip_prefix("https://github.com/")?;
    let name = path.trim_end_matches('/').trim_end_matches(".git");
    let (owner, repo) = name.split_once('/')?;
    let plain = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    (plain(owner) && plain(repo) && !repo.contains('/')).then(|| name.to_string())
}

/// The remote's default branch, as the clone knows it: what its `HEAD`
/// points at, asked of the remote when the clone doesn't say.
fn default_branch(root: &Path, log: &mut dyn FnMut(String), cancel: &Cancel) -> Result<String> {
    let known = || {
        git_output(
            root,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )
    };
    let head = match known() {
        Ok(head) => head,
        Err(_) => {
            run_git(
                root,
                &["remote", "set-head", "origin", "--auto"],
                log,
                cancel,
            )?;
            known().context("the remote doesn't say which branch is its default")?
        }
    };
    Ok(head.strip_prefix("origin/").unwrap_or(&head).to_string())
}

/// Whether `program` is on the `PATH`.
fn has(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// git, as lattice runs it: never asking for a password, never reading
/// from a terminal.
fn git(dir: &Path) -> Command {
    let mut git = Command::new("git");
    git.arg("-C").arg(dir).env("GIT_TERMINAL_PROMPT", "0");
    git
}

/// What `git args` prints in `dir`, trimmed, or why it failed.
fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let out = git(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("couldn't run git: is it installed?")?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        bail!("git {} failed: {}", args.join(" "), said.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Runs `git args` in `dir`, what it says on its standard error going to
/// `log`, until it ends or `cancel` says to stop.
fn run_git(dir: &Path, args: &[&str], log: &mut dyn FnMut(String), cancel: &Cancel) -> Result<()> {
    let mut command = git(dir);
    command.args(args);
    run(command, dir, log, cancel).with_context(|| format!("git {} failed", args[0]))
}

/// Runs `command` in `dir`, its standard error, a line at a time, going to
/// `log` with credentials taken out, until it ends or `cancel` says to stop
/// it.
fn run(
    mut command: Command,
    dir: &Path,
    log: &mut dyn FnMut(String),
    cancel: &Cancel,
) -> Result<()> {
    let mut child = command
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't start it")?;
    let stderr = child.stderr.take().expect("its errors are piped");
    let (send, lines) = std::sync::mpsc::channel();
    thread::spawn(move || {
        // git ends its progress lines with a carriage return.
        for line in BufReader::new(stderr).split(b'\n') {
            let Ok(line) = line else { break };
            let line = String::from_utf8_lossy(&line).into_owned();
            let Some(line) = line.rsplit('\r').find(|part| !part.trim().is_empty()) else {
                continue;
            };
            if send.send(line.trim().to_string()).is_err() {
                break;
            }
        }
    });
    let mut said = None;
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            bail!("it was cancelled");
        }
        match lines.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let line = secrets::redact(&crate::printable::line(&line));
                said = Some(line.clone());
                log(line);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait()?;
    if !status.success() {
        match said {
            Some(said) => bail!("{said}"),
            None => bail!("it ended with {status}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_github_url_over_https_is_cloned_by_its_name() {
        assert_eq!(
            github_name("https://github.com/golang/go.git").as_deref(),
            Some("golang/go")
        );
        assert_eq!(
            github_name("https://github.com/golang/go/").as_deref(),
            Some("golang/go")
        );
        assert_eq!(github_name("git@github.com:golang/go.git"), None);
        assert_eq!(github_name("https://gitlab.com/golang/go.git"), None);
        assert_eq!(github_name("https://github.com/golang/go/tree/x"), None);
    }
}
