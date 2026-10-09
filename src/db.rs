//! The database lattice keeps what it knows in: one SQLite file in its data
//! directory (see [`crate::paths`]) holding the repositories it was given,
//! the versions of each one's wiki, and the jobs that build them. The
//! versions' own files, the wiki and its build's bookkeeping and log, are
//! on disk beside it, under `repos/<key>/v<n>/`; the settings are in the
//! config file, which people edit by hand.
//!
//! It's opened in WAL mode, so readers never wait on a writer, and each
//! write that's more than one statement is a transaction, so one cut short
//! by a crash leaves what was there before, never half of it. A database
//! that can't be read is an error, not a fresh start that would write over
//! it. Its tables are made, and later added to, by [`MIGRATIONS`], a step
//! for each version, kept in its `user_version`.
//!
//! Adapted from crystal's `src/db.rs` (MIT).

use crate::paths;
use crate::source::{self, Source};
use crate::time::Timestamp;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// How long a write waits for another to finish: the server and the
/// commands share the one database.
const BUSY_WAIT: Duration = Duration::from_secs(5);

/// The tables as they were first. A repository is kept by its key, which
/// is its page's address, with where its code is: `kind` `local` and the
/// path, or `git` and the URL. A version is numbered from 1 within its
/// repository, never given again while the repository is kept. A job's
/// progress is JSON, as the API says it.
const TABLES: &str = "
CREATE TABLE repos (
  key      TEXT PRIMARY KEY,
  name     TEXT NOT NULL,
  kind     TEXT NOT NULL,
  location TEXT NOT NULL,
  added    INTEGER NOT NULL,
  UNIQUE (kind, location)
);
CREATE TABLE versions (
  repo       TEXT NOT NULL REFERENCES repos(key) ON DELETE CASCADE,
  n          INTEGER NOT NULL,
  commit_sha TEXT NOT NULL,
  branch     TEXT,
  model      TEXT NOT NULL,
  at         INTEGER NOT NULL,
  cost_usd   REAL NOT NULL,
  PRIMARY KEY (repo, n)
);
CREATE TABLE jobs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  repo        TEXT NOT NULL REFERENCES repos(key) ON DELETE CASCADE,
  kind        TEXT NOT NULL,
  model       TEXT,
  concurrency INTEGER,
  state       TEXT NOT NULL,
  progress    TEXT,
  error       TEXT,
  version     INTEGER,
  created     INTEGER NOT NULL,
  started     INTEGER,
  finished    INTEGER
);
CREATE INDEX jobs_repo ON jobs(repo, id);
CREATE INDEX jobs_state ON jobs(state, id);
";

/// What makes the database as it is now, a step for each version: a
/// database at version `v`, kept in its `user_version`, takes the steps
/// after the first `v`.
const MIGRATIONS: &[&str] = &[TABLES];

const REPO_COLUMNS: &str = "key, name, kind, location, added";
const VERSION_COLUMNS: &str = "n, commit_sha, branch, model, at, cost_usd";
const JOB_COLUMNS: &str = "id, repo, kind, model, concurrency, state, progress, error, version, \
                           created, started, finished";

/// What a job left running says once it can't be any more: the server that
/// ran it stopped.
pub const INTERRUPTED: &str = "the server stopped while it ran: resume it to carry on";

/// A repository lattice was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repo {
    /// Its page's address, and the name of its directory.
    pub key: String,
    /// As people say it: `owner/repo`, or its directory's name.
    pub name: String,
    pub source: Source,
    pub added: Timestamp,
}

/// A version of a repository's wiki: what one build made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Version {
    /// Its number, from 1.
    pub n: u32,
    /// The commit it was written at.
    pub commit: String,
    pub branch: Option<String>,
    /// The model that wrote it, as Claude Code names it.
    pub model: String,
    /// When it was finished.
    pub at: Timestamp,
    pub cost_usd: f64,
}

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    /// The first version of a wiki, or one written again as if the first.
    Build,
    /// A new version from the latest one: the pages whose code changed
    /// since its commit written again.
    Sync,
    /// A build that failed or was stopped carried on from where it got.
    Resume,
    /// A new version written from the start, the latest one aside.
    Regenerate,
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    /// Whether it's over, one way or another.
    pub fn is_over(self) -> bool {
        matches!(
            self,
            JobState::Done | JobState::Failed | JobState::Cancelled
        )
    }
}

/// What a build is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Reading the repository and planning the page's sections.
    Plan,
    /// Writing the subsections.
    Write,
    /// Linking the names in them to the code.
    Link,
    /// Writing the overview and each section's summary.
    Overview,
}

/// How far a build has got, as its job's events say it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub phase: Phase,
    /// How many of the phase's steps are done, of how many.
    pub done: u32,
    pub total: u32,
    /// What it's doing now: the title of the subsection being written.
    pub current: Option<String>,
    /// What the build has spent so far.
    pub cost_usd: f64,
}

/// A job: a build of a repository's wiki, asked for, going on or over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    /// The key of the repository it builds.
    pub repo: String,
    pub kind: JobKind,
    /// The model it was asked to build with, when not the settings'.
    pub model: Option<String>,
    /// How many writers at once, when not the settings' number.
    pub concurrency: Option<usize>,
    pub state: JobState,
    /// How far it got, once it's said.
    pub progress: Option<Progress>,
    /// Why it failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The version it made, once it's done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    pub created: Timestamp,
    pub started: Option<Timestamp>,
    pub finished: Option<Timestamp>,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// It made version `n`.
    Done(u32),
    Failed(String),
    Cancelled,
}

/// The database, open.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// The database in lattice's data directory, made if it isn't there.
    pub fn open() -> Result<Db> {
        Db::open_at(&paths::db_file())
    }

    /// The database in `file`, made if it isn't there, and brought up to
    /// date.
    pub fn open_at(file: &Path) -> Result<Db> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).with_context(|| format!("couldn't make {}", dir.display()))?;
        }
        let mut conn =
            Connection::open(file).with_context(|| format!("couldn't open {}", file.display()))?;
        conn.busy_timeout(BUSY_WAIT)?;
        // Readers then never wait on a writer, nor a writer on them, and a
        // commit is written once, to the log.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn).with_context(|| format!("couldn't set up {}", file.display()))?;
        Ok(Db { conn })
    }

    /// Adds the repository at `source`, under a key of its own, and gives it
    /// back; or the one there already, when it was added before.
    pub fn add_repo(&mut self, source: &Source) -> Result<Repo> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (kind, location) = columns_of(source);
        let known = tx
            .query_row(
                &format!("SELECT {REPO_COLUMNS} FROM repos WHERE kind = ?1 AND location = ?2"),
                params![kind, location],
                repo_of,
            )
            .optional()?;
        if let Some(repo) = known {
            return repo;
        }
        let name = source.name();
        let key = {
            let mut taken = tx.prepare("SELECT 1 FROM repos WHERE key = ?1")?;
            source::key_for(&name, |key| taken.exists([key]).unwrap_or(true))
        };
        let repo = Repo {
            key,
            name,
            source: source.clone(),
            added: Timestamp::now(),
        };
        tx.execute(
            "INSERT INTO repos (key, name, kind, location, added) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo.key, repo.name, kind, location, repo.added.0],
        )?;
        tx.commit()?;
        Ok(repo)
    }

    /// Every repository, by name.
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {REPO_COLUMNS} FROM repos ORDER BY name, key"
        ))?;
        let rows = statement.query_map([], repo_of)?;
        rows.map(|row| row?).collect()
    }

    /// The repository called `key`.
    pub fn repo(&self, key: &str) -> Result<Option<Repo>> {
        self.conn
            .query_row(
                &format!("SELECT {REPO_COLUMNS} FROM repos WHERE key = ?1"),
                [key],
                repo_of,
            )
            .optional()?
            .transpose()
    }

    /// Forgets the repository called `key`, its versions and its jobs:
    /// whether there was one. Its files on disk are its caller's to remove.
    pub fn remove_repo(&self, key: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM repos WHERE key = ?1", [key])?
            > 0)
    }

    /// The number the next version of `repo`'s wiki gets.
    pub fn next_version(&self, repo: &str) -> Result<u32> {
        let last: Option<u32> = self.conn.query_row(
            "SELECT max(n) FROM versions WHERE repo = ?1",
            [repo],
            |row| row.get(0),
        )?;
        Ok(last.unwrap_or(0) + 1)
    }

    /// Keeps `version` of `repo`'s wiki, its files in place.
    pub fn add_version(&self, repo: &str, version: &Version) -> Result<()> {
        self.conn.execute(
            "INSERT INTO versions (repo, n, commit_sha, branch, model, at, cost_usd)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                repo,
                version.n,
                version.commit,
                version.branch,
                version.model,
                version.at.0,
                version.cost_usd
            ],
        )?;
        Ok(())
    }

    /// The versions of `repo`'s wiki, the first first.
    pub fn versions(&self, repo: &str) -> Result<Vec<Version>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {VERSION_COLUMNS} FROM versions WHERE repo = ?1 ORDER BY n"
        ))?;
        let rows = statement.query_map([repo], version_of)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Asks for a job of `kind` on `repo`, queued until it can run.
    pub fn add_job(
        &self,
        repo: &str,
        kind: JobKind,
        model: Option<&str>,
        concurrency: Option<usize>,
    ) -> Result<Job> {
        self.conn.execute(
            "INSERT INTO jobs (repo, kind, model, concurrency, state, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                repo,
                word(&kind)?,
                model,
                concurrency,
                word(&JobState::Queued)?,
                Timestamp::now().0
            ],
        )?;
        let id = self.conn.last_insert_rowid() as u64;
        self.job(id)?.context("the job just added")
    }

    /// The job numbered `id`.
    pub fn job(&self, id: u64) -> Result<Option<Job>> {
        self.conn
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"),
                [id],
                job_of,
            )
            .optional()?
            .transpose()
    }

    /// The jobs on `repo`, the latest first.
    pub fn jobs(&self, repo: &str) -> Result<Vec<Job>> {
        self.jobs_where("repo = ?1 ORDER BY id DESC", repo)
    }

    /// The jobs waiting to run, the first asked for first.
    pub fn queued_jobs(&self) -> Result<Vec<Job>> {
        self.jobs_where("state = ?1 ORDER BY id", &word(&JobState::Queued)?)
    }

    fn jobs_where(&self, condition: &str, value: &str) -> Result<Vec<Job>> {
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE {condition}"))?;
        let rows = statement.query_map([value], job_of)?;
        rows.map(|row| row?).collect()
    }

    /// Has job `id` running, from now.
    pub fn start_job(&self, id: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE jobs SET state = ?2, started = ?3 WHERE id = ?1",
            params![id, word(&JobState::Running)?, Timestamp::now().0],
        )?;
        Ok(())
    }

    /// Keeps how far job `id` has got.
    pub fn set_progress(&self, id: u64, progress: &Progress) -> Result<()> {
        self.conn.execute(
            "UPDATE jobs SET progress = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(progress)?],
        )?;
        Ok(())
    }

    /// Has job `id` over, as `ended` says, from now.
    pub fn finish_job(&self, id: u64, ended: &Ended) -> Result<()> {
        let (state, error, version) = match ended {
            Ended::Done(n) => (JobState::Done, None, Some(*n)),
            Ended::Failed(why) => (JobState::Failed, Some(why.as_str()), None),
            Ended::Cancelled => (JobState::Cancelled, None, None),
        };
        self.conn.execute(
            "UPDATE jobs SET state = ?2, error = ?3, version = ?4, finished = ?5 WHERE id = ?1",
            params![id, word(&state)?, error, version, Timestamp::now().0],
        )?;
        Ok(())
    }

    /// Has every job left running, by a server that stopped, failed with
    /// [`INTERRUPTED`]: how many there were. A server runs this as it
    /// starts, before it runs any.
    pub fn interrupt_running(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE jobs SET state = ?1, error = ?2, finished = ?3 WHERE state = ?4",
            params![
                word(&JobState::Failed)?,
                INTERRUPTED,
                Timestamp::now().0,
                word(&JobState::Running)?
            ],
        )?)
    }
}

/// Brings the database up to date: makes its tables, or adds what a newer
/// lattice keeps.
fn migrate(conn: &mut Connection) -> Result<()> {
    let version = |conn: &Connection| -> rusqlite::Result<usize> {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
    };
    if version(conn)? >= MIGRATIONS.len() {
        return Ok(());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Another process may have got here first.
    let done = version(&tx)?;
    for step in MIGRATIONS.iter().skip(done) {
        tx.execute_batch(step)?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {}", MIGRATIONS.len()))?;
    tx.commit()?;
    Ok(())
}

/// A source's `kind` and `location` columns.
fn columns_of(source: &Source) -> (&'static str, String) {
    match source {
        Source::Local { path } => ("local", path.to_string_lossy().into_owned()),
        Source::Git { url } => ("git", url.clone()),
    }
}

/// The word an enum is written as, in the database as in the API.
fn word<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(word) => Ok(word),
        other => anyhow::bail!("{other} isn't a word"),
    }
}

/// The enum `text` is the word of.
fn from_word<T: for<'de> Deserialize<'de>>(text: String) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(text.clone()))
        .with_context(|| format!("{text} isn't one lattice knows"))
}

fn repo_of(row: &Row) -> rusqlite::Result<Result<Repo>> {
    let (key, name, kind, location, added): (String, String, String, String, u64) = (
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    );
    let source = match kind.as_str() {
        "local" => Ok(Source::Local {
            path: location.into(),
        }),
        "git" => Ok(Source::Git { url: location }),
        other => Err(anyhow::anyhow!(
            "{key}'s kind is {other}, which lattice doesn't know"
        )),
    };
    Ok(source.map(|source| Repo {
        key,
        name,
        source,
        added: Timestamp(added),
    }))
}

fn version_of(row: &Row) -> rusqlite::Result<Version> {
    Ok(Version {
        n: row.get(0)?,
        commit: row.get(1)?,
        branch: row.get(2)?,
        model: row.get(3)?,
        at: Timestamp(row.get(4)?),
        cost_usd: row.get(5)?,
    })
}

fn job_of(row: &Row) -> rusqlite::Result<Result<Job>> {
    let progress: Option<String> = row.get(6)?;
    let job = (|| -> Result<Job> {
        Ok(Job {
            id: row.get(0)?,
            repo: row.get(1)?,
            kind: from_word(row.get(2)?)?,
            model: row.get(3)?,
            concurrency: row.get(4)?,
            state: from_word(row.get(5)?)?,
            progress: progress.as_deref().map(serde_json::from_str).transpose()?,
            error: row.get(7)?,
            version: row.get(8)?,
            created: Timestamp(row.get(9)?),
            started: row.get::<_, Option<u64>>(10)?.map(Timestamp),
            finished: row.get::<_, Option<u64>>(11)?.map(Timestamp),
        })
    })();
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn db_in(dir: &tempfile::TempDir) -> Db {
        Db::open_at(&dir.path().join("lattice.db")).unwrap()
    }

    fn git(url: &str) -> Source {
        Source::Git { url: url.into() }
    }

    #[test]
    fn a_repository_is_kept_under_a_key_of_its_own_and_added_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = db_in(&dir);
        let go = db
            .add_repo(&git("https://github.com/golang/go.git"))
            .unwrap();
        assert_eq!((go.key.as_str(), go.name.as_str()), ("go", "golang/go"));
        let again = db
            .add_repo(&git("https://github.com/golang/go.git"))
            .unwrap();
        assert_eq!(again, go, "the same source is the same repository");
        let fork = db
            .add_repo(&git("git@git.example.com:acme/go.git"))
            .unwrap();
        assert_eq!(fork.key, "acme-go");
        let local = Source::Local {
            path: PathBuf::from("/code/app"),
        };
        let app = db.add_repo(&local).unwrap();
        assert_eq!((app.key.as_str(), app.name.as_str()), ("app", "app"));
        let names: Vec<String> = db.repos().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["acme/go", "app", "golang/go"]);
        assert_eq!(db.repo("app").unwrap().unwrap().source, local);
        assert_eq!(db.repo("nothing").unwrap(), None);
    }

    #[test]
    fn versions_are_numbered_from_one_and_kept_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = db_in(&dir);
        let repo = db.add_repo(&git("https://example.com/app.git")).unwrap();
        assert_eq!(db.next_version(&repo.key).unwrap(), 1);
        for (commit, model) in [("abc", "claude-sonnet-5-5"), ("def", "claude-opus-5-5")] {
            let n = db.next_version(&repo.key).unwrap();
            let version = Version {
                n,
                commit: commit.into(),
                branch: Some("main".into()),
                model: model.into(),
                at: Timestamp(1_791_547_200 + u64::from(n)),
                cost_usd: 4.25,
            };
            db.add_version(&repo.key, &version).unwrap();
        }
        let versions = db.versions(&repo.key).unwrap();
        let kept: Vec<(u32, &str, &str)> = versions
            .iter()
            .map(|v| (v.n, v.commit.as_str(), v.model.as_str()))
            .collect();
        assert_eq!(
            kept,
            [
                (1, "abc", "claude-sonnet-5-5"),
                (2, "def", "claude-opus-5-5")
            ]
        );
        let json = serde_json::to_value(&versions[0]).unwrap();
        assert_eq!(json["at"], "2026-10-09T12:00:01Z");
    }

    #[test]
    fn a_job_is_queued_runs_says_how_far_it_got_and_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = db_in(&dir);
        let repo = db.add_repo(&git("https://example.com/app.git")).unwrap();
        let job = db
            .add_job(&repo.key, JobKind::Build, Some("opus"), Some(2))
            .unwrap();
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(
            (job.model.as_deref(), job.concurrency),
            (Some("opus"), Some(2))
        );
        let other = db.add_job(&repo.key, JobKind::Sync, None, None).unwrap();
        let queued: Vec<u64> = db.queued_jobs().unwrap().iter().map(|j| j.id).collect();
        assert_eq!(queued, [job.id, other.id]);
        db.start_job(job.id).unwrap();
        let progress = Progress {
            phase: Phase::Write,
            done: 12,
            total: 64,
            current: Some("Handing the daemon over".into()),
            cost_usd: 1.2,
        };
        db.set_progress(job.id, &progress).unwrap();
        let running = db.job(job.id).unwrap().unwrap();
        assert_eq!(running.state, JobState::Running);
        assert!(running.started.is_some() && running.finished.is_none());
        assert_eq!(running.progress, Some(progress));
        db.finish_job(job.id, &Ended::Done(1)).unwrap();
        let done = db.job(job.id).unwrap().unwrap();
        assert_eq!((done.state, done.version), (JobState::Done, Some(1)));
        assert!(done.state.is_over() && done.finished.is_some());
        let json = serde_json::to_value(&done).unwrap();
        assert_eq!(json["kind"], "build");
        assert_eq!(json["state"], "done");
        assert_eq!(json["progress"]["phase"], "write");
        assert_eq!(json["version"], 1);
        assert!(json.get("error").is_none());
        db.finish_job(other.id, &Ended::Failed("no".into()))
            .unwrap();
        let failed = db.job(other.id).unwrap().unwrap();
        assert_eq!(failed.error.as_deref(), Some("no"));
        let ids: Vec<u64> = db.jobs(&repo.key).unwrap().iter().map(|j| j.id).collect();
        assert_eq!(ids, [other.id, job.id], "the latest first");
    }

    #[test]
    fn a_job_left_running_by_a_server_that_stopped_has_failed() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = db_in(&dir);
        let repo = db.add_repo(&git("https://example.com/app.git")).unwrap();
        let running = db.add_job(&repo.key, JobKind::Build, None, None).unwrap();
        let queued = db.add_job(&repo.key, JobKind::Sync, None, None).unwrap();
        db.start_job(running.id).unwrap();
        drop(db);
        let db = db_in(&dir);
        assert_eq!(db.interrupt_running().unwrap(), 1);
        let interrupted = db.job(running.id).unwrap().unwrap();
        assert_eq!(interrupted.state, JobState::Failed);
        assert_eq!(interrupted.error.as_deref(), Some(INTERRUPTED));
        assert_eq!(db.job(queued.id).unwrap().unwrap().state, JobState::Queued);
    }

    #[test]
    fn a_repository_removed_takes_its_versions_and_jobs_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = db_in(&dir);
        let repo = db.add_repo(&git("https://example.com/app.git")).unwrap();
        let job = db.add_job(&repo.key, JobKind::Build, None, None).unwrap();
        let version = Version {
            n: 1,
            commit: "abc".into(),
            branch: None,
            model: "claude-sonnet-5-5".into(),
            at: Timestamp(0),
            cost_usd: 0.0,
        };
        db.add_version(&repo.key, &version).unwrap();
        assert!(db.remove_repo(&repo.key).unwrap());
        assert!(!db.remove_repo(&repo.key).unwrap());
        assert_eq!(db.job(job.id).unwrap(), None);
        assert!(db.versions(&repo.key).unwrap().is_empty());
        let again = db.add_repo(&git("https://example.com/app.git")).unwrap();
        assert_eq!(db.next_version(&again.key).unwrap(), 1);
    }

    #[test]
    fn opening_it_again_keeps_what_it_holds() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = db_in(&dir);
        db.add_repo(&git("https://example.com/app.git")).unwrap();
        drop(db);
        let db = db_in(&dir);
        assert_eq!(db.repos().unwrap().len(), 1);
        let version: usize = db
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len());
    }
}
