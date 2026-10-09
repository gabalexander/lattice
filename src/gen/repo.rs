//! What the generator needs of git: a tree of exactly the commit it
//! writes from, so that nothing uncommitted shows; the commits that
//! touched some files; what changed between two commits; where a line of
//! a file at one commit is at another, so that the links of what isn't
//! written again still point where they did; and where the repository is
//! on its forge, for the page's links to it.
//!
//! The job has the code ready before the generator starts, a clone of a
//! git source at the commit, or a repository on this machine as it is.
//! When the latter isn't clean at the commit, the generator reads a clone
//! of its own instead, one that shares its objects, so it costs no more
//! disk than its files.
//!
//! Adapted from crystal's wiki generator (`src/wiki/repo.rs`, MIT).

use crate::printable;
use crate::wiki::CodeLink;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs git in `dir` and gives back what it printed.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .context("couldn't run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            printable::line(String::from_utf8_lossy(&out.stderr).trim())
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A tree of `commit` to read: `root` itself when it's checked out at the
/// commit with nothing uncommitted, or else `clone`, made a clean checkout
/// of the commit that shares `root`'s objects.
pub fn tree(root: &Path, commit: &str, clone: &Path) -> Result<PathBuf> {
    let head = git(root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let clean = git(root, &["status", "--porcelain", "--untracked-files=no"])
        .is_ok_and(|said| said.trim().is_empty());
    if head.trim() == commit && clean {
        return Ok(root.to_path_buf());
    }
    if !clone.join(".git").exists() {
        if clone.exists() {
            std::fs::remove_dir_all(clone)
                .with_context(|| format!("couldn't clear {}", clone.display()))?;
        }
        if let Some(parent) = clone.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let out = Command::new("git")
            .args(["clone", "--quiet", "--shared", "--no-checkout"])
            .arg(root)
            .arg(clone)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("couldn't run git")?;
        if !out.status.success() {
            bail!(
                "couldn't make a checkout of {}: {}",
                root.display(),
                printable::line(String::from_utf8_lossy(&out.stderr).trim())
            );
        }
    }
    // The commit may be newer than what the clone has fetched.
    if git(clone, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_err() {
        git(clone, &["fetch", "--quiet", "origin", commit])
            .with_context(|| format!("couldn't fetch {commit} into the checkout"))?;
    }
    git(
        clone,
        &["checkout", "--quiet", "--force", "--detach", commit],
    )
    .with_context(|| format!("couldn't check out {commit}"))?;
    git(clone, &["clean", "-ffdxq"])?;
    Ok(clone.to_path_buf())
}

/// The commits that touched `paths` lately, the newest first, a line each:
/// `a1b2c3d 2026-10-01 Fix the ledger`. Nothing when there are none or git
/// can't say.
pub fn log(tree: &Path, paths: &[String], most: usize) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let most = most.to_string();
    let mut args = vec![
        "log",
        "--no-merges",
        "--format=%h %ad %s",
        "--date=short",
        "-n",
        &most,
        "--",
    ];
    args.extend(paths.iter().map(|path| path.trim_end_matches('/')));
    printable::text(&git(tree, &args).unwrap_or_default()).into_owned()
}

/// The commits from `old` to `new` that touched `paths`, a line each, the
/// newest first, at most `most`.
pub fn commits_between(tree: &Path, old: &str, new: &str, paths: &[String], most: usize) -> String {
    let range = format!("{old}..{new}");
    let most = most.to_string();
    let mut args = vec![
        "log",
        "--no-merges",
        "--format=%h %ad %s",
        "--date=short",
        "-n",
        &most,
        &range,
        "--",
    ];
    args.extend(paths.iter().map(|path| path.trim_end_matches('/')));
    printable::text(&git(tree, &args).unwrap_or_default()).into_owned()
}

/// What changed in `paths` from `old` to `new`, as a patch cut to `most`
/// bytes.
pub fn diff(tree: &Path, old: &str, new: &str, paths: &[String], most: usize) -> String {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-M", old, new, "--"];
    args.extend(paths.iter().map(|path| path.trim_end_matches('/')));
    let mut patch = git(tree, &args).unwrap_or_default();
    if patch.len() > most {
        let mut cut = most;
        while !patch.is_char_boundary(cut) {
            cut -= 1;
        }
        patch.truncate(cut);
        patch.push_str("\n[the rest of the diff is cut]\n");
    }
    patch
}

/// The files that changed from `old` to `new`, by their paths at `new`,
/// and those that went, by their paths at `old`.
pub fn changed(tree: &Path, old: &str, new: &str) -> Result<Vec<String>> {
    let said = git(
        tree,
        &["diff", "--name-only", "--no-renames", "-z", old, new],
    )?;
    Ok(said
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

/// A repository on its forge's web site, for links to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Web {
    /// `owner/name`, or for GitLab, `group/subgroup/name`.
    pub path: String,
    /// The repository's page: `https://github.com/owner/name`.
    pub url: String,
    /// Where a file is at a commit, `{commit}` and `{path}` to fill in.
    pub code_url: String,
}

/// The forges a wiki links into, by how they write a file's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Forge {
    GitHub,
    GitLab,
    /// Gitea, Forgejo and Codeberg.
    Gitea,
}

/// The repository whose remote is `url` on the web, when it's on a forge
/// lattice knows by its host: GitHub's and GitLab's own, and a server
/// whose name says it's one of theirs, or Gitea's.
pub fn web(url: &str) -> Option<Web> {
    let (authority, path) = match url.split_once("://") {
        Some((scheme, rest)) if scheme != "file" => rest.split_once('/')?,
        Some(_) => return None,
        // scp's way, `user@host:path`.
        None => url.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?;
    let host = match host.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => host,
    };
    let host = match host.to_ascii_lowercase().as_str() {
        "ssh.github.com" => "github.com".to_string(),
        "altssh.gitlab.com" => "gitlab.com".to_string(),
        other => other.to_string(),
    };
    let forge = if host.contains("github") {
        Forge::GitHub
    } else if host.contains("gitlab") {
        Forge::GitLab
    } else if host.contains("gitea") || host.contains("forgejo") || host == "codeberg.org" {
        Forge::Gitea
    } else {
        return None;
    };
    let path = path
        .trim_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    if host.is_empty() || path.split('/').count() < 2 || path.split('/').any(str::is_empty) {
        return None;
    }
    let url = format!("https://{host}/{path}");
    let code_url = match forge {
        Forge::GitHub => format!("{url}/blob/{{commit}}/{{path}}"),
        Forge::GitLab => format!("{url}/-/blob/{{commit}}/{{path}}"),
        Forge::Gitea => format!("{url}/src/commit/{{commit}}/{{path}}"),
    };
    Some(Web {
        path: path.to_string(),
        url,
        code_url,
    })
}

/// The URL of the remote `origin` of the repository at `root`, when it has
/// one.
pub fn origin(root: &Path) -> Option<String> {
    git(root, &["config", "--get", "remote.origin.url"])
        .ok()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
}

/// Where the lines of each file at one commit are at another.
#[derive(Debug, Default)]
pub struct Remap {
    /// Files renamed, from their old path to their new.
    renames: HashMap<String, String>,
    /// Each changed file's hunks, by its old path, in order.
    hunks: HashMap<String, Vec<Hunk>>,
}

/// A hunk of `git diff -U0`: `old_len` lines from `old_start` became
/// `new_len` from `new_start`. With `old_len` 0, the lines went in after
/// line `old_start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hunk {
    old_start: u32,
    old_len: u32,
    new_start: u32,
    new_len: u32,
}

impl Remap {
    /// What changed from `old` to `new` in the tree at `dir`.
    pub fn between(dir: &Path, old: &str, new: &str) -> Result<Remap> {
        let patch = git(
            dir,
            &["diff", "--no-color", "--no-ext-diff", "-M", "-U0", old, new],
        )?;
        Ok(Remap::of_patch(&patch))
    }

    /// Reads a patch of `git diff -M -U0`.
    fn of_patch(patch: &str) -> Remap {
        let mut remap = Remap::default();
        let mut file: Option<String> = None;
        for line in patch.lines() {
            if line.starts_with("diff --git ") {
                file = None;
            } else if let Some(from) = line.strip_prefix("rename from ") {
                file = Some(from.to_string());
            } else if let Some(to) = line.strip_prefix("rename to ") {
                if let Some(from) = &file {
                    remap.renames.insert(from.clone(), to.to_string());
                }
            } else if let Some(path) = line.strip_prefix("--- a/") {
                file = Some(path.to_string());
            } else if let Some(header) = line.strip_prefix("@@ ") {
                let (Some(file), Some(hunk)) = (&file, hunk_of(header)) else {
                    continue;
                };
                remap.hunks.entry(file.clone()).or_default().push(hunk);
            }
        }
        remap
    }

    /// Where line `line` of `path` at the old commit is at the new: moved
    /// by the lines put in and taken out above it, or for a line that
    /// changed, the start of what it became.
    fn line(&self, path: &str, line: u32) -> u32 {
        let mut offset: i64 = 0;
        for hunk in self.hunks.get(path).into_iter().flatten() {
            if hunk.old_len == 0 {
                if line > hunk.old_start {
                    offset += i64::from(hunk.new_len);
                    continue;
                }
                break;
            }
            if line < hunk.old_start {
                break;
            }
            if line < hunk.old_start + hunk.old_len {
                return hunk.new_start.max(1);
            }
            offset += i64::from(hunk.new_len) - i64::from(hunk.old_len);
        }
        (i64::from(line) + offset).max(1) as u32
    }

    /// `link`, written at the old commit, where it is at the new.
    pub fn link(&self, link: &CodeLink) -> CodeLink {
        let path = self
            .renames
            .get(&link.path)
            .cloned()
            .unwrap_or_else(|| link.path.clone());
        let start = link.start.map(|line| self.line(&link.path, line));
        let end = match (link.end, start) {
            (Some(end), Some(start)) => Some(self.line(&link.path, end).max(start)),
            _ => None,
        };
        CodeLink { path, start, end }
    }

    /// Whether nothing moved.
    pub fn is_empty(&self) -> bool {
        self.renames.is_empty() && self.hunks.is_empty()
    }
}

/// `-12,3 +14,0 @@ …` read.
fn hunk_of(header: &str) -> Option<Hunk> {
    let mut parts = header.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |text: &str| -> Option<(u32, u32)> {
        match text.split_once(',') {
            Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
            None => Some((text.parse().ok()?, 1)),
        }
    };
    let (old_start, old_len) = range(old)?;
    let (new_start, new_len) = range(new)?;
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    /// Runs git in `dir` as a test would, failing it when git fails.
    pub fn run(dir: &Path, args: &[&str]) -> String {
        git(dir, args).unwrap()
    }

    /// Commits everything in `dir` with `message`, and gives back the
    /// commit.
    pub fn commit(dir: &Path, message: &str) -> String {
        run(dir, &["add", "-A"]);
        run(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                message,
            ],
        );
        run(dir, &["rev-parse", "HEAD"]).trim().to_string()
    }

    #[test]
    fn a_line_moves_with_what_went_in_and_out_above_it() {
        let patch = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n\
@@ -2,0 +3,2 @@ fn x\n+one\n+two\n@@ -10,3 +12 @@\n-a\n-b\n-c\n+d\n\
diff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\n--- a/old.rs\n+++ b/new.rs\n@@ -1 +1 @@\n-x\n+y\n";
        let remap = Remap::of_patch(patch);
        // Above the first change, nothing moves.
        assert_eq!(remap.line("src/a.rs", 2), 2);
        // Two lines went in after line 2.
        assert_eq!(remap.line("src/a.rs", 3), 5);
        assert_eq!(remap.line("src/a.rs", 9), 11);
        // Lines 10 to 12 became line 12.
        assert_eq!(remap.line("src/a.rs", 11), 12);
        // Below, two in and two out.
        assert_eq!(remap.line("src/a.rs", 20), 20);
        assert_eq!(remap.line("untouched.rs", 7), 7);
        let link = remap.link(&CodeLink {
            path: "old.rs".into(),
            start: Some(1),
            end: Some(4),
        });
        assert_eq!(
            link,
            CodeLink {
                path: "new.rs".into(),
                start: Some(1),
                end: Some(4)
            }
        );
        assert!(!remap.is_empty());
    }

    #[test]
    fn a_remote_on_a_forge_has_a_page_and_its_files_a_place() {
        let github = web("git@github.com:acme/app.git").unwrap();
        assert_eq!(github.path, "acme/app");
        assert_eq!(github.url, "https://github.com/acme/app");
        assert_eq!(
            github.code_url,
            "https://github.com/acme/app/blob/{commit}/{path}"
        );
        let ssh = web("ssh://git@ssh.github.com:443/acme/app.git").unwrap();
        assert_eq!(ssh.url, "https://github.com/acme/app");
        let gitlab = web("https://gitlab.acme.io/platform/sub/api.git").unwrap();
        assert_eq!(gitlab.path, "platform/sub/api");
        assert_eq!(
            gitlab.code_url,
            "https://gitlab.acme.io/platform/sub/api/-/blob/{commit}/{path}"
        );
        assert_eq!(
            web("https://codeberg.org/acme/app").unwrap().code_url,
            "https://codeberg.org/acme/app/src/commit/{commit}/{path}"
        );
        assert_eq!(web("git@github.com:app.git"), None);
        assert_eq!(web("https://git.acme.io/acme/app.git"), None);
        assert_eq!(web("file:///srv/git/app.git"), None);
    }

    #[test]
    fn a_tree_is_the_commit_and_nothing_uncommitted() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("app");
        fs::create_dir(&project).unwrap();
        run(&project, &["init", "-q"]);
        fs::write(project.join("a.rs"), "fn a() {}\n").unwrap();
        let first = commit(&project, "first");
        fs::write(project.join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        let second = commit(&project, "second");
        let clone = dir.path().join("tree");
        // Clean at the commit: read where it is.
        assert_eq!(tree(&project, &second, &clone).unwrap(), project);
        assert!(!clone.exists());
        // At another commit, or with changes, a clone of its own.
        fs::write(project.join("a.rs"), "uncommitted\n").unwrap();
        assert_eq!(tree(&project, &second, &clone).unwrap(), clone);
        assert_eq!(
            fs::read_to_string(clone.join("a.rs")).unwrap(),
            "fn a() {}\nfn b() {}\n"
        );
        fs::write(clone.join("stray.txt"), "x").unwrap();
        tree(&project, &first, &clone).unwrap();
        assert_eq!(
            fs::read_to_string(clone.join("a.rs")).unwrap(),
            "fn a() {}\n"
        );
        assert!(!clone.join("stray.txt").exists());
        assert!(log(&clone, &["a.rs".into()], 5).contains(" first\n"));
        // A commit made after the clone was is fetched into it.
        fs::write(project.join("c.rs"), "fn c() {}\n").unwrap();
        let third = commit(&project, "third");
        fs::write(project.join("c.rs"), "uncommitted\n").unwrap();
        assert_eq!(tree(&project, &third, &clone).unwrap(), clone);
        assert!(clone.join("c.rs").exists());
        assert!(commits_between(&clone, &first, &third, &["a.rs".into()], 9).contains(" second\n"));
        assert_eq!(changed(&clone, &first, &third).unwrap(), ["a.rs", "c.rs"]);
        let remap = Remap::between(&clone, &first, &second).unwrap();
        assert_eq!(remap.line("a.rs", 1), 1);
        assert!(diff(&clone, &first, &second, &["a.rs".into()], 10_000).contains("+fn b() {}"));
        // The project has no worktree of it.
        assert!(!run(&project, &["worktree", "list"]).contains("tree"));
    }
}
