//! The server's API end to end: a server of the test's own on a free port,
//! its generator a fake that writes a fixture `wiki.json`, a fake `claude`
//! for the chat and a fake editor first on the `PATH`, and every directory
//! lattice keeps things in a temporary one. Every test here shares those
//! directories, set once for the process, so they run one at a time.

use lattice::cancel::Cancel;
use lattice::db::{Db, Phase, Progress, Version};
use lattice::generator::{Built, Generator, JobSpec, Report};
use lattice::server::{self, Running};
use lattice::time::Timestamp;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// The directories every test here shares.
struct Home {
    dir: PathBuf,
}

static HOME: OnceLock<Home> = OnceLock::new();

/// Held by each test while it runs.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The test's turn, and the directories, set up the first time.
fn home() -> (MutexGuard<'static, ()>, &'static Home) {
    let turn = ONE_AT_A_TIME.lock().unwrap_or_else(|err| err.into_inner());
    let home = HOME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap().keep();
        let dir = dir.canonicalize().unwrap();
        let bin = dir.join("bin");
        std::fs::create_dir(&bin).unwrap();
        let record = dir.join("record");
        // Claude Code headless, as the chat runs it: it notes its arguments
        // and the question, reads a file, and answers in pieces.
        script(
            &bin.join("claude"),
            &format!(
                r#"printf '%s\n' "$@" > {record}.args
cat > {record}.question
echo '{{"type":"system","subtype":"init","session_id":"0b5c9e2a-77","model":"claude-sonnet-5-5"}}'
echo '{{"type":"stream_event","event":{{"type":"content_block_delta","delta":{{"type":"text_delta","text":"It is "}}}}}}'
echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{"file_path":"'"$PWD"'/src/x.rs"}}}}]}}}}'
echo '{{"type":"stream_event","event":{{"type":"content_block_delta","delta":{{"type":"text_delta","text":"[x](code:src/x.rs#L1)."}}}}}}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"It is [x](code:src/x.rs#L1).","session_id":"0b5c9e2a-77","total_cost_usd":0.03}}'
"#,
                record = record.display()
            ),
        );
        // An editor with a window of its own, by its name.
        script(
            &bin.join("code"),
            &format!(
                "printf '%s\\n' \"$PWD\" \"$@\" > {0}.new && mv {0}.new {0}\n",
                dir.join("opened").display()
            ),
        );
        let path = format!("{}:/usr/bin:/bin", bin.display());
        // SAFETY: set once, before any server runs, with every test that
        // could read the environment waiting on ONE_AT_A_TIME.
        unsafe {
            std::env::set_var("HOME", &dir);
            std::env::set_var("PATH", path);
            std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
            std::env::set_var("XDG_CACHE_HOME", dir.join("cache"));
            std::env::set_var("LATTICE_NO_DOWNLOAD", "1");
            std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
            std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
            std::env::set_var("VISUAL", bin.join("code"));
            for name in ["CLAUDECODE", "SSH_CONNECTION", "SSH_TTY", "EDITOR"] {
                std::env::remove_var(name);
            }
        }
        Home { dir }
    });
    (turn, home)
}

impl Home {
    /// What the fake `claude` wrote down: `args` or `question`.
    fn recorded(&self, what: &str) -> String {
        std::fs::read_to_string(self.dir.join(format!("record.{what}"))).unwrap()
    }

    /// A git repository of the test's own called `name`, with a commit:
    /// `src/x.rs`.
    fn repo(&self, name: &str) -> PathBuf {
        let root = self.dir.join("code").join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/x.rs"), "fn x() {}\n").unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        commit(&root, "first");
        root
    }
}

fn script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=lattice",
            "-c",
            "user.email=lattice@example.com",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Commits everything in `root`, and gives the commit.
fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

/// What the fake generator does.
#[derive(Clone, Default)]
struct Fake {
    /// Fails, saying this.
    fail: Option<String>,
    /// Waits until it's cancelled.
    hold: bool,
}

impl Generator for Fake {
    fn run(
        &self,
        job: &JobSpec,
        report: &mut dyn FnMut(Report),
        cancel: &Cancel,
    ) -> anyhow::Result<Built> {
        let progress = |phase, done, current: &str| {
            Report::Progress(Progress {
                phase,
                done,
                total: 2,
                current: Some(current.to_string()),
                cost_usd: 0.5,
            })
        };
        report(progress(Phase::Plan, 0, "the outline"));
        report(Report::Log(format!(
            "planning {} with {}",
            job.repo.name, job.config.model
        )));
        if self.hold {
            while !cancel.is_cancelled() {
                std::thread::sleep(Duration::from_millis(20));
            }
            anyhow::bail!("stopped");
        }
        if let Some(why) = &self.fail {
            anyhow::bail!("{why}");
        }
        report(progress(Phase::Write, 1, "Starting"));
        let wiki = json!({
            "version": 1,
            "repo": {"name": job.repo.name, "root": job.root, "commit": job.commit,
                     "branch": job.branch, "web_url": null, "code_url": null},
            "generated": {"at": "2026-10-09T12:00:00Z", "model": "claude-sonnet-5-5"},
            "overview": {"summary_md": "An app.", "diagram": null},
            "sections": [{"id": "starting", "title": "Starting", "summary_md": "How it starts.",
                          "diagram": null, "subsections": [
                              {"id": "x", "title": "The x", "body_md": "See [x](code:src/x.rs#L1).",
                               "diagram": null, "files": ["src/x.rs"]}]}],
            "kind": job.kind,
            "latest": job.latest.as_ref().map(|version| version.n),
        });
        std::fs::write(job.work.join("wiki.json"), wiki.to_string()).unwrap();
        Ok(Built {
            model: "claude-sonnet-5-5".into(),
            cost_usd: 1.25,
        })
    }
}

/// A server of the test's own, stopped as the test ends.
struct Served {
    running: Option<Running>,
    port: u16,
}

impl Served {
    fn start(fake: Fake) -> Served {
        let running = server::start(Ipv4Addr::LOCALHOST, Some(0), Arc::new(fake)).unwrap();
        let port = running.port();
        Served {
            running: Some(running),
            port,
        }
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Sends `request` as it is, its `Host` this server's unless it says
    /// one, and gives the status and the answer, its head and its body.
    fn send(&self, request: &str) -> (u16, String, String) {
        let request = match request.to_ascii_lowercase().contains("\r\nhost:") {
            true => request.to_string(),
            false => request.replacen("\r\n", &format!("\r\nHost: 127.0.0.1:{}\r\n", self.port), 1),
        };
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).unwrap();
        let answer = String::from_utf8_lossy(&answer).into_owned();
        let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((&answer, ""));
        let status = head
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        (status, head.to_string(), body.to_string())
    }

    fn get(&self, path: &str) -> (u16, String, String) {
        self.send(&format!("GET {path} HTTP/1.1\r\n\r\n"))
    }

    fn json(&self, path: &str) -> Value {
        let (status, head, body) = self.get(path);
        assert_eq!(status, 200, "{path}: {head}{body}");
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("{path}: {body}"))
    }

    /// A request with a body, from the server's own page.
    fn call(&self, method: &str, path: &str, body: &Value) -> (u16, Value) {
        let body = body.to_string();
        let (status, _, answer) = self.send(&format!(
            "{method} {path} HTTP/1.1\r\nOrigin: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            self.origin(),
            body.len()
        ));
        (status, serde_json::from_str(&answer).unwrap_or(Value::Null))
    }

    /// Adds the repository at `source`, and gives its key.
    fn add(&self, source: &str) -> String {
        let (status, added) = self.call("POST", "/api/repos", &json!({ "source": source }));
        assert_eq!(status, 200, "{added}");
        added["key"].as_str().unwrap().to_string()
    }

    /// Asks for a job of `kind` on `key`, and gives it.
    fn job(&self, key: &str, kind: &str) -> Value {
        let (status, job) = self.call(
            "POST",
            &format!("/api/repos/{key}/jobs"),
            &json!({ "kind": kind, "model": null, "concurrency": null }),
        );
        assert_eq!(status, 200, "{job}");
        job
    }

    /// Every event of job `id`, until it ends.
    fn events_of(&self, id: &Value) -> Vec<(String, Value)> {
        let (status, head, body) = self.get(&format!("/api/jobs/{id}/events"));
        assert_eq!(status, 200, "{head}");
        assert!(head.contains("Content-Type: text/event-stream"), "{head}");
        events(&body)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
    }
}

/// The events of a `text/event-stream`, each its name and its data.
fn events(body: &str) -> Vec<(String, Value)> {
    body.split("\n\n")
        .filter_map(|event| {
            let name = event
                .lines()
                .find_map(|line| line.strip_prefix("event: "))?;
            let data = event.lines().find_map(|line| line.strip_prefix("data: "))?;
            Some((name.to_string(), serde_json::from_str(data).unwrap()))
        })
        .collect()
}

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(name, _)| name.as_str()).collect()
}

/// Waits for `check` to hold, failing the test after a while.
fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !check() {
        assert!(Instant::now() < deadline, "{what} never came");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_repository_is_built_followed_versioned_and_synced() {
    let (_turn, home) = home();
    let root = home.repo("app");
    let first = git(&root, &["rev-parse", "HEAD"]);
    let served = Served::start(Fake::default());
    let key = served.add(&root.display().to_string());
    assert_eq!(key, "app");
    assert_eq!(
        served.add(&root.join("src").display().to_string()),
        key,
        "the same repository"
    );
    let listed = served.json("/api/repos");
    let app = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|repo| repo["key"] == key)
        .unwrap();
    assert_eq!(app["source"], json!({"kind": "local", "path": root}));
    assert_eq!(app["root"], json!(root), "its code, for the editors' links");
    assert_eq!(app["versions"], json!([]));
    assert_eq!(app["job"], Value::Null);
    assert_eq!(served.get(&format!("/api/repos/{key}/wiki")).0, 404);

    let job = served.job(&key, "build");
    assert_eq!(job["kind"], "build");
    let said = served.events_of(&job["id"]);
    assert_eq!(
        said.last().unwrap(),
        &("done".to_string(), json!({"version": 1}))
    );
    let progress: Vec<&Value> = said
        .iter()
        .filter(|(n, _)| n == "progress")
        .map(|(_, d)| d)
        .collect();
    assert!(!progress.is_empty(), "{said:?}");
    assert!(
        progress
            .iter()
            .all(|p| p["total"] == 2 && p["cost_usd"] == 0.5)
    );
    let log: Vec<&str> = said
        .iter()
        .filter(|(n, _)| n == "log")
        .map(|(_, d)| d["line"].as_str().unwrap())
        .collect();
    assert!(
        log.contains(&format!("job {}: build of app", job["id"]).as_str()),
        "{log:?}"
    );
    assert!(
        log.contains(&format!("at {first} on main").as_str()),
        "{log:?}"
    );
    assert!(log.contains(&"planning app with sonnet"), "{log:?}");

    let done = served.json(&format!("/api/jobs/{}", job["id"]));
    assert_eq!(
        (done["state"].as_str(), done["version"].as_u64()),
        (Some("done"), Some(1))
    );
    assert!(done["started"].is_string() && done["finished"].is_string());
    let wiki = served.json(&format!("/api/repos/{key}/wiki"));
    assert_eq!(wiki["repo"]["commit"], first);
    assert_eq!(wiki["kind"], "build");
    let v1 = home.dir.join("data/lattice/repos/app/v1");
    assert!(v1.join("wiki.json").is_file() && v1.join("build.log").is_file());
    assert!(!home.dir.join("data/lattice/repos/app/work").exists());
    let listed = served.json("/api/repos");
    let app = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|repo| repo["key"] == key)
        .unwrap();
    let version = &app["versions"][0];
    assert_eq!(
        (
            &version["n"],
            &version["commit"],
            &version["branch"],
            &version["model"],
            &version["cost_usd"]
        ),
        (
            &json!(1),
            &json!(first),
            &json!("main"),
            &json!("claude-sonnet-5-5"),
            &json!(1.25)
        )
    );
    assert_eq!(app["job"]["id"], job["id"]);

    // Once the code moves on, the wiki is stale, and a sync makes the next.
    let status = served.json(&format!("/api/repos/{key}/status"));
    assert_eq!(
        (&status["stale"], &status["commit"], &status["head"]),
        (&json!(false), &json!(first), &json!(first))
    );
    std::fs::write(root.join("src/y.rs"), "fn y() {}\n").unwrap();
    let second = commit(&root, "second");
    let status = served.json(&format!("/api/repos/{key}/status"));
    assert_eq!(
        (&status["stale"], &status["head"]),
        (&json!(true), &json!(second))
    );
    let sync = served.job(&key, "sync");
    assert_eq!(
        served.events_of(&sync["id"]).last().unwrap().1,
        json!({"version": 2})
    );
    let wiki = served.json(&format!("/api/repos/{key}/wiki"));
    assert_eq!(
        (&wiki["kind"], &wiki["latest"], &wiki["repo"]["commit"]),
        (&json!("sync"), &json!(1), &json!(second))
    );
    let older = served.json(&format!("/api/repos/{key}/wiki?version=1"));
    assert_eq!(older["repo"]["commit"], first);
    assert_eq!(
        served.get(&format!("/api/repos/{key}/wiki?version=9")).0,
        404
    );
    // A job's events, once it's over, are what was kept of it.
    let again = served.events_of(&job["id"]);
    assert_eq!(again.last().unwrap().1, json!({"version": 1}));
    assert!(names(&again).contains(&"log"));

    // Removed, it's gone, with its files; the repository itself stays.
    let (status, _) = served.call("DELETE", &format!("/api/repos/{key}"), &json!(null));
    assert_eq!(status, 204);
    assert_eq!(served.get(&format!("/api/repos/{key}/status")).0, 404);
    assert!(!home.dir.join("data/lattice/repos/app").exists());
    assert!(root.join("src/x.rs").is_file());
}

#[test]
fn a_job_waits_its_turn_can_be_cancelled_and_resumed() {
    let (_turn, home) = home();
    let root = home.repo("held");
    let served = Served::start(Fake {
        hold: true,
        ..Fake::default()
    });
    let key = served.add(&root.display().to_string());
    let (status, refused) = served.call(
        "POST",
        &format!("/api/repos/{key}/jobs"),
        &json!({"kind": "sync"}),
    );
    assert_eq!(status, 409);
    assert_eq!(
        refused["message"],
        "there's no version to sync from: build one first"
    );
    let (status, refused) = served.call(
        "POST",
        &format!("/api/repos/{key}/jobs"),
        &json!({"kind": "build", "model": "--help"}),
    );
    assert_eq!(status, 400, "{refused}");
    let job = served.job(&key, "build");
    let id = job["id"].clone();
    eventually("the job runs", || {
        served.json(&format!("/api/jobs/{id}"))["state"] == "running"
    });
    let (status, refused) = served.call(
        "POST",
        &format!("/api/repos/{key}/jobs"),
        &json!({"kind": "build"}),
    );
    assert_eq!(status, 409);
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("is running for it already"),
        "{refused}"
    );
    // From another site, a cancel is refused.
    let (status, _, _) = served.send(&format!(
        "POST /api/jobs/{id}/cancel HTTP/1.1\r\nOrigin: https://evil.example\r\n\r\n"
    ));
    assert_eq!(status, 403);
    let (status, _) = served.call("POST", &format!("/api/jobs/{id}/cancel"), &json!(null));
    assert_eq!(status, 200);
    let said = served.events_of(&id);
    assert_eq!(
        said.last().unwrap(),
        &("error".to_string(), json!({"message": "cancelled"}))
    );
    assert_eq!(
        served.json(&format!("/api/jobs/{id}"))["state"],
        "cancelled"
    );
    // What it left is resumed.
    let resumed = served.job(&key, "resume");
    assert_eq!(resumed["kind"], "resume");
    served.call(
        "POST",
        &format!("/api/jobs/{}/cancel", resumed["id"]),
        &json!(null),
    );
    served.events_of(&resumed["id"]);
}

#[test]
fn a_job_that_fails_says_why_without_its_secrets() {
    let (_turn, home) = home();
    let root = home.repo("failing");
    let served = Served::start(Fake {
        fail: Some("the API said no to token=sk-ant-api03-AbCdEf0123456789xyz".into()),
        ..Fake::default()
    });
    let key = served.add(&root.display().to_string());
    let job = served.job(&key, "build");
    let said = served.events_of(&job["id"]);
    let (name, data) = said.last().unwrap();
    assert_eq!(name, "error");
    assert_eq!(data["message"], "the API said no to token=[redacted]");
    let failed = served.json(&format!("/api/jobs/{}", job["id"]));
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["error"], "the API said no to token=[redacted]");
    let log = std::fs::read_to_string(home.dir.join("data/lattice/repos/failing/work/build.log"))
        .unwrap();
    assert!(
        log.contains("it stopped: the API said no to token=[redacted]"),
        "{log}"
    );
    assert!(!log.contains("sk-ant"), "{log}");
}

/// A repository with a version of its wiki, made as a build would.
fn with_a_wiki(home: &Home, name: &str) -> (PathBuf, String) {
    let root = home.repo(name);
    let mut db = Db::open().unwrap();
    let repo = lattice::repos::add(&mut db, &root.display().to_string(), Path::new("/")).unwrap();
    let dir = home
        .dir
        .join("data/lattice/repos")
        .join(&repo.key)
        .join("v1");
    std::fs::create_dir_all(&dir).unwrap();
    let wiki = json!({
        "version": 1,
        "repo": {"name": "acme/app", "root": root, "commit": "0123456789abcdef"},
        "overview": {"summary_md": "An app."},
        "sections": [{"id": "starting", "title": "Starting", "summary_md": "How it starts.",
                      "subsections": [{"id": "x", "title": "The x", "body_md": "The x body."}]}]
    });
    std::fs::write(dir.join("wiki.json"), wiki.to_string()).unwrap();
    db.add_version(
        &repo.key,
        &Version {
            n: 1,
            commit: "0123456789abcdef".into(),
            branch: Some("main".into()),
            model: "claude-sonnet-5-5".into(),
            at: Timestamp(1_791_547_200),
            cost_usd: 1.0,
        },
    )
    .unwrap();
    (root, repo.key)
}

#[test]
fn the_chat_streams_claude_s_answer_what_it_reads_and_how_it_ended() {
    let (_turn, home) = home();
    let (_, key) = with_a_wiki(home, "chatty");
    let served = Served::start(Fake::default());
    let ask = |body: &str, origin: Option<&str>| {
        let origin = origin
            .map(|origin| format!("Origin: {origin}\r\n"))
            .unwrap_or_default();
        served.send(&format!(
            "POST /api/repos/{key}/ask HTTP/1.1\r\n{origin}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        ))
    };
    let origin = served.origin();
    let question = r#"{"question":"What is x?","conversation":null,"section":"x"}"#;
    let (status, head, body) = ask(question, Some(&origin));
    assert_eq!(status, 200, "{head}{body}");
    assert!(head.contains("Content-Type: text/event-stream"), "{head}");
    let said = events(&body);
    assert_eq!(names(&said), ["delta", "tool", "delta", "done"], "{body}");
    assert_eq!(said[0].1, json!({"text": "It is "}));
    assert_eq!(said[1].1, json!({"name": "Read", "path": "src/x.rs"}));
    assert_eq!(
        said[3].1,
        json!({"conversation": "0b5c9e2a-77", "cost_usd": 0.03})
    );
    assert_eq!(home.recorded("question"), "What is x?");
    let args = home.recorded("args");
    let args: Vec<&str> = args.lines().collect();
    let after = |flag: &str| args[args.iter().position(|arg| *arg == flag).unwrap() + 1];
    assert_eq!(after("--model"), "sonnet");
    assert_eq!(after("--max-budget-usd"), "0.50");
    assert_eq!(after("--tools"), "Read,Grep,Glob");
    assert!(args.contains(&"--restricted"), "{args:?}");
    assert!(
        !args.contains(&"--no-session-persistence"),
        "a follow-up resumes it"
    );
    let prompt = args.join("\n");
    assert!(prompt.contains("the code of acme/app"), "{prompt}");
    assert!(prompt.contains("The user is reading \"The x\", under \"Starting\""));

    let follow_up = r#"{"question":"And then?","conversation":"0b5c9e2a-77"}"#;
    let (_, _, body) = ask(follow_up, Some(&origin));
    assert_eq!(events(&body).last().unwrap().0, "done");
    assert!(home.recorded("args").contains("--resume\n0b5c9e2a-77\n"));

    assert_eq!(ask(question, None).0, 403);
    assert_eq!(ask(question, Some("https://evil.example")).0, 403);
    let (_, _, body) = ask(r#"{"question":"  "}"#, Some(&origin));
    assert_eq!(
        events(&body),
        [(
            "error".to_string(),
            json!({"message": "the question is empty"})
        )]
    );
    assert_eq!(ask("not json", Some(&origin)).0, 400);
    let nobody = format!(
        "POST /api/repos/nobody/ask HTTP/1.1\r\nOrigin: {origin}\r\nContent-Length: {}\r\n\r\n{question}",
        question.len()
    );
    assert_eq!(served.send(&nobody).0, 404);
}

#[test]
fn a_file_of_the_repository_is_opened_in_the_editor_at_its_line() {
    let (_turn, home) = home();
    let (root, key) = with_a_wiki(home, "opened");
    let served = Served::start(Fake::default());
    let opened = home.dir.join("opened");
    let _ = std::fs::remove_file(&opened);
    let open = |query: &str, from: &str| {
        served.send(&format!(
            "GET /api/repos/{key}/open?{query} HTTP/1.1\r\nSec-Fetch-Site: {from}\r\n\r\n"
        ))
    };
    let (status, head, body) = open("path=src%2Fx.rs&line=12", "same-origin");
    assert_eq!(status, 204, "{head}{body}");
    eventually("the editor has the file open", || opened.exists());
    let file = root.join("src/x.rs");
    assert_eq!(
        std::fs::read_to_string(&opened).unwrap(),
        format!("{}\n--goto\n{}:12\n", root.display(), file.display())
    );
    assert_eq!(open("path=..%2Fsecret&line=1", "same-origin").0, 404);
    assert_eq!(open("path=src%2Fgone.rs", "same-origin").0, 404);
    assert_eq!(open("path=src%2Fx.rs", "cross-site").0, 403);
}

#[test]
fn the_settings_are_read_checked_and_saved() {
    let (_turn, home) = home();
    let served = Served::start(Fake::default());
    let _ = std::fs::remove_file(home.dir.join("config/lattice/config.toml"));
    let settings = served.json("/api/settings");
    assert_eq!(
        settings,
        json!({"model": "sonnet", "concurrency": 4, "budget_usd": 0.0, "ask_model": "sonnet",
               "ask_budget_usd": 0.5, "exclude": [], "open_code_in": "vscode",
               "index": {"precise": true, "indexer_timeout_secs": 900, "indexer_memory_mb": 8192,
                         "max_file_kb": 1024,
                         "paths_only": ["vendor/", "third_party/", "node_modules/", "testdata/"]}})
    );
    let mut changed = settings.clone();
    changed["model"] = json!("opus");
    changed["exclude"] = json!(["vendor/**"]);
    changed["open_code_in"] = json!("zed");
    let (status, saved) = served.call("PUT", "/api/settings", &changed);
    assert_eq!((status, &saved), (200, &changed));
    assert_eq!(served.json("/api/settings"), changed);
    let file = std::fs::read_to_string(home.dir.join("config/lattice/config.toml")).unwrap();
    assert!(file.contains("model = \"opus\""), "{file}");
    assert!(file.contains("open_code_in = \"zed\""), "{file}");
    let mut wrong = changed.clone();
    wrong["concurrency"] = json!(99);
    let (status, refused) = served.call("PUT", "/api/settings", &wrong);
    assert_eq!(status, 400);
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("concurrency is 99"),
        "{refused}"
    );
    let mut nowhere = changed.clone();
    nowhere["open_code_in"] = json!("notepad");
    let (status, refused) = served.call("PUT", "/api/settings", &nowhere);
    assert_eq!(status, 400, "{refused}");
    let (status, refused) = served.call("PUT", "/api/settings", &json!({"modle": "opus"}));
    assert_eq!(status, 400, "{refused}");
    let body = changed.to_string();
    let unsaid = format!(
        "PUT /api/settings HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    assert_eq!(served.send(&unsaid).0, 403, "a PUT must say its origin");
    let _ = std::fs::remove_file(home.dir.join("config/lattice/config.toml"));
}

#[test]
fn only_this_machine_s_pages_are_answered_and_nothing_steps_out() {
    let (_turn, _home) = home();
    let served = Served::start(Fake::default());
    let (status, _, said) = served.send("GET /api/repos HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert_eq!(status, 403);
    assert!(said.contains("127.0.0.1 and localhost only"), "{said}");
    assert_eq!(
        served
            .send("GET /api/repos HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .0,
        200
    );
    assert_eq!(served.get("/_app/../../etc/passwd").0, 400);
    assert_eq!(served.get("/api/repos/..%2F..%2Fsecret/wiki").0, 400);
    let (status, _, said) = served.get("/api/nothing");
    assert_eq!(status, 404);
    assert!(said.starts_with("{\"message\":"), "{said}");
    assert_eq!(served.get("/api/repos/nobody/status").0, 404);
    assert_eq!(served.get("/api/jobs/999999").0, 404);
    // Every other address is the app's page, which routes it.
    for path in ["/", "/settings", "/app", "/jobs/3", "/app/v/2"] {
        let (status, head, _) = served.get(path);
        assert_eq!(status, 200, "{path}");
        assert!(
            head.contains("Content-Type: text/html; charset=utf-8"),
            "{head}"
        );
    }
    let (status, _, said) = served.get("/assets/mermaid.min.js");
    assert_eq!(status, 503);
    assert!(said.contains("LATTICE_NO_DOWNLOAD"), "{said}");
    let (status, _, _) = served.send("POST /api/repos HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}");
    assert_eq!(status, 403, "a POST must say its origin");
    let (status, refused) = served.call("POST", "/api/repos", &json!({"source": "ext::sh -c x"}));
    assert_eq!(status, 400, "{refused}");
    assert_eq!(
        served.json("/api/server")["lattice"],
        env!("CARGO_PKG_VERSION")
    );
}
