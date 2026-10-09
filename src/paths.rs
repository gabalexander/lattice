//! Where lattice keeps what it keeps, as the XDG base directories say:
//!
//! - the settings, `$XDG_CONFIG_HOME/lattice/config.toml` (else
//!   `~/.config/lattice/`), which people edit by hand ([`crate::config`]);
//! - its data, `$XDG_DATA_HOME/lattice` (else `~/.local/share/lattice`):
//!   the database of repositories, versions and jobs ([`crate::db`]), and
//!   under `repos/<key>/` each repository's files: `checkout/`, the clone
//!   of a git source (a local one is read where it is), `work/`, the build
//!   going on or cut short, and `v<n>/`, each version a build made, with
//!   its `wiki.json`, `build.json` and `build.log`;
//! - what it downloads, `$XDG_CACHE_HOME/lattice` (else `~/.cache/lattice`),
//!   like mermaid ([`crate::download`]).
//!
//! The variables are what the tests set, to a directory of their own each,
//! and what a container points at its volumes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The wiki a build makes, in a version's directory.
pub const WIKI_FILE: &str = "wiki.json";

/// The build's own bookkeeping, for the next build to carry on from.
pub const BUILD_FILE: &str = "build.json";

/// What the build did, a line a step.
pub const LOG_FILE: &str = "build.log";

/// The settings file.
pub fn config_file() -> PathBuf {
    base("XDG_CONFIG_HOME", ".config")
        .join("lattice")
        .join("config.toml")
}

/// lattice's data directory.
pub fn data_dir() -> PathBuf {
    base("XDG_DATA_HOME", ".local/share").join("lattice")
}

/// lattice's directory in the user's cache.
pub fn cache_dir() -> PathBuf {
    base("XDG_CACHE_HOME", ".cache").join("lattice")
}

/// The database, in the data directory.
pub fn db_file() -> PathBuf {
    data_dir().join("lattice.db")
}

/// The files of the repository called `key`.
pub fn repo_dir(key: &str) -> PathBuf {
    data_dir().join("repos").join(key)
}

/// Where a git source is cloned.
pub fn checkout_dir(key: &str) -> PathBuf {
    repo_dir(key).join("checkout")
}

/// Where the build going on, or the one cut short, keeps its files.
pub fn work_dir(key: &str) -> PathBuf {
    repo_dir(key).join("work")
}

/// The files of version `n` of the wiki of the repository called `key`.
pub fn version_dir(key: &str, n: u32) -> PathBuf {
    repo_dir(key).join(format!("v{n}"))
}

/// The directory the variable `name` says, or else `home_default` in the
/// home directory.
fn base(name: &str, home_default: &str) -> PathBuf {
    from(
        std::env::var_os(name),
        std::env::var_os("HOME"),
        home_default,
    )
}

/// `from_env`, when it's an absolute path, as the XDG spec has it; or else
/// `home_default` under `home`.
fn from(from_env: Option<OsString>, home: Option<OsString>, home_default: &str) -> PathBuf {
    match from_env {
        Some(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        _ => PathBuf::from(home.unwrap_or_default()).join(home_default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_names_the_directory_and_home_is_the_fallback() {
        let home = Some(OsString::from("/home/ann"));
        assert_eq!(
            from(Some("/xdg/data".into()), home.clone(), ".local/share"),
            PathBuf::from("/xdg/data")
        );
        assert_eq!(
            from(None, home.clone(), ".local/share"),
            PathBuf::from("/home/ann/.local/share")
        );
        // The spec says a relative path is to be ignored.
        assert_eq!(
            from(Some("data".into()), home, ".cache"),
            PathBuf::from("/home/ann/.cache")
        );
    }

    #[test]
    fn a_repository_s_files_are_under_its_key() {
        let data = data_dir();
        assert_eq!(repo_dir("app"), data.join("repos/app"));
        assert_eq!(checkout_dir("app"), data.join("repos/app/checkout"));
        assert_eq!(work_dir("app"), data.join("repos/app/work"));
        assert_eq!(version_dir("app", 3), data.join("repos/app/v3"));
        assert_eq!(db_file(), data.join("lattice.db"));
        assert!(config_file().ends_with("lattice/config.toml"));
    }
}
