//! `build.json`: the generator's own bookkeeping beside a wiki. While a
//! build runs, in the job's directory, everything written so far, which a
//! resume carries on from; once it's done, in the version's directory,
//! what the wiki was written from: its commit, its outline, and the hash of
//! every file each subsection covers, which says what a sync must write
//! again.
//!
//! Adapted from crystal's wiki generator (`src/wiki/book.rs`, MIT).

use super::check::Tally;
use super::plan::Plan;
use crate::db::{JobKind, Phase};
use crate::time::Timestamp;
use crate::wiki::{Diagram, write_whole};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// What `version` says of `build.json`'s shape.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Book {
    #[serde(default)]
    pub version: u32,
    /// What the wiki beside it was written from, once it's written.
    #[serde(default)]
    pub built: Option<Built>,
    /// The build under way, or stopped before it ended.
    #[serde(default)]
    pub run: Option<Run>,
}

/// What a wiki was written from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Built {
    pub commit: String,
    pub branch: Option<String>,
    pub at: Timestamp,
    pub plan: Plan,
    /// The files each subsection covered, by its id, each with the hash of
    /// its content.
    pub blobs: BTreeMap<String, BTreeMap<String, String>>,
    /// The subsections that couldn't be written, which the next sync
    /// writes.
    #[serde(default)]
    pub missing: BTreeSet<String>,
    /// What writing this version cost, in US dollars.
    pub cost_usd: f64,
    /// The model that wrote the most of it, as Claude Code names it.
    pub model: String,
    /// What checking and linking found and did.
    #[serde(default)]
    pub tally: Tally,
    /// How long it took, in seconds, every run of it together.
    #[serde(default)]
    pub seconds: u64,
}

/// A build under way, or stopped before it ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// A build, from the start, or a sync, from a version.
    pub kind: JobKind,
    pub commit: String,
    pub branch: Option<String>,
    pub started: Timestamp,
    /// The version a sync starts from.
    #[serde(default)]
    pub base: Option<u32>,
    /// The outline: made by its planner, or the version's own.
    #[serde(default)]
    pub plan: Option<Plan>,
    /// Which subsections it writes; every one for a build.
    #[serde(default)]
    pub todo: BTreeSet<String>,
    /// Whether the outline changed from the version's, which has the
    /// overview written again.
    #[serde(default)]
    pub replanned: bool,
    /// The subsections written, by id.
    #[serde(default)]
    pub pages: BTreeMap<String, Page>,
    /// The subsections that couldn't be written, with why.
    #[serde(default)]
    pub failed: BTreeMap<String, String>,
    /// The sections' summaries written, by id.
    #[serde(default)]
    pub sections: BTreeMap<String, Summary>,
    #[serde(default)]
    pub overview: Option<Summary>,
    pub phase: Phase,
    /// What it has cost so far, every run of Claude, in US dollars.
    pub cost_usd: f64,
    /// How long it has taken so far, in seconds, every run of it together.
    #[serde(default)]
    pub seconds: u64,
    /// What checking and linking found and did.
    #[serde(default)]
    pub tally: Tally,
    /// The cost of each model that answered, by its name.
    #[serde(default)]
    pub models: BTreeMap<String, f64>,
}

/// A subsection written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub body_md: String,
    pub diagram: Option<Diagram>,
    /// The files it covered, each with its hash.
    pub blobs: BTreeMap<String, String>,
    /// For a sync, whether what it says changed in meaning.
    pub meaning_changed: bool,
    pub cost_usd: f64,
}

/// A section's summary, or the overview, written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub summary_md: String,
    pub diagram: Option<Diagram>,
    pub meaning_changed: bool,
    pub cost_usd: f64,
}

impl Book {
    /// The book in `path`, or an empty one when there's none.
    pub fn read(path: &Path) -> Result<Book> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("couldn't read {}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Book::default()),
            Err(err) => Err(err).with_context(|| format!("couldn't read {}", path.display())),
        }
    }

    /// Writes it to `path`, in one go.
    pub fn write(&self, path: &Path) -> Result<()> {
        let book = Book {
            version: VERSION,
            ..self.clone()
        };
        write_whole(
            path,
            &serde_json::to_string_pretty(&book).expect("a book makes JSON"),
        )
    }
}

impl Run {
    /// A new run of `kind` at `commit`, from the version `base` for a sync.
    pub fn new(kind: JobKind, commit: &str, branch: Option<String>, base: Option<u32>) -> Run {
        Run {
            kind,
            commit: commit.to_string(),
            branch,
            started: Timestamp::now(),
            base,
            plan: None,
            todo: BTreeSet::new(),
            replanned: false,
            pages: BTreeMap::new(),
            failed: BTreeMap::new(),
            sections: BTreeMap::new(),
            overview: None,
            phase: Phase::Plan,
            cost_usd: 0.0,
            seconds: 0,
            tally: Tally::default(),
            models: BTreeMap::new(),
        }
    }

    /// The model that cost most.
    pub fn top_model(&self) -> Option<String> {
        self.models
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(model, _)| model.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_book_reads_back_as_it_was_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("build.json");
        let book = Book::read(&path).unwrap();
        assert!(book.built.is_none() && book.run.is_none());
        let mut run = Run::new(JobKind::Sync, "c0ffee", Some("main".into()), Some(2));
        run.models.insert("claude-haiku-5-5".into(), 0.1);
        run.models.insert("claude-sonnet-5-5".into(), 2.5);
        run.todo.insert("retries".into());
        Book {
            run: Some(run.clone()),
            ..book
        }
        .write(&path)
        .unwrap();
        let read = Book::read(&path).unwrap();
        assert_eq!(read.version, VERSION);
        assert_eq!(read.run.as_ref(), Some(&run));
        assert_eq!(run.top_model().as_deref(), Some("claude-sonnet-5-5"));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"kind\": \"sync\""), "{text}");
        assert!(text.contains("\"phase\": \"plan\""), "{text}");
        fs::write(&path, "nonsense").unwrap();
        assert!(Book::read(&path).is_err());
    }
}
