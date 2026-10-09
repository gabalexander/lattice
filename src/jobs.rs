//! Jobs: a repository's wiki built, synced, resumed or written again, a
//! version at a time, by the [`Generator`]. The server keeps a queue of
//! them, [`Jobs`]: one job a repository at a time, [`AT_ONCE`]
//! repositories at once, each job told to its pages as it goes through a
//! [`Feed`]; `lattice build` runs one itself. Both run it the same way,
//! with [`run`]:
//!
//! 1. It holds the repository's [`BuildLock`], so no other lattice builds
//!    it meanwhile, from the server or from the command line.
//! 2. It gives the job an empty directory to write in, `work/` (or keeps
//!    the one a build that stopped left there, to resume it), and keeps
//!    there, in `build.log`, every line said along the way.
//! 3. It has the code ready (see [`repos::prepare`]), fetched but for a
//!    resume, which carries on at the commit it started from.
//! 4. It runs the generator, which writes `wiki.json` in `work/`.
//! 5. It makes `work/` the next version, `v<n>/`, and keeps it.
//!
//! A job that fails or is cancelled leaves `work/` for a resume. What a
//! job says is redacted (see [`crate::secrets`]) before it's kept or
//! shown: a page and its log are read by whoever has the page.

use crate::cancel::Cancel;
use crate::config::{self, Config};
use crate::db::{Db, Ended, Job, JobKind, JobState, Progress, Version};
use crate::generator::{Generator, JobSpec, Report};
use crate::paths;
use crate::repos;
use crate::secrets;
use crate::time::Timestamp;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// How many repositories the server builds at once.
pub const AT_ONCE: usize = 3;

/// How often the queue looks for work it wasn't told of: a job another
/// lattice queued, or a repository another lattice stopped building.
const LOOK: Duration = Duration::from_secs(1);

/// The lock a build holds on its repository: a `flock` on
/// `repos/<key>/build.lock`, let go when it's dropped, or when the process
/// holding it ends, however it ends.
pub struct BuildLock {
    /// Open while it's held: the lock goes with it.
    _held: File,
}

impl BuildLock {
    /// The lock on the repository called `key`, unless another build holds
    /// it.
    pub fn take(key: &str) -> Result<Option<BuildLock>> {
        let dir = paths::repo_dir(key);
        fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
        let path = dir.join("build.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("couldn't open {}", path.display()))?;
        // SAFETY: flock has no preconditions; the descriptor is the file's.
        let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        Ok(taken.then_some(BuildLock { _held: file }))
    }
}

/// Whether a build of the repository called `key` is going on, in this
/// lattice or another.
pub fn building(key: &str) -> bool {
    paths::repo_dir(key).is_dir() && !matches!(BuildLock::take(key), Ok(Some(_)))
}

/// Runs job `id`, which is waiting to run, on a repository whose lock is
/// `lock`, with `generator`, telling `tell` how it goes, until it ends,
/// which it says; `cancel` stops it. The job is kept in `db` as it goes.
pub fn run(
    db: &Mutex<Db>,
    id: u64,
    lock: BuildLock,
    generator: &dyn Generator,
    cancel: &Cancel,
    tell: &mut dyn FnMut(Report),
) -> Result<Ended> {
    let job = locked(db)
        .job(id)?
        .with_context(|| format!("there's no job {id}"))?;
    if !locked(db).start_job(id)? {
        bail!("job {id} isn't waiting to run");
    }
    let built = build(db, &job, generator, cancel, tell);
    drop(lock);
    let ended = match built {
        Ok(n) => Ended::Done(n),
        Err(_) if cancel.is_cancelled() => Ended::Cancelled,
        Err(err) => Ended::Failed(secrets::redact(&format!("{err:#}"))),
    };
    locked(db).finish_job(id, &ended)?;
    Ok(ended)
}

/// Steps 2 to 5 of [`run`]: the version made.
fn build(
    db: &Mutex<Db>,
    job: &Job,
    generator: &dyn Generator,
    cancel: &Cancel,
    tell: &mut dyn FnMut(Report),
) -> Result<u32> {
    let repo = locked(db)
        .repo(&job.repo)?
        .with_context(|| format!("there's no repository called {}", job.repo))?;
    let mut config = Config::load()?;
    if let Some(model) = &job.model {
        config.model = model.clone();
    }
    if let Some(concurrency) = job.concurrency {
        config.concurrency = concurrency;
    }
    config.check()?;
    let work = paths::work_dir(&repo.key);
    if job.kind == JobKind::Resume {
        if !work.is_dir() {
            bail!("there's no build to resume: build it instead");
        }
    } else {
        match fs::remove_dir_all(&work) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(err).with_context(|| format!("couldn't clear {}", work.display()));
            }
            _ => {}
        }
        fs::create_dir_all(&work).with_context(|| format!("couldn't make {}", work.display()))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(work.join(paths::LOG_FILE))
        .context("couldn't open the build's log")?;
    let mut told = Told { file, tell };
    told.line(format!(
        "job {}: {} of {}",
        job.id,
        word(job.kind),
        repo.name
    ));
    let checkout = repos::prepare(
        &repo,
        job.kind != JobKind::Resume,
        &mut |line| told.line(line),
        cancel,
    )?;
    let branch = checkout.branch.as_deref().unwrap_or("no branch");
    told.line(format!("at {} on {branch}", checkout.commit));
    let latest = locked(db).versions(&repo.key)?.pop();
    if job.kind == JobKind::Sync && latest.is_none() {
        bail!("there's no version to sync from: build one first");
    }
    let spec = JobSpec {
        id: job.id,
        kind: job.kind,
        root: repos::root(&repo),
        repo,
        config,
        commit: checkout.commit,
        branch: checkout.branch,
        work: work.clone(),
        latest,
    };
    let built = generator.run(
        &spec,
        &mut |report| match report {
            Report::Progress(progress) => {
                let _ = locked(db).set_progress(job.id, &progress);
                (told.tell)(Report::Progress(progress));
            }
            Report::Log(line) => told.line(line),
        },
        cancel,
    );
    let built = match built {
        Ok(built) => built,
        Err(err) => {
            told.line(format!("it stopped: {err:#}"));
            return Err(err);
        }
    };
    let wiki = fs::read(work.join(paths::WIKI_FILE)).context("the generator wrote no wiki.json")?;
    let wiki: serde_json::Value =
        serde_json::from_slice(&wiki).context("the wiki.json the generator wrote isn't JSON")?;
    if !wiki.is_object() {
        bail!("the wiki.json the generator wrote isn't a wiki");
    }
    told.line(format!("done: ${:.2}", built.cost_usd));
    drop(told);
    let key = &spec.repo.key;
    let db = locked(db);
    let n = db.next_version(key)?;
    let dir = paths::version_dir(key, n);
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&work, &dir)
        .with_context(|| format!("couldn't move the build to {}", dir.display()))?;
    db.add_version(
        key,
        &Version {
            n,
            commit: spec.commit,
            branch: spec.branch,
            model: built.model,
            at: Timestamp::now(),
            cost_usd: built.cost_usd,
        },
    )?;
    Ok(n)
}

/// Where what a job says goes: each line into its `build.log`, and all of
/// it to whoever follows the job.
struct Told<'a> {
    file: File,
    tell: &'a mut dyn FnMut(Report),
}

impl Told<'_> {
    /// A line said, its credentials taken out.
    fn line(&mut self, line: String) {
        let line = secrets::redact(&line);
        let _ = writeln!(self.file, "{line}");
        (self.tell)(Report::Log(line));
    }
}

/// The word a job's kind is said with.
pub fn word(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Build => "build",
        JobKind::Sync => "sync",
        JobKind::Resume => "resume",
        JobKind::Regenerate => "regenerate",
    }
}

/// The database, for a moment.
fn locked(db: &Mutex<Db>) -> MutexGuard<'_, Db> {
    db.lock().unwrap_or_else(|err| err.into_inner())
}

/// Why a job can't be asked for: the HTTP status that says so, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub status: u16,
    pub message: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

fn refused(status: u16, message: impl Into<String>) -> Refused {
    Refused {
        status,
        message: message.into(),
    }
}

/// Checks a job of `kind` can be asked for on the repository called `key`,
/// with `model` and `concurrency` when they're given, and asks for it.
pub fn submit(
    db: &Db,
    key: &str,
    kind: JobKind,
    model: Option<&str>,
    concurrency: Option<usize>,
) -> Result<Job, Refused> {
    let internal = |err: anyhow::Error| refused(500, format!("{err:#}"));
    if db.repo(key).map_err(internal)?.is_none() {
        return Err(refused(404, format!("there's no repository called {key}")));
    }
    if let Some(model) = model {
        config::check_model(model).map_err(|err| refused(400, format!("{err:#}")))?;
    }
    if let Some(concurrency) = concurrency
        && !config::CONCURRENCY.contains(&concurrency)
    {
        return Err(refused(
            400,
            format!(
                "concurrency is {concurrency}: it's from {} to {}",
                config::CONCURRENCY.start(),
                config::CONCURRENCY.end()
            ),
        ));
    }
    // A job left running by a lattice that was killed holds nothing now.
    db.interrupt_running(building).map_err(internal)?;
    let jobs = db.jobs(key).map_err(internal)?;
    if let Some(going) = jobs.iter().find(|job| !job.state.is_over()) {
        return Err(refused(
            409,
            format!(
                "job {} is {} for it already: wait for it, or cancel it",
                going.id,
                serde_word(&going.state)
            ),
        ));
    }
    if kind == JobKind::Sync && db.versions(key).map_err(internal)?.is_empty() {
        return Err(refused(
            409,
            "there's no version to sync from: build one first",
        ));
    }
    if kind == JobKind::Resume && !paths::work_dir(key).is_dir() {
        return Err(refused(409, "there's no build to resume: build it instead"));
    }
    db.add_job(key, kind, model, concurrency).map_err(internal)
}

/// How an enum is said, as the API says it.
fn serde_word<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => word,
        _ => String::new(),
    }
}

/// How a job that's over ended, by what's kept of it.
pub fn ended_of(job: &Job) -> Option<Ended> {
    match job.state {
        JobState::Done => Some(Ended::Done(job.version.unwrap_or(0))),
        JobState::Failed => Some(Ended::Failed(
            job.error.clone().unwrap_or_else(|| "it failed".to_string()),
        )),
        JobState::Cancelled => Some(Ended::Cancelled),
        _ => None,
    }
}

/// The log a job kept, as far as it's on disk: its version's for one that
/// made one, or else what's in `work/`.
pub fn kept_log(job: &Job) -> Vec<String> {
    let dir = match job.version {
        Some(n) => paths::version_dir(&job.repo, n),
        None => paths::work_dir(&job.repo),
    };
    fs::read_to_string(dir.join(paths::LOG_FILE))
        .map(|log| log.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// What a job has said, for its pages to follow: how far it got, its log,
/// and how it ended. Each page reads it from the start, then waits for
/// what's new.
#[derive(Default)]
pub struct Feed {
    said: Mutex<Said>,
    changed: Condvar,
}

#[derive(Default)]
struct Said {
    /// Counts every change, for a page to wait past the ones it has seen.
    changes: u64,
    progress: Option<Progress>,
    /// The change the progress was last.
    progress_at: u64,
    log: Vec<String>,
    end: Option<Ended>,
}

/// How far a page has read a [`Feed`].
#[derive(Debug, Default, Clone, Copy)]
pub struct Seen {
    changes: u64,
    lines: usize,
    progress_at: u64,
}

/// What's new on a [`Feed`] since a page last read it.
#[derive(Debug, Default, PartialEq)]
pub struct News {
    pub progress: Option<Progress>,
    pub lines: Vec<String>,
    pub end: Option<Ended>,
}

impl Feed {
    fn change(&self, change: impl FnOnce(&mut Said)) {
        let mut said = self.said.lock().unwrap_or_else(|err| err.into_inner());
        change(&mut said);
        said.changes += 1;
        self.changed.notify_all();
    }

    /// Tells the feed what the job reported.
    pub fn report(&self, report: Report) {
        self.change(|said| match report {
            Report::Progress(progress) => {
                said.progress = Some(progress);
                said.progress_at = said.changes + 1;
            }
            Report::Log(line) => said.log.push(line),
        });
    }

    /// Tells the feed how the job ended.
    pub fn end(&self, ended: Ended) {
        self.change(|said| said.end = Some(ended));
    }

    /// What's new since `seen`, which it moves on, waiting up to `wait` for
    /// something to be: nothing, when nothing came.
    pub fn next(&self, seen: &mut Seen, wait: Duration) -> News {
        let said = self.said.lock().unwrap_or_else(|err| err.into_inner());
        let (said, _) = self
            .changed
            .wait_timeout_while(said, wait, |said| said.changes <= seen.changes)
            .unwrap_or_else(|err| err.into_inner());
        let news = News {
            progress: (said.progress_at > seen.progress_at)
                .then(|| said.progress.clone())
                .flatten(),
            lines: said.log[seen.lines.min(said.log.len())..].to_vec(),
            end: said.end.clone(),
        };
        *seen = Seen {
            changes: said.changes,
            lines: said.log.len(),
            progress_at: said.progress_at,
        };
        news
    }
}

/// The server's queue of jobs.
#[derive(Clone)]
pub struct Jobs {
    inner: Arc<Inner>,
}

struct Inner {
    db: Arc<Mutex<Db>>,
    generator: Arc<dyn Generator>,
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    /// The jobs running, by their numbers, with their repositories' keys.
    running: HashMap<u64, (String, Cancel)>,
    /// The feeds of the jobs not over, which their pages follow.
    feeds: HashMap<u64, Arc<Feed>>,
    stopping: bool,
}

impl Jobs {
    /// The queue, running the jobs `db` has waiting with `generator`. The
    /// jobs a lattice that stopped left running have failed, unless
    /// another lattice is still building their repositories.
    pub fn start(db: Arc<Mutex<Db>>, generator: Arc<dyn Generator>) -> Result<Jobs> {
        locked(&db).interrupt_running(building)?;
        let jobs = Jobs {
            inner: Arc::new(Inner {
                db,
                generator,
                state: Mutex::new(State::default()),
                wake: Condvar::new(),
            }),
        };
        let queue = jobs.clone();
        thread::spawn(move || queue.dispatch());
        Ok(jobs)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    /// Asks for a job, as [`submit`] does, and has the queue run it.
    pub fn submit(
        &self,
        key: &str,
        kind: JobKind,
        model: Option<&str>,
        concurrency: Option<usize>,
    ) -> Result<Job, Refused> {
        let job = submit(&locked(&self.inner.db), key, kind, model, concurrency)?;
        self.state().feeds.insert(job.id, Arc::new(Feed::default()));
        self.inner.wake.notify_all();
        Ok(job)
    }

    /// Stops job `id`: one running is cancelled, and ends as soon as it
    /// stops; one waiting never runs. The job, as it is then.
    pub fn cancel(&self, id: u64) -> Result<Option<Job>> {
        let db = &self.inner.db;
        let Some(job) = locked(db).job(id)? else {
            return Ok(None);
        };
        let mut state = self.state();
        if let Some((_, cancel)) = state.running.get(&id) {
            cancel.cancel();
        } else if job.state == JobState::Queued {
            locked(db).finish_job(id, &Ended::Cancelled)?;
            if let Some(feed) = state.feeds.remove(&id) {
                feed.end(Ended::Cancelled);
            }
        }
        drop(state);
        locked(db).job(id)
    }

    /// Cancels the job on the repository called `key`, if there's one, and
    /// waits up to `wait` for it to stop: whether none runs on it then.
    pub fn stop_repo(&self, key: &str, wait: Duration) -> Result<bool> {
        let going: Vec<u64> = locked(&self.inner.db)
            .jobs(key)?
            .iter()
            .filter(|job| !job.state.is_over())
            .map(|job| job.id)
            .collect();
        for id in going {
            self.cancel(id)?;
        }
        let state = self.state();
        let (state, _) = self
            .inner
            .wake
            .wait_timeout_while(state, wait, |state| {
                state.running.values().any(|(repo, _)| repo == key)
            })
            .unwrap_or_else(|err| err.into_inner());
        let idle = !state.running.values().any(|(repo, _)| repo == key);
        Ok(idle && !building(key))
    }

    /// The feed of job `id`, while it's not over and this queue has it.
    pub fn feed(&self, id: u64) -> Option<Arc<Feed>> {
        self.state().feeds.get(&id).cloned()
    }

    /// Stops the queue: the jobs running are cancelled, and waited for up
    /// to `wait`.
    pub fn stop(&self, wait: Duration) {
        let mut state = self.state();
        state.stopping = true;
        for (_, cancel) in state.running.values() {
            cancel.cancel();
        }
        self.inner.wake.notify_all();
        let _ = self
            .inner
            .wake
            .wait_timeout_while(state, wait, |state| !state.running.is_empty());
    }

    /// The queue's own thread: it starts what's waiting, as far as it may,
    /// whenever it's told something changed, and every [`LOOK`] besides; and
    /// fails the jobs a lattice that was killed left running, whose
    /// repositories nobody builds.
    fn dispatch(&self) {
        loop {
            let queued = {
                let db = locked(&self.inner.db);
                let _ = db.interrupt_running(building);
                db.queued_jobs().unwrap_or_default()
            };
            let mut state = self.state();
            if state.stopping {
                return;
            }
            for job in queued {
                if state.running.len() >= AT_ONCE {
                    break;
                }
                if state.running.values().any(|(repo, _)| *repo == job.repo) {
                    continue;
                }
                let Ok(Some(lock)) = BuildLock::take(&job.repo) else {
                    continue;
                };
                let cancel = Cancel::new();
                let feed = state.feeds.entry(job.id).or_default().clone();
                state
                    .running
                    .insert(job.id, (job.repo.clone(), cancel.clone()));
                let queue = self.clone();
                thread::spawn(move || queue.work(job.id, lock, cancel, feed));
            }
            // Woken by a job asked for or ended, or else after a while.
            let _ = self.inner.wake.wait_timeout(state, LOOK);
        }
    }

    /// Runs job `id` on a thread of its own, telling `feed`.
    fn work(&self, id: u64, lock: BuildLock, cancel: Cancel, feed: Arc<Feed>) {
        let inner = &self.inner;
        let ended = run(
            &inner.db,
            id,
            lock,
            inner.generator.as_ref(),
            &cancel,
            &mut |report| feed.report(report),
        );
        let ended = ended.unwrap_or_else(|err| {
            let why = format!("{err:#}");
            // It couldn't even be kept: say so wherever it can be.
            let _ = locked(&inner.db).finish_job(id, &Ended::Failed(why.clone()));
            Ended::Failed(why)
        });
        feed.end(ended);
        let mut state = self.state();
        state.running.remove(&id);
        state.feeds.remove(&id);
        inner.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn a_feed_tells_each_page_everything_then_what_s_new() {
        let feed = Feed::default();
        feed.report(Report::Log("cloning".into()));
        let progress = Progress {
            phase: crate::db::Phase::Write,
            done: 1,
            total: 4,
            current: Some("Start".into()),
            cost_usd: 0.5,
        };
        feed.report(Report::Progress(progress.clone()));
        feed.report(Report::Log("writing".into()));
        let mut seen = Seen::default();
        let news = feed.next(&mut seen, Duration::ZERO);
        assert_eq!(
            news,
            News {
                progress: Some(progress.clone()),
                lines: vec!["cloning".into(), "writing".into()],
                end: None
            }
        );
        let started = Instant::now();
        assert_eq!(
            feed.next(&mut seen, Duration::from_millis(50)),
            News::default()
        );
        assert!(started.elapsed() >= Duration::from_millis(40), "it waited");
        feed.report(Report::Log("linking".into()));
        feed.end(Ended::Done(2));
        let news = feed.next(&mut seen, Duration::from_secs(5));
        assert_eq!(news.progress, None, "the progress hasn't changed");
        assert_eq!(news.lines, ["linking"]);
        assert_eq!(news.end, Some(Ended::Done(2)));
        let mut again = Seen::default();
        assert_eq!(feed.next(&mut again, Duration::ZERO).lines.len(), 3);
    }

    #[test]
    fn a_page_waiting_hears_of_a_change_at_once() {
        let feed = Arc::new(Feed::default());
        let later = feed.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            later.end(Ended::Cancelled);
        });
        let started = Instant::now();
        let news = feed.next(&mut Seen::default(), Duration::from_secs(20));
        assert_eq!(news.end, Some(Ended::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
