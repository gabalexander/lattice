//! `lattice serve` and `lattice open`: the web app and its API, on
//! 127.0.0.1 only. The API is the repositories (added by a path, a git
//! URL or GitHub's `owner/repo`), their wikis' versions, the jobs that
//! build them, followed as server-sent events, the chat, opening a file in
//! the user's editor, and the settings; `docs/server.md` lists it. Every
//! other address is the web app's (see [`crate::web`]), and
//! `/assets/mermaid.min.js` is mermaid, from lattice's cache.
//!
//! Nothing but those is read: a repository is named by its key, among
//! those in the database, a job by its number, and a file to open must be
//! in the repository; the checks of [`crate::http`] keep other sites out.
//!
//! One server runs a data directory at a time, holding `serve.lock` in it,
//! and says where it is in `serve.json` beside it, which `lattice open`
//! reads: it starts one in the background when none runs, cut loose from
//! the terminal and logging into `serve.log`, and stops one of another
//! lattice, after an upgrade, to start its own.
//!
//! Adapted from crystal's `src/wiki_server.rs` (MIT).

use crate::ask::{self, Question};
use crate::clipboard;
use crate::config::Config;
use crate::db::{Db, Ended, Job, JobKind, Progress, Repo};
use crate::download::{Fetched, MERMAID};
use crate::editor;
use crate::generator::{self, Generator};
use crate::http::{self, Reply, Request};
use crate::jobs::{self, BuildLock, Feed, Jobs, Seen};
use crate::links;
use crate::paths;
use crate::repos;
use crate::signals;
use crate::source::{self, Source};
use crate::web;
use crate::{errln, outln};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The port a server takes when it's free: another is chosen when it isn't.
pub const DEFAULT_PORT: u16 = 7347;

/// Where a server says where it is, in the data directory.
const SERVING_FILE: &str = "serve.json";

/// What the server running holds, in the data directory.
const LOCK_FILE: &str = "serve.lock";

/// What a server in the background writes, in the data directory.
const LOG_FILE: &str = "serve.log";

/// How long a connection may take to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a write to a page may hang before the page is taken as gone.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a job's events wait for news before telling the page they're
/// still there, which finds a page that went away.
const QUIET: Duration = Duration::from_secs(15);

/// How often a job another lattice runs is looked at again, for its events.
const POLL: Duration = Duration::from_secs(1);

/// The least time between two fetches of a repository for its status.
const FETCH_EVERY: Duration = Duration::from_secs(5 * 60);

/// How long the jobs running get to stop as the server does.
const STOP_WAIT: Duration = Duration::from_secs(10);

/// This lattice's version, which a server says so `open` knows its own.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A server running, as `serve.json` says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Serving {
    pid: u32,
    port: u16,
    lattice: String,
}

/// What a server serves from.
struct Server {
    db: Arc<Mutex<Db>>,
    jobs: Jobs,
    port: u16,
    mermaid: Fetched,
    /// When each repository was last fetched for its status.
    fetched: Mutex<HashMap<String, Instant>>,
}

/// A server started by [`start`], until it's stopped.
pub struct Running {
    server: Arc<Server>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Running {
    pub fn port(&self) -> u16 {
        self.server.port
    }

    /// Stops it: it takes no more requests, and the jobs running are
    /// cancelled, and waited for a while.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Its listener waits for a connection: this is the last one.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.server.port));
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        self.server.jobs.stop(STOP_WAIT);
    }
}

/// Starts serving on 127.0.0.1, on `port`, or else [`DEFAULT_PORT`] or a
/// free one when that's taken, running jobs with `generator`, until it's
/// stopped.
pub fn start(port: Option<u16>, generator: Arc<dyn Generator>) -> Result<Running> {
    let listener = bind(port)?;
    let port = listener.local_addr()?.port();
    let db = Arc::new(Mutex::new(Db::open()?));
    let jobs = Jobs::start(db.clone(), generator)?;
    let server = Arc::new(Server {
        db,
        jobs,
        port,
        mermaid: Fetched::new(&MERMAID),
        fetched: Mutex::new(HashMap::new()),
    });
    // Ready before the first page asks for it.
    let early = server.clone();
    thread::spawn(move || {
        if let Err(why) = early.mermaid.get() {
            errln!("the diagrams are left as their source: {why}");
        }
    });
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let serving = server.clone();
    let accept = thread::spawn(move || {
        for stream in listener.incoming() {
            if stopping.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let server = serving.clone();
            thread::spawn(move || server.handle(stream));
        }
    });
    Ok(Running {
        server,
        stop,
        accept: Some(accept),
    })
}

/// A listener on 127.0.0.1: on `port` if it's given, or else on
/// [`DEFAULT_PORT`], or a free port when that's taken.
fn bind(port: Option<u16>) -> Result<TcpListener> {
    let at = |port| TcpListener::bind((Ipv4Addr::LOCALHOST, port));
    match port {
        Some(port) => at(port).with_context(|| format!("couldn't listen on port {port}")),
        None => at(DEFAULT_PORT)
            .or_else(|_| at(0))
            .context("couldn't listen on 127.0.0.1"),
    }
}

/// `lattice serve`: serves until it's stopped, by ctrl+c or SIGTERM, the
/// jobs running stopped first. As the `helper` `open` starts, it's cut
/// loose from the terminal, and says nothing but in its log.
pub fn serve(port: Option<u16>, helper: bool) -> Result<()> {
    if helper {
        // SAFETY: setsid has no preconditions.
        unsafe {
            libc::setsid();
        }
    }
    let _lock = take_lock()?;
    signals::catch();
    let running = start(port, generator::current())?;
    let port = running.port();
    write_serving(&Serving {
        pid: std::process::id(),
        port,
        lattice: VERSION.to_string(),
    })?;
    let home = format!("http://127.0.0.1:{port}/");
    if helper {
        errln!("serving lattice at {home}");
    } else {
        outln!("serving lattice at {home} (ctrl+c stops it)")?;
    }
    while !signals::asked_to_stop() {
        thread::sleep(Duration::from_millis(100));
    }
    errln!("stopping: the jobs running are cancelled");
    running.stop();
    forget_serving();
    Ok(())
}

/// Holds the data directory's server lock, or says where the server that
/// holds it is.
fn take_lock() -> Result<File> {
    let dir = paths::data_dir();
    fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    let path = dir.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("couldn't open {}", path.display()))?;
    // SAFETY: flock has no preconditions; the descriptor is the file's.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let at = read_serving()
            .map(|serving| format!(", at http://127.0.0.1:{}/", serving.port))
            .unwrap_or_default();
        bail!("a lattice server runs already{at}: `lattice serve --stop` stops it");
    }
    Ok(file)
}

/// Writes `serving` into the data directory, whole or not at all.
fn write_serving(serving: &Serving) -> Result<()> {
    let path = paths::data_dir().join(SERVING_FILE);
    let partial = path.with_extension(format!("json.{}", serving.pid));
    fs::write(&partial, serde_json::to_vec(serving)?)
        .with_context(|| format!("couldn't write {}", partial.display()))?;
    fs::rename(&partial, &path).with_context(|| format!("couldn't write {}", path.display()))
}

fn read_serving() -> Option<Serving> {
    serde_json::from_slice(&fs::read(paths::data_dir().join(SERVING_FILE)).ok()?).ok()
}

/// Takes out `serve.json` when it's this process's.
fn forget_serving() {
    if read_serving().is_some_and(|serving| serving.pid == std::process::id()) {
        let _ = fs::remove_file(paths::data_dir().join(SERVING_FILE));
    }
}

/// `lattice open`: opens the browser on the wiki of the repository `typed`
/// names, added first when it's new, or on the home page, starting a
/// server in the background first when none is running. Over ssh, it says
/// the address, and how to reach it from the user's own machine.
pub fn open(typed: Option<String>) -> Result<()> {
    let page = match typed {
        None => "/".to_string(),
        Some(typed) => {
            let cwd = std::env::current_dir().context("couldn't tell the current directory")?;
            let mut db = Db::open()?;
            let repo = match repos::find(&db, &typed, &cwd)? {
                Some(repo) => repo,
                None => repos::add(&mut db, &typed, &cwd)?,
            };
            page_of(&repo)
        }
    };
    let port = match running() {
        Some(port) => port,
        None => start_helper()?,
    };
    show(&format!("http://127.0.0.1:{port}{page}"), port)
}

/// `lattice serve --stop`: stops the server running, and says so.
pub fn stop() -> Result<()> {
    let Some(serving) = read_serving().filter(|serving| alive(serving.pid)) else {
        outln!("no lattice server is running")?;
        return Ok(());
    };
    terminate(serving.pid)?;
    outln!("stopped the lattice server on port {}", serving.port)?;
    Ok(())
}

/// Whether the process `pid` is there.
fn alive(pid: u32) -> bool {
    // SAFETY: kill with no signal only asks whether the process is there.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Asks the server `pid` to stop, and waits for it to.
fn terminate(pid: u32) -> Result<()> {
    // SAFETY: kill has no preconditions; it's a lattice server's process.
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    let deadline = Instant::now() + STOP_WAIT + Duration::from_secs(5);
    while alive(pid) {
        if Instant::now() >= deadline {
            bail!("the lattice server (pid {pid}) didn't stop");
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

/// Opens `url` in the browser and says it; over ssh, says how to forward
/// its `port` from the user's machine, then it. Last, for whoever reads
/// only the last line.
fn show(url: &str, port: u16) -> Result<()> {
    if clipboard::remote() {
        outln!(
            "lattice runs on another machine: forward the port from yours, then open the link \
             there:\n  ssh -L {port}:127.0.0.1:{port} {}",
            hostname()
        )?;
    } else {
        links::open(url)?;
    }
    outln!("{url}")?;
    Ok(())
}

/// This machine's name, for the `ssh` line.
fn hostname() -> String {
    let mut name = [0u8; 256];
    // SAFETY: the buffer is as long as it's said to be.
    let got = unsafe { libc::gethostname(name.as_mut_ptr().cast(), name.len()) };
    let end = name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(name.len());
    match got {
        0 if end > 0 => String::from_utf8_lossy(&name[..end]).into_owned(),
        _ => "this-machine".to_string(),
    }
}

/// The port of the server running, if one of this lattice is. One of
/// another lattice is stopped, for this one to take its place.
fn running() -> Option<u16> {
    let serving = read_serving()?;
    if !alive(serving.pid) {
        return None;
    }
    let says = ask_server(serving.port, "/api/server");
    if says.as_ref().is_some_and(|says| says["lattice"] == VERSION) {
        return Some(serving.port);
    }
    if says.is_some() {
        let _ = terminate(serving.pid);
    }
    None
}

/// What the server on `port` answers to a GET of `path`, as JSON.
fn ask_server(port: u16, path: &str) -> Option<Value> {
    let address = (Ipv4Addr::LOCALHOST, port).into();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).ok()?;
    let at = answer.windows(4).position(|w| w == b"\r\n\r\n")?;
    serde_json::from_slice(&answer[at + 4..]).ok()
}

/// Starts a server in the background, and gives its port once it's
/// listening.
fn start_helper() -> Result<u16> {
    let dir = paths::data_dir();
    fs::create_dir_all(&dir).with_context(|| format!("couldn't make {}", dir.display()))?;
    let log_path = dir.join(LOG_FILE);
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    // Not waited on: it outlives us, and init reaps it.
    let child = Command::new(std::env::current_exe()?)
        .args(["serve", "--helper"])
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("couldn't start the lattice server")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(serving) = read_serving().filter(|serving| serving.pid == child.id()) {
            return Ok(serving.port);
        }
        thread::sleep(Duration::from_millis(20));
    }
    bail!(
        "the lattice server didn't start; see {}",
        log_path.display()
    )
}

/// What a request asks for.
#[derive(Debug, PartialEq, Eq)]
enum Route<'a> {
    /// Which lattice this is, for `open`.
    Server,
    Repos,
    AddRepo,
    RemoveRepo(&'a str),
    StartJob(&'a str),
    Wiki(&'a str),
    Ask(&'a str),
    Open(&'a str),
    Status(&'a str),
    Job(u64),
    CancelJob(u64),
    JobEvents(u64),
    Settings,
    SaveSettings,
    Mermaid,
    /// One of the web app's files, or its page, by its path.
    App(&'a str),
}

impl Route<'_> {
    /// Whether it does something, so must come from the server's own page.
    fn acts(&self) -> bool {
        matches!(
            self,
            Route::AddRepo
                | Route::RemoveRepo(_)
                | Route::StartJob(_)
                | Route::Ask(_)
                | Route::Open(_)
                | Route::CancelJob(_)
                | Route::SaveSettings
        )
    }
}

/// What `method` and `path` ask for, or the status that refuses them, and
/// why. A path that steps out of where it is (`..`) is refused, though
/// nothing would be read by it.
fn route<'a>(method: &str, path: &'a str) -> Result<Route<'a>, (u16, String)> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err((400, "that isn't a path".into()));
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let stepping = |part: &&str| matches!(*part, "." | "..") || part.contains(['\\', '\0']);
    if parts.iter().any(stepping) {
        return Err((400, "a path may not step out of where it is".into()));
    }
    let repo = |key: &'a str| match source::is_key(key) {
        true => Ok(key),
        false => Err((404, format!("there's no repository called {key}"))),
    };
    let job = |id: &str| {
        id.parse::<u64>()
            .map_err(|_| (404, format!("there's no job {id}")))
    };
    let (wanted, route) = match parts.as_slice() {
        ["api", "server"] => ("GET", Route::Server),
        ["api", "repos"] if method == "POST" => ("POST", Route::AddRepo),
        ["api", "repos"] => ("GET", Route::Repos),
        ["api", "repos", key] => ("DELETE", Route::RemoveRepo(repo(key)?)),
        ["api", "repos", key, "jobs"] => ("POST", Route::StartJob(repo(key)?)),
        ["api", "repos", key, "wiki"] => ("GET", Route::Wiki(repo(key)?)),
        ["api", "repos", key, "ask"] => ("POST", Route::Ask(repo(key)?)),
        ["api", "repos", key, "open"] => ("GET", Route::Open(repo(key)?)),
        ["api", "repos", key, "status"] => ("GET", Route::Status(repo(key)?)),
        ["api", "jobs", id] => ("GET", Route::Job(job(id)?)),
        ["api", "jobs", id, "cancel"] => ("POST", Route::CancelJob(job(id)?)),
        ["api", "jobs", id, "events"] => ("GET", Route::JobEvents(job(id)?)),
        ["api", "settings"] if method == "PUT" => ("PUT", Route::SaveSettings),
        ["api", "settings"] => ("GET", Route::Settings),
        ["api", ..] => return Err((404, format!("there's no {path} in lattice's API"))),
        ["assets", "mermaid.min.js"] => ("GET", Route::Mermaid),
        _ => ("GET", Route::App(rest)),
    };
    if method != wanted {
        return Err((405, format!("{path} takes a {wanted}, not a {method}")));
    }
    Ok(route)
}

/// The body of `POST /api/repos`.
#[derive(Deserialize)]
struct Added {
    source: String,
}

/// The body of `POST /api/repos/<key>/jobs`.
#[derive(Deserialize)]
struct Asked {
    kind: JobKind,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    concurrency: Option<usize>,
}

impl Server {
    fn db(&self) -> MutexGuard<'_, Db> {
        self.db.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Answers the one request `stream` sends, and closes it.
    fn handle(&self, stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        let _ = stream.set_nodelay(true);
        let Ok(read) = stream.try_clone() else {
            return;
        };
        let mut out = stream;
        let mut reader = BufReader::new(read);
        let request = match http::read_request(&mut reader, &mut out) {
            Ok(request) => request,
            Err(status) => {
                let _ = Reply::text(status, http::reason(status)).write(&mut out);
                return;
            }
        };
        let _ = self.answer(&request, &mut out);
    }

    fn answer(&self, request: &Request, out: &mut TcpStream) -> io::Result<()> {
        if !http::is_local_host(request.header("host")) {
            let why = "lattice answers to 127.0.0.1 and localhost only";
            return Reply::text(403, why).write(out);
        }
        let route = match route(&request.method, &request.path) {
            Ok(route) => route,
            Err((status, why)) if request.path.starts_with("/api/") => {
                return Reply::error(status, why).write(out);
            }
            Err((status, why)) => return Reply::text(status, why).write(out),
        };
        if route.acts()
            && let Some(from) = http::foreign(request, self.port)
        {
            return Reply::error(403, format!("refused: it came from {from}")).write(out);
        }
        let reply = match route {
            Route::Server => Reply::json(&json!({ "lattice": VERSION })),
            Route::Repos => self.repos(),
            Route::AddRepo => self.add_repo(request),
            Route::RemoveRepo(key) => self.remove_repo(key),
            Route::StartJob(key) => self.start_job(key, request),
            Route::Wiki(key) => self.wiki(key, request),
            Route::Ask(key) => return self.ask(key, request, out),
            Route::Open(key) => self.open(key, request),
            Route::Status(key) => self.status(key),
            Route::Job(id) => match self.db().job(id) {
                Ok(Some(job)) => Reply::json(&job),
                Ok(None) => Reply::error(404, format!("there's no job {id}")),
                Err(err) => internal(err),
            },
            Route::CancelJob(id) => match self.jobs.cancel(id) {
                Ok(Some(job)) => Reply::json(&job),
                Ok(None) => Reply::error(404, format!("there's no job {id}")),
                Err(err) => internal(err),
            },
            Route::JobEvents(id) => return self.job_events(id, out),
            Route::Settings => match Config::load() {
                Ok(config) => Reply::json(&config),
                Err(err) => internal(err),
            },
            Route::SaveSettings => save_settings(request),
            Route::Mermaid => self.mermaid(),
            Route::App(path) => app(path),
        };
        reply.write(out)
    }

    /// The repository called `key`, or the reply saying there's none.
    fn repo(&self, key: &str) -> Result<Repo, Reply> {
        match self.db().repo(key) {
            Ok(Some(repo)) => Ok(repo),
            Ok(None) => Err(Reply::error(
                404,
                format!("there's no repository called {key}"),
            )),
            Err(err) => Err(internal(err)),
        }
    }

    /// `GET /api/repos`: every repository with its versions and its latest
    /// job.
    fn repos(&self) -> Reply {
        let listed = (|| -> Result<Vec<Value>> {
            let db = self.db();
            let mut listed = Vec::new();
            for repo in db.repos()? {
                let versions = db.versions(&repo.key)?;
                let job = db.jobs(&repo.key)?.into_iter().next();
                listed.push(json!({
                    "key": repo.key,
                    "name": repo.name,
                    "source": repo.source,
                    "versions": versions,
                    "job": job,
                }));
            }
            Ok(listed)
        })();
        match listed {
            Ok(listed) => Reply::json(&listed),
            Err(err) => internal(err),
        }
    }

    /// `POST /api/repos`: the repository the body's `source` names, added,
    /// a path from the home directory unless it's absolute.
    fn add_repo(&self, request: &Request) -> Reply {
        let added: Added = match serde_json::from_slice(&request.body) {
            Ok(added) => added,
            Err(err) => return Reply::error(400, format!("that isn't a repository: {err}")),
        };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let from = home.unwrap_or_else(|| PathBuf::from("/"));
        match repos::add(&mut self.db(), &added.source, &from) {
            Ok(repo) => Reply::json(&json!({ "key": repo.key })),
            Err(err) => Reply::error(400, format!("{err:#}")),
        }
    }

    /// `DELETE /api/repos/<key>`: the repository forgotten, its job stopped
    /// first, and its files taken away.
    fn remove_repo(&self, key: &str) -> Reply {
        match self.jobs.stop_repo(key, STOP_WAIT) {
            Ok(true) => {}
            Ok(false) => {
                return Reply::error(409, "its job is still stopping: try again in a moment");
            }
            Err(err) => return internal(err),
        }
        match repos::remove(&self.db(), key) {
            Ok(true) => Reply::empty(),
            Ok(false) => Reply::error(404, format!("there's no repository called {key}")),
            Err(err) => internal(err),
        }
    }

    /// `POST /api/repos/<key>/jobs`: a job asked for, queued.
    fn start_job(&self, key: &str, request: &Request) -> Reply {
        let asked: Asked = match serde_json::from_slice(&request.body) {
            Ok(asked) => asked,
            Err(err) => return Reply::error(400, format!("that isn't a job: {err}")),
        };
        match self
            .jobs
            .submit(key, asked.kind, asked.model.as_deref(), asked.concurrency)
        {
            Ok(job) => Reply::json(&job),
            Err(refused) => Reply::error(refused.status, refused.message),
        }
    }

    /// The `wiki.json` of version `n` of `repo`'s wiki, or of its latest,
    /// and its number.
    fn wiki_file(&self, repo: &Repo, n: Option<u32>) -> Result<(u32, PathBuf), Reply> {
        let versions = self.db().versions(&repo.key).map_err(internal)?;
        let found = match n {
            Some(n) => versions.iter().find(|version| version.n == n),
            None => versions.last(),
        };
        match found {
            Some(version) => Ok((
                version.n,
                paths::version_dir(&repo.key, version.n).join(paths::WIKI_FILE),
            )),
            None => Err(Reply::error(
                404,
                match n {
                    Some(n) => format!("{} has no version {n}", repo.name),
                    None => format!("{} has no wiki yet", repo.name),
                },
            )),
        }
    }

    /// `GET /api/repos/<key>/wiki?version=<n>`: a version's `wiki.json`.
    fn wiki(&self, key: &str, request: &Request) -> Reply {
        let repo = match self.repo(key) {
            Ok(repo) => repo,
            Err(reply) => return reply,
        };
        let n = match request.param("version") {
            None => None,
            Some(n) => match n.parse() {
                Ok(n) => Some(n),
                Err(_) => return Reply::error(400, format!("{n} isn't a version's number")),
            },
        };
        match self.wiki_file(&repo, n) {
            Ok((_, file)) => match fs::read(&file) {
                Ok(bytes) => Reply::bytes("application/json", bytes),
                Err(err) => internal(anyhow::anyhow!("couldn't read {}: {err}", file.display())),
            },
            Err(reply) => reply,
        }
    }

    /// `POST /api/repos/<key>/ask`: the question in the body, answered as
    /// it comes, about the latest version.
    fn ask(&self, key: &str, request: &Request, out: &mut TcpStream) -> io::Result<()> {
        let question: Question = match serde_json::from_slice(&request.body) {
            Ok(question) => question,
            Err(err) => {
                return Reply::error(400, format!("that isn't a question: {err}")).write(out);
            }
        };
        let prepared = (|| {
            let repo = self.repo(key)?;
            let (_, file) = self.wiki_file(&repo, None)?;
            let wiki: Value = fs::read(&file)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| internal(anyhow::anyhow!("couldn't read {}", file.display())))?;
            let config = Config::load().map_err(internal)?;
            Ok::<_, Reply>((repo, wiki, config))
        })();
        let (repo, wiki, config) = match prepared {
            Ok(prepared) => prepared,
            Err(reply) => return reply.write(out),
        };
        http::start_events(out)?;
        let root = repos::root(&repo);
        if !root.is_dir() {
            let why = format!("{}'s code isn't at {} any more", repo.name, root.display());
            return http::event(out, "error", &json!({ "message": why }));
        }
        ask::answer(&question, &wiki, &root, &config, out)
    }

    /// `GET /api/repos/<key>/open?path=&line=`: the file `path` in the
    /// repository opened in the user's editor, at `line`.
    fn open(&self, key: &str, request: &Request) -> Reply {
        let repo = match self.repo(key) {
            Ok(repo) => repo,
            Err(reply) => return reply,
        };
        let root = repos::root(&repo);
        let path = request.param("path").unwrap_or_default();
        let Some(file) = editor::file_in(&root, &path) else {
            return Reply::error(404, format!("there's no file {path} in {}", repo.name));
        };
        let line = request
            .param("line")
            .and_then(|line| line.parse::<usize>().ok())
            .filter(|line| *line > 0);
        match editor::open(&root, &file, line) {
            Ok(()) => Reply::empty(),
            Err(err) => Reply::error(500, format!("{err:#}")),
        }
    }

    /// `GET /api/repos/<key>/status`: whether its branch has moved on since
    /// the latest version, and its latest job.
    fn status(&self, key: &str) -> Reply {
        let repo = match self.repo(key) {
            Ok(repo) => repo,
            Err(reply) => return reply,
        };
        let known = (|| -> Result<_> {
            let db = self.db();
            Ok((db.versions(key)?.pop(), db.jobs(key)?.into_iter().next()))
        })();
        let (latest, job) = match known {
            Ok(known) => known,
            Err(err) => return internal(err),
        };
        self.fetch_now_and_then(&repo);
        let branch = latest.as_ref().and_then(|version| version.branch.clone());
        let head = repos::head(&repo, branch.as_deref());
        let commit = latest.map(|version| version.commit);
        let stale = matches!((&head, &commit), (Some(head), Some(commit)) if head != commit);
        Reply::json(&json!({ "stale": stale, "head": head, "commit": commit, "job": job }))
    }

    /// Fetches a git repository in the background, for its status to say
    /// where its branch is, unless it was fetched a while ago or it's being
    /// built.
    fn fetch_now_and_then(&self, repo: &Repo) {
        if !matches!(repo.source, Source::Git { .. }) {
            return;
        }
        let mut fetched = self.fetched.lock().unwrap_or_else(|err| err.into_inner());
        if fetched
            .get(&repo.key)
            .is_some_and(|at| at.elapsed() < FETCH_EVERY)
        {
            return;
        }
        fetched.insert(repo.key.clone(), Instant::now());
        let repo = repo.clone();
        thread::spawn(move || {
            // Held meanwhile, so no build starts fetching too.
            if let Ok(Some(_lock)) = BuildLock::take(&repo.key) {
                repos::fetch_quietly(&repo);
            }
        });
    }

    /// `GET /api/jobs/<id>/events`: what job `id` has said, then what it
    /// says as it says it, until it ends.
    fn job_events(&self, id: u64, out: &mut TcpStream) -> io::Result<()> {
        let job = match self.db().job(id) {
            Ok(Some(job)) => job,
            Ok(None) => return Reply::error(404, format!("there's no job {id}")).write(out),
            Err(err) => return internal(err).write(out),
        };
        http::start_events(out)?;
        if let Some(feed) = self.jobs.feed(id) {
            return follow(&feed, out);
        }
        self.follow_kept(job, out)
    }

    /// Tells the page what's kept of a job this server doesn't run: one
    /// that's over, or one another lattice runs, looked at again now and
    /// then until it's over.
    fn follow_kept(&self, mut job: Job, out: &mut TcpStream) -> io::Result<()> {
        let mut progress: Option<Progress> = None;
        let mut lines = 0;
        loop {
            if job.progress != progress
                && let Some(now) = &job.progress
            {
                http::event(out, "progress", &json!(now))?;
                progress = job.progress.clone();
            }
            let log = jobs::kept_log(&job);
            for line in log.iter().skip(lines) {
                http::event(out, "log", &json!({ "line": line }))?;
            }
            lines = lines.max(log.len());
            if let Some(ended) = jobs::ended_of(&job) {
                return end(&ended, out);
            }
            thread::sleep(POLL);
            http::keep_alive(out)?;
            job = match self.db().job(job.id) {
                Ok(Some(job)) => job,
                _ => return end(&Ended::Cancelled, out),
            };
        }
    }

    /// `GET /assets/mermaid.min.js`: mermaid, downloaded first if need be.
    fn mermaid(&self) -> Reply {
        let read = self.mermaid.get().and_then(|path| {
            fs::read(&path).map_err(|err| format!("couldn't read {}: {err}", path.display()))
        });
        match read {
            Ok(bytes) => Reply::bytes(web::content_type(MERMAID.name), bytes)
                .with("Cache-Control", "public, max-age=86400"),
            Err(why) => Reply::text(503, format!("mermaid isn't here: {why}")),
        }
    }
}

/// Follows a job's `feed` to the page: everything it has said, then each
/// thing as it's said, until it ends.
fn follow(feed: &Feed, out: &mut TcpStream) -> io::Result<()> {
    let mut seen = Seen::default();
    loop {
        let news = feed.next(&mut seen, QUIET);
        if news == jobs::News::default() {
            http::keep_alive(out)?;
            continue;
        }
        if let Some(progress) = &news.progress {
            http::event(out, "progress", &json!(progress))?;
        }
        for line in &news.lines {
            http::event(out, "log", &json!({ "line": line }))?;
        }
        if let Some(ended) = &news.end {
            return end(ended, out);
        }
    }
}

/// The event that says how a job ended.
fn end(ended: &Ended, out: &mut TcpStream) -> io::Result<()> {
    match ended {
        Ended::Done(n) => http::event(out, "done", &json!({ "version": n })),
        Ended::Failed(why) => http::event(out, "error", &json!({ "message": why })),
        Ended::Cancelled => http::event(out, "error", &json!({ "message": "cancelled" })),
    }
}

/// `PUT /api/settings`: the settings in the body, checked and saved, and
/// given back as they're saved.
fn save_settings(request: &Request) -> Reply {
    let config: Config = match serde_json::from_slice(&request.body) {
        Ok(config) => config,
        Err(err) => return Reply::error(400, format!("those aren't settings: {err}")),
    };
    if let Err(err) = config.check() {
        return Reply::error(400, format!("{err:#}"));
    }
    match config.save() {
        Ok(()) => Reply::json(&config),
        Err(err) => internal(err),
    }
}

/// One of the web app's files, or its page for any other address, which
/// the app routes itself.
fn app(path: &str) -> Reply {
    if !path.is_empty()
        && let Some(bytes) = web::file(path)
    {
        let cache = match web::is_immutable(path) {
            true => "public, max-age=31536000, immutable",
            false => "no-cache",
        };
        return Reply::bytes(web::content_type(path), bytes).with("Cache-Control", cache);
    }
    Reply::bytes("text/html; charset=utf-8", web::index())
}

/// The reply to something that went wrong in the server itself.
fn internal(err: anyhow::Error) -> Reply {
    Reply::error(500, format!("{err:#}"))
}

/// The path of a repository's page in the web app.
pub fn page_of(repo: &Repo) -> String {
    format!("/{}", repo.key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_path_goes_to_its_route_and_nothing_steps_out() {
        fn ok<'a>(method: &str, path: &'a str) -> Route<'a> {
            route(method, path).unwrap()
        }
        assert_eq!(ok("GET", "/api/server"), Route::Server);
        assert_eq!(ok("GET", "/api/repos"), Route::Repos);
        assert_eq!(ok("POST", "/api/repos"), Route::AddRepo);
        assert_eq!(ok("DELETE", "/api/repos/go"), Route::RemoveRepo("go"));
        assert_eq!(ok("POST", "/api/repos/go/jobs"), Route::StartJob("go"));
        assert_eq!(ok("GET", "/api/repos/go/wiki"), Route::Wiki("go"));
        assert_eq!(ok("POST", "/api/repos/go/ask"), Route::Ask("go"));
        assert_eq!(ok("GET", "/api/repos/go/open"), Route::Open("go"));
        assert_eq!(ok("GET", "/api/repos/go/status"), Route::Status("go"));
        assert_eq!(ok("GET", "/api/jobs/12"), Route::Job(12));
        assert_eq!(ok("POST", "/api/jobs/12/cancel"), Route::CancelJob(12));
        assert_eq!(ok("GET", "/api/jobs/12/events"), Route::JobEvents(12));
        assert_eq!(ok("GET", "/api/settings"), Route::Settings);
        assert_eq!(ok("PUT", "/api/settings"), Route::SaveSettings);
        assert_eq!(ok("GET", "/assets/mermaid.min.js"), Route::Mermaid);
        assert_eq!(ok("GET", "/"), Route::App(""));
        assert_eq!(ok("GET", "/go/v/2"), Route::App("go/v/2"));
        assert_eq!(
            ok("GET", "/_app/immutable/x.js"),
            Route::App("_app/immutable/x.js")
        );
        let refused = |method: &str, path: &str| route(method, path).unwrap_err().0;
        assert_eq!(refused("GET", "/api/repos/go/ask"), 405);
        assert_eq!(refused("POST", "/api/repos/go/wiki"), 405);
        assert_eq!(refused("PUT", "/go"), 405);
        assert_eq!(refused("GET", "/api/repos/Go/wiki"), 404, "not a key");
        assert_eq!(refused("GET", "/api/repos/settings/wiki"), 404, "reserved");
        assert_eq!(refused("GET", "/api/jobs/x"), 404);
        assert_eq!(refused("GET", "/api/nothing"), 404);
        assert_eq!(refused("GET", "/_app/../../etc/passwd"), 400);
        assert_eq!(refused("GET", "/api/repos/../wiki"), 400);
        assert_eq!(refused("GET", "/a\\b"), 400);
        assert_eq!(refused("GET", "nothing"), 400);
    }

    #[test]
    fn what_does_something_is_known_as_such() {
        for route in [
            Route::AddRepo,
            Route::RemoveRepo("go"),
            Route::StartJob("go"),
            Route::Ask("go"),
            Route::Open("go"),
            Route::CancelJob(1),
            Route::SaveSettings,
        ] {
            assert!(route.acts(), "{route:?}");
        }
        for route in [
            Route::Repos,
            Route::Wiki("go"),
            Route::JobEvents(1),
            Route::App(""),
        ] {
            assert!(!route.acts(), "{route:?}");
        }
    }

    #[test]
    fn the_app_s_own_addresses_are_its_page() {
        let mut out = Vec::new();
        app("settings").write(&mut out).unwrap();
        let out = String::from_utf8_lossy(&out).into_owned();
        assert!(out.starts_with("HTTP/1.1 200 OK\r\n"), "{out}");
        assert!(out.contains("Content-Type: text/html; charset=utf-8"));
        assert!(out.contains("Cache-Control: no-cache"));
    }
}
