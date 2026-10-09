//! What lattice downloads rather than ships: mermaid, which draws the
//! wiki's diagrams in the browser. It's downloaded once, at a pinned
//! version, with `curl`, checked against its SHA-256, and kept in
//! lattice's cache, where the server serves it from and an export copies
//! it out of; a page never loads it from a CDN.
//!
//! Adapted from crystal's `src/wiki_site.rs` and `src/embed.rs` (MIT).

use crate::paths;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The mermaid build the page draws its diagrams with: one file, which
/// sets `window.mermaid`.
pub const MERMAID: Download = Download {
    name: "mermaid.min.js",
    version: "11.17.2",
    url: "https://cdn.jsdelivr.net/npm/mermaid@11.17.2/dist/mermaid.min.js",
    size: 3_572_661,
    sha256: "581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8",
};

/// The variable that keeps lattice from downloading, as its tests set:
/// what isn't in the cache already is missing.
pub const NO_DOWNLOAD: &str = "LATTICE_NO_DOWNLOAD";

/// A file lattice downloads rather than ships, at a pinned version, with
/// its size and SHA-256.
pub struct Download {
    /// Its name, as the page asks for it.
    pub name: &'static str,
    pub version: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
}

impl Download {
    /// Where it's kept in `cache`: under its version, so another never
    /// mixes with it.
    fn path(&self, cache: &Path) -> PathBuf {
        cache.join("downloads").join(self.version).join(self.name)
    }

    /// Whether it's at `path`, the size it should be. Its hash was checked
    /// as it was downloaded.
    fn is_at(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|meta| meta.len() == self.size)
    }

    /// The file, from lattice's cache, or downloaded into it first.
    pub fn fetch(&self) -> Result<PathBuf> {
        self.fetch_into(&paths::cache_dir())
    }

    /// The file, from `cache`, or downloaded into it with `curl` unless
    /// [`NO_DOWNLOAD`] is set: beside its place first, and moved there only
    /// once its hash is right.
    fn fetch_into(&self, cache: &Path) -> Result<PathBuf> {
        let path = self.path(cache);
        if self.is_at(&path) {
            return Ok(path);
        }
        if std::env::var_os(NO_DOWNLOAD).is_some() {
            bail!("{} isn't downloaded, and {NO_DOWNLOAD} is set", self.name);
        }
        let dir = path.parent().context("its place has a directory")?;
        fs::create_dir_all(dir).with_context(|| format!("couldn't make {}", dir.display()))?;
        let partial = path.with_extension(format!("part{}", std::process::id()));
        let status = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--retry",
                "3",
                "--silent",
                "--show-error",
            ])
            .arg("--output")
            .arg(&partial)
            .arg(self.url)
            .status()
            .with_context(|| format!("couldn't run curl, which downloads {}", self.name))?;
        if !status.success() {
            let _ = fs::remove_file(&partial);
            bail!("couldn't download {}", self.url);
        }
        let sha256 = sha256_of(&partial)?;
        if sha256 != self.sha256 {
            let _ = fs::remove_file(&partial);
            bail!(
                "{} came with the wrong SHA-256: {sha256}, not {}",
                self.url,
                self.sha256
            );
        }
        fs::rename(&partial, &path)
            .with_context(|| format!("couldn't move it to {}", path.display()))?;
        Ok(path)
    }
}

/// The SHA-256 of the file at `path`, in hex.
pub fn sha256_of(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("couldn't read {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "abc", and its SHA-256.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn a_file_s_hash_is_its_sha256_in_hex() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc");
        fs::write(&path, "abc").unwrap();
        assert_eq!(sha256_of(&path).unwrap(), ABC);
    }

    #[test]
    fn mermaid_is_kept_under_its_version_and_known_by_its_size() {
        let cache = tempfile::tempdir().unwrap();
        let path = MERMAID.path(cache.path());
        assert_eq!(path, cache.path().join("downloads/11.17.2/mermaid.min.js"));
        assert!(!MERMAID.is_at(&path));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"too short").unwrap();
        assert!(!MERMAID.is_at(&path));
        fs::write(&path, vec![b' '; MERMAID.size as usize]).unwrap();
        assert!(MERMAID.is_at(&path));
        assert_eq!(MERMAID.fetch_into(cache.path()).unwrap(), path);
    }

    /// A download from a file on this machine, which curl takes as it
    /// takes a URL, so nothing reaches the network.
    fn local(source: &Path, sha256: &'static str) -> Download {
        let url = format!("file://{}", source.display());
        Download {
            name: "abc.txt",
            version: "1.0.0",
            url: Box::leak(url.into_boxed_str()),
            size: 3,
            sha256,
        }
    }

    #[test]
    fn a_download_is_kept_only_once_its_hash_is_right() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("abc");
        fs::write(&source, "abc").unwrap();
        let cache = dir.path().join("cache");
        let wrong = local(&source, "0000");
        let refused = wrong.fetch_into(&cache).unwrap_err().to_string();
        assert!(refused.contains("wrong SHA-256"), "{refused}");
        assert!(!wrong.path(&cache).exists());
        let right = local(&source, ABC);
        let path = right.fetch_into(&cache).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "abc");
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "nothing partial is left beside it");
    }
}
