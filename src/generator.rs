//! What writes a wiki: the [`Generator`] a job runs (see [`crate::jobs`]),
//! given the repository's code ready at a commit and a directory to write
//! in, telling how far it has got as it goes.
//!
//! The job does everything around it: it prepares the code, gives the
//! generator an empty directory to write in (or, to resume, the one a
//! build that stopped left), writes each [`Report::Log`] line into its
//! `build.log`, and once the generator is done, makes the directory a
//! version of the wiki. The generator writes `wiki.json` there, and its
//! own bookkeeping, `build.json`, for a resume or a sync to carry on from.

use crate::cancel::Cancel;
use crate::config::Config;
use crate::db::{JobKind, Progress, Repo, Version};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

/// What a job asks of the generator.
#[derive(Debug, Clone)]
pub struct JobSpec {
    /// The job's number.
    pub id: u64,
    pub kind: JobKind,
    pub repo: Repo,
    /// The settings, with the job's own model and concurrency over them.
    pub config: Config,
    /// Where the repository's code is, ready at `commit`.
    pub root: PathBuf,
    pub commit: String,
    pub branch: Option<String>,
    /// Where it writes: empty for a build, a regeneration and a sync, and
    /// as the build that stopped left it for a resume.
    pub work: PathBuf,
    /// The latest version of the wiki, whose files are in
    /// `paths::version_dir(key, n)`: what a sync starts from.
    pub latest: Option<Version>,
}

/// What a generator says as it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Report {
    /// How far it has got: the job's `progress` event.
    Progress(Progress),
    /// A line for the job's log, and its `log` event.
    Log(String),
}

/// What a generator made, once `wiki.json` is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// The model that wrote it, as Claude Code names it.
    pub model: String,
    pub cost_usd: f64,
}

/// What writes a wiki.
pub trait Generator: Send + Sync {
    /// Writes `job`'s wiki into `job.work`, telling `report` how it goes,
    /// until it's done, it fails, or `cancel` says to stop: an error then,
    /// which `cancel.is_cancelled()` tells from a failure.
    fn run(&self, job: &JobSpec, report: &mut dyn FnMut(Report), cancel: &Cancel) -> Result<Built>;
}

/// The generator this lattice has: Claude Code's ([`crate::r#gen`]).
pub fn current() -> Arc<dyn Generator> {
    Arc::new(crate::r#gen::Claude)
}
