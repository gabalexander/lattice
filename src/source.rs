//! Where a repository's code comes from, as the user gives it: a path on
//! this machine, a git URL on any git server, or `owner/repo` on GitHub;
//! and the key lattice keeps it under, which is its page's address.
//!
//! A local repository is read where it is. A git one is cloned into
//! lattice's data directory with the user's own git, and so with their
//! credentials: their SSH keys and credential helpers. A URL with a
//! password in it is refused, since lattice would keep it and show it.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The schemes a git URL may have: what git clones over, and nothing that
/// runs a command, like `ext::`.
const SCHEMES: &[&str] = &["https", "http", "ssh", "git", "file"];

/// The words the web app's own pages take at the top of the address, which
/// no repository's key may be.
pub const RESERVED: &[&str] = &[
    "api", "assets", "settings", "jobs", "export", "fonts", "_app",
];

/// Where a repository's code is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// A repository on this machine, by its absolute path.
    Local { path: PathBuf },
    /// A repository on a git server, by the URL git clones it from.
    Git { url: String },
}

impl Source {
    /// What `typed` names, relative to `cwd`: a path when it's a directory
    /// there or starts like one (`/`, `~`, `.`), a git URL when it has a
    /// scheme or is written as scp writes one (`git@host:owner/repo`), and
    /// GitHub's `owner/repo` when it's that.
    pub fn parse(typed: &str, cwd: &Path) -> Result<Source> {
        let typed = typed.trim();
        if typed.is_empty() {
            bail!("say which repository: a path, a git URL, or owner/repo on GitHub");
        }
        if typed.starts_with('-') {
            bail!("{typed} isn't a repository");
        }
        let path = crate::shell::expand_home(Path::new(typed));
        let path = cwd.join(path);
        if typed.starts_with(['/', '~', '.']) || path.is_dir() {
            if !path.is_dir() {
                bail!("there's no directory at {}", path.display());
            }
            let path = path.canonicalize().unwrap_or(path);
            return Ok(Source::Local { path });
        }
        if let Some((scheme, _)) = typed.split_once("://") {
            if !SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) {
                bail!(
                    "{scheme}:// isn't a way lattice clones: give an {} URL",
                    SCHEMES.join(", ")
                );
            }
            return git(typed);
        }
        if is_scp_like(typed) {
            return git(typed);
        }
        if is_github(typed) {
            let name = typed.trim_end_matches(".git");
            return Ok(Source::Git {
                url: format!("https://github.com/{name}.git"),
            });
        }
        bail!(
            "{typed} isn't a directory, a git URL or owner/repo on GitHub",
            typed = crate::printable::line(typed)
        )
    }

    /// The repository's name as people say it: `owner/repo` from a git
    /// URL's path, or a directory's name.
    pub fn name(&self) -> String {
        match self {
            Source::Local { path } => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".to_string()),
            Source::Git { url } => {
                let path = match url.split_once("://") {
                    Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
                    None => url.split_once(':').map_or("", |(_, path)| path),
                };
                let path = path.trim_end_matches('/').trim_end_matches(".git");
                let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
                match parts.as_slice() {
                    [] => "repo".to_string(),
                    [one] => one.to_string(),
                    [.., owner, repo] => format!("{owner}/{repo}"),
                }
            }
        }
    }
}

/// A git URL, unless it carries a password, or a space or a control
/// character, which no URL has.
fn git(url: &str) -> Result<Source> {
    if url.contains(|c: char| c.is_whitespace() || c.is_control()) {
        bail!("{} isn't a git URL", crate::printable::line(url));
    }
    let authority = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();
    if let Some((user, _)) = authority.rsplit_once('@')
        && user.contains(':')
    {
        bail!(
            "lattice doesn't keep passwords: leave it out of the URL, and let git's credential \
             helper or an SSH key give it"
        );
    }
    Ok(Source::Git {
        url: url.to_string(),
    })
}

/// Whether `typed` is a git URL as scp writes it, `[user@]host:path`: a
/// colon before any slash, a host's name before it, and a path after it,
/// not another colon as `ext::` has.
fn is_scp_like(typed: &str) -> bool {
    let Some((host, path)) = typed.split_once(':') else {
        return false;
    };
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    !host.is_empty()
        && !path.is_empty()
        && !path.starts_with(':')
        && !host.contains('/')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-.".contains(c))
}

/// Whether `typed` is GitHub's `owner/repo`.
fn is_github(typed: &str) -> bool {
    let Some((owner, repo)) = typed.split_once('/') else {
        return false;
    };
    let owner_ok = owner
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo_ok = !repo.is_empty()
        && !repo.starts_with('.')
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    owner_ok && repo_ok
}

/// Whether `key` could be a repository's key: lowercase letters, digits
/// and dashes, starting with a letter or a digit, and none of the web
/// app's own words.
pub fn is_key(key: &str) -> bool {
    key.len() <= 64
        && key
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !RESERVED.contains(&key)
}

/// The key for a repository called `name`, which `taken` says isn't one
/// already: the repository's own name, then its owner's and its own, then
/// one of those numbered.
pub fn key_for(name: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    let slug = |text: &str| -> String {
        let mut slug = String::new();
        for c in text.chars() {
            match c.to_ascii_lowercase() {
                c if c.is_ascii_alphanumeric() => slug.push(c),
                _ if !slug.is_empty() && !slug.ends_with('-') => slug.push('-'),
                _ => {}
            }
        }
        let slug: String = slug.trim_end_matches('-').chars().take(48).collect();
        slug.trim_end_matches('-').to_string()
    };
    let own = slug(name.rsplit('/').next().unwrap_or(name));
    let own = if own.is_empty() {
        "repo".to_string()
    } else {
        own
    };
    let mut tries = vec![own.clone(), slug(name)];
    tries.retain(|key| !key.is_empty());
    tries.dedup();
    if let Some(key) = tries.iter().find(|key| is_key(key) && !taken(key)) {
        return key.clone();
    }
    (2..)
        .map(|n| format!("{own}-{n}"))
        .find(|key| is_key(key) && !taken(key))
        .expect("there's always a number free")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(typed: &str) -> Result<Source> {
        Source::parse(typed, Path::new("/nowhere"))
    }

    fn url(typed: &str) -> String {
        match parse(typed).unwrap() {
            Source::Git { url } => url,
            local => panic!("{typed} is {local:?}"),
        }
    }

    #[test]
    fn a_directory_is_a_local_source_by_its_real_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("app")).unwrap();
        let real = dir.path().join("app").canonicalize().unwrap();
        let local = Source::parse("app", dir.path()).unwrap();
        assert_eq!(local, Source::Local { path: real.clone() });
        let absolute = Source::parse(&real.display().to_string(), Path::new("/")).unwrap();
        assert_eq!(absolute, Source::Local { path: real });
        assert_eq!(absolute.name(), "app");
        let missing = parse("./gone").unwrap_err().to_string();
        assert!(missing.contains("there's no directory"), "{missing}");
    }

    #[test]
    fn git_urls_are_taken_as_git_clones_them() {
        for typed in [
            "https://github.com/golang/go.git",
            "https://git.example.com/team/sub/app",
            "ssh://git@git.example.com:2222/team/app.git",
            "git@github.com:gabalexander/crystal.git",
            "deploy@10.0.0.7:repos/app",
            "file:///srv/git/app.git",
        ] {
            assert_eq!(url(typed), typed);
        }
        assert_eq!(url("golang/go"), "https://github.com/golang/go.git");
        assert_eq!(url("golang/go.git"), "https://github.com/golang/go.git");
    }

    #[test]
    fn what_isn_t_a_repository_or_carries_a_password_is_refused() {
        for typed in [
            "",
            "--upload-pack=touch /tmp/x",
            "ext::sh -c touch% /tmp/pwned",
            "ftp://example.com/app.git",
            "just words",
            "a/b/c",
            "git@github.com:owner/repo name",
        ] {
            assert!(parse(typed).is_err(), "{typed}");
        }
        let password = parse("https://me:hunter2@git.example.com/app.git").unwrap_err();
        assert!(password.to_string().contains("doesn't keep passwords"));
        assert_eq!(
            url("https://me@git.example.com/app.git"),
            "https://me@git.example.com/app.git",
            "a user alone is no password"
        );
    }

    #[test]
    fn a_source_is_named_by_its_owner_and_repository() {
        let named = |typed: &str| parse(typed).unwrap().name();
        assert_eq!(named("golang/go"), "golang/go");
        assert_eq!(
            named("git@github.com:gabalexander/crystal.git"),
            "gabalexander/crystal"
        );
        assert_eq!(named("https://git.example.com/team/sub/app/"), "sub/app");
        assert_eq!(named("deploy@10.0.0.7:app"), "app");
    }

    #[test]
    fn a_key_is_the_repository_s_name_unless_it_s_taken_or_reserved() {
        let none = |_: &str| false;
        assert_eq!(key_for("golang/go", none), "go");
        assert_eq!(key_for("gabalexander/Crystal_App", none), "crystal-app");
        assert_eq!(key_for("acme/go", |key| key == "go"), "acme-go");
        assert_eq!(key_for("go", |key| key == "go"), "go-2");
        assert_eq!(key_for("acme/settings", none), "acme-settings");
        assert_eq!(key_for("settings", none), "settings-2");
        assert_eq!(key_for("日本", none), "repo");
        for key in ["go", "crystal-app", "a1"] {
            assert!(is_key(key), "{key}");
        }
        for key in ["", "-go", "Go", "go.json", "_app", "api", "jobs", "a/b"] {
            assert!(!is_key(key), "{key}");
        }
    }
}
