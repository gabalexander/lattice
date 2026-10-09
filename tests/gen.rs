//! The generator end to end, as a job runs it: a small repository of the
//! test's own, a fake `claude` first on the `PATH` that answers each run
//! from fixtures by what it's asked (the preflight, the planner, a
//! subsection's writer, a writer putting its text right, a sync's writer,
//! a section's summary, the overview) and writes down each call, and its
//! settings, data and cache in a temporary directory. It never reads the
//! user's real data nor reaches the network.

use lattice::cancel::Cancel;
use lattice::config::Config;
use lattice::db::{JobKind, Phase, Repo, Version};
use lattice::r#gen::Claude;
use lattice::generator::{Generator, JobSpec, Report};
use lattice::server;
use lattice::source::Source;
use lattice::time::Timestamp;
use lattice::wiki::Wiki;
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

/// The generator reads the environment, its `PATH` for `claude` and its
/// data directory, which only one test at a time may set.
static ENV_HELD: Mutex<()> = Mutex::new(());

/// The system's own programs, after the fake on a test's `PATH`.
const SYSTEM_PATH: &str = "/usr/bin:/bin";

/// What each run of the fake costs.
const COST: f64 = 0.05;

const MAIN: &str = "mod queue;\nmod retry;\n\nfn main() {\n    let mut queue = queue::Queue::new();\n    queue.push(1);\n    retry::backoff(1);\n}\n";

const QUEUE: &str = "/// Jobs in the order they came.\npub struct Queue {\n    jobs: Vec<u32>,\n}\n\nimpl Queue {\n    pub fn new() -> Queue {\n        Queue { jobs: Vec::new() }\n    }\n\n    pub fn push(&mut self, job: u32) {\n        self.jobs.push(job);\n    }\n\n    pub fn pop(&mut self) -> Option<u32> {\n        self.jobs.pop()\n    }\n}\n";

const RETRY: &str = "/// How many times a job runs before it's given up on.\npub const MAX_ATTEMPTS: u32 = 5;\n\n/// The wait before attempt `attempt`, in seconds.\npub fn backoff(attempt: u32) -> u64 {\n    2u64.pow(attempt.min(MAX_ATTEMPTS))\n}\n";

/// A home of the test's own: the repository, the fixtures, the fake's
/// calls, and lattice's directories, all under one temporary directory.
struct Home {
    dir: TempDir,
}

impl Home {
    fn new() -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        for dir in ["bin", "fixtures", "repo/src"] {
            fs::create_dir_all(home.path(dir)).unwrap();
        }
        let repo = home.path("repo");
        git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("README.md"), "# jobs\n\nA tiny job queue.\n").unwrap();
        fs::write(repo.join("src/main.rs"), MAIN).unwrap();
        fs::write(repo.join("src/queue.rs"), QUEUE).unwrap();
        fs::write(repo.join("src/retry.rs"), RETRY).unwrap();
        commit(&repo, "first");
        home.fake_claude();
        home.fixtures();
        home
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Writes the fixture `name` the fake answers with.
    fn fixture(&self, name: &str, value: Value) {
        fs::write(
            self.path("fixtures").join(format!("{name}.json")),
            serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
    }

    /// The fixtures of a build that goes well, but for a link at the wrong
    /// lines, which the checks move, and a diagram that doesn't read, which
    /// the writer is asked to put right.
    fn fixtures(&self) {
        let sub =
            |files: &[&str]| json!({"title": "", "about": "", "kind": "component", "files": files});
        let mut queue = sub(&["src/queue.rs"]);
        queue["title"] = json!("The Queue");
        queue["kind"] = json!("data-model");
        let mut retry = sub(&["src/retry.rs"]);
        retry["title"] = json!("Retrying");
        let mut start = sub(&["src/main.rs"]);
        start["title"] = json!("The Entry Point");
        self.fixture(
            "plan",
            json!({"overview": "A job queue.", "sections": [
                {"title": "Queueing Jobs", "about": "The queue and its retries.", "subsections": [queue, retry]},
                {"title": "Starting Up", "about": "Where it starts.", "subsections": [start]}
            ]}),
        );
        let card = |label: &str| json!({"mermaid": format!("flowchart TD\n  a[\"{label}\"] -->|calls| b[\"Queue\"]"), "caption": "How it fits."});
        self.fixture(
            "sub-the-queue",
            json!({
                "body_md": "[`Queue`](code:src/queue.rs#L2-L4) keeps jobs in order; `push` adds one and [`pop`](code:src/queue.rs#L1) takes the last ([queue.rs:15-17](code:src/queue.rs#L15-L17)). [Retrying](#retrying) says what happens on failure.\n\nSources: [queue.rs:2-18](code:src/queue.rs#L2-L18)",
                "diagram": card("Queue.push"),
                "failure": null
            }),
        );
        self.fixture(
            "sub-retrying",
            json!({
                "body_md": "[`backoff`](code:src/retry.rs#L5-L7) doubles the wait, up to `MAX_ATTEMPTS`.\n\nSources: [retry.rs:1-7](code:src/retry.rs#L1-L7)",
                "diagram": {"mermaid": "pie\n  \"a\": 1", "caption": "Not a flowchart."},
                "failure": null
            }),
        );
        self.fixture(
            "fix-retrying",
            json!({
                "body_md": "[`backoff`](code:src/retry.rs#L5-L7) doubles the wait, up to `MAX_ATTEMPTS`.\n\nSources: [retry.rs:1-7](code:src/retry.rs#L1-L7)",
                "diagram": card("backoff"),
                "failure": null
            }),
        );
        self.fixture(
            "sub-the-entry-point",
            json!({
                "body_md": "[`main`](code:src/main.rs#L4-L8) makes a `Queue` and calls `backoff`; see [The Queue](#the-queue) and [nowhere](#nowhere).",
                "diagram": card("main"),
                "failure": null
            }),
        );
        // Put right, the link to no part of the page is there still.
        self.fixture(
            "fix-the-entry-point",
            json!({
                "body_md": "[`main`](code:src/main.rs#L4-L8) makes a `Queue` and calls `backoff`; see [The Queue](#the-queue) and [nowhere](#nowhere).",
                "diagram": card("main"),
                "failure": null
            }),
        );
        for id in ["queueing-jobs", "starting-up"] {
            self.fixture(
                &format!("section-{id}"),
                json!({"summary_md": format!("The section {id}, over [The Queue](#the-queue)."), "diagram": card(id), "failure": null}),
            );
        }
        self.fixture(
            "overview",
            json!({"summary_md": "A job queue: [Queueing Jobs](#queueing-jobs) and [Starting Up](#starting-up), from [`main`](code:src/main.rs#L4).", "diagram": card("overview"), "failure": null}),
        );
    }

    /// A `claude` that answers each run by what it's asked, from the
    /// fixtures, and writes down a line for each call in `calls`. A
    /// subsection whose `fail-<id>` file is there gets an answer saying
    /// the code couldn't be read.
    fn fake_claude(&self) {
        let fixtures = self.path("fixtures");
        let script = format!(
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "2.1.295 (Claude Code)"; exit 0; fi
F={fixtures}
msg=$(cat)
first=$(printf '%s\n' "$msg" | head -1)
echo '{{"type":"system","subtype":"init","session_id":"0b5c-77","model":"claude-sonnet-5-5"}}'
answer() {{
  printf '%s\n' "$2" >> "$F/calls"
  out=$(cat "$F/$1.json")
  printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"0b5c-77","total_cost_usd":{COST},"modelUsage":{{"claude-sonnet-5-5":{{"costUSD":{COST}}}}},"structured_output":'"$out"'}}'
  exit 0
}}
if [ -f marker.txt ]; then
  printf 'preflight\n' >> "$F/calls"
  said=$(cat marker.txt)
  echo '{{"type":"result","subtype":"success","is_error":false,"result":"'"$said"'","session_id":"0b5c-77","total_cost_usd":0.01,"modelUsage":{{"claude-sonnet-5-5":{{"costUSD":0.01}}}}}}'
  exit 0
fi
update=no
if printf '%s\n' "$@" | grep -q '^This is an update'; then update=yes; fi
fix=no
if printf '%s\n' "$msg" | grep -q '^The problems found:'; then fix=yes; fi
case "$first" in
  "Plan the wiki of"*) answer plan plan ;;
  "Write the subsection"*)
    id=$(printf '%s\n' "$first" | sed -n 's/^Write the subsection "[^"]*" (#\([^)]*\)).*/\1/p')
    if [ -f "$F/fail-$id" ]; then answer failed "failed $id"; fi
    if [ $fix = yes ]; then answer "fix-$id" "fix $id"; fi
    if [ $update = yes ] && [ -f "$F/update-$id.json" ]; then answer "update-$id" "update $id"; fi
    answer "sub-$id" "sub $id" ;;
  "Write the summary of the section"*)
    id=$(printf '%s\n' "$first" | sed -n 's/^Write the summary of the section "[^"]*" (#\([^)]*\)).*/\1/p')
    answer "section-$id" "section $id" ;;
  "Write the overview"*) answer overview overview ;;
esac
echo "the fake doesn't know: $first" >&2
exit 3
"#,
            fixtures = fixtures.display()
        );
        let path = self.path("bin/claude");
        fs::write(&path, script).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&path, permissions).unwrap();
        self.fixture(
            "failed",
            json!({"body_md": "", "diagram": {"mermaid": "", "caption": ""}, "failure": "the files can't be read"}),
        );
    }

    /// The calls the fake was asked, a line each, and forgets them.
    fn calls(&self) -> Vec<String> {
        let path = self.path("fixtures/calls");
        let calls = fs::read_to_string(&path).unwrap_or_default();
        let _ = fs::remove_file(&path);
        calls.lines().map(str::to_string).collect()
    }

    fn repo(&self) -> Repo {
        let path = self.path("repo");
        Repo {
            key: "jobs".into(),
            name: "jobs".into(),
            source: Source::Local { path },
            added: Timestamp(0),
        }
    }

    /// A job of `kind` on the repository at its `HEAD`, writing in its work
    /// directory, from `latest`.
    fn job(&self, kind: JobKind, config: Config, latest: Option<Version>) -> JobSpec {
        let root = self.path("repo");
        let commit = git(&root, &["rev-parse", "HEAD"]).trim().to_string();
        JobSpec {
            id: 1,
            kind,
            repo: self.repo(),
            config,
            root,
            commit,
            branch: Some("main".into()),
            work: self.path("data/lattice/repos/jobs/work"),
            latest,
        }
    }

    /// Runs `job` with this home's environment, and gives back what it
    /// built, or why not, and what it reported.
    fn run(&self, job: &JobSpec) -> (anyhow::Result<lattice::generator::Built>, Vec<Report>) {
        self.within(|| {
            let mut reports = Vec::new();
            let built = Claude.run(job, &mut |report| reports.push(report), &Cancel::new());
            (built, reports)
        })
    }

    /// Runs `work` with this home's environment: its fake first on the
    /// `PATH`, and lattice's directories its own.
    fn within<T>(&self, work: impl FnOnce() -> T) -> T {
        let _held = ENV_HELD.lock().unwrap_or_else(|err| err.into_inner());
        let vars = [
            (
                "PATH",
                format!("{}:{SYSTEM_PATH}", self.path("bin").display()),
            ),
            ("HOME", self.dir.path().display().to_string()),
            ("XDG_CONFIG_HOME", self.path("config").display().to_string()),
            ("XDG_DATA_HOME", self.path("data").display().to_string()),
            ("XDG_CACHE_HOME", self.path("cache").display().to_string()),
            ("LATTICE_NO_DOWNLOAD", "1".to_string()),
            ("GIT_CONFIG_GLOBAL", "/dev/null".to_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".to_string()),
        ];
        let before: Vec<_> = vars
            .iter()
            .map(|(name, _)| (*name, std::env::var_os(name)))
            .collect();
        // SAFETY: the tests that change the environment hold ENV_HELD while
        // they do, and nothing else here reads it meanwhile.
        unsafe {
            for (name, value) in &vars {
                std::env::set_var(name, value);
            }
        }
        let out = work();
        unsafe {
            for (name, value) in before {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
        out
    }

    /// Makes what the job wrote version `n`, as the job's runner does.
    fn keep(&self, job: &JobSpec, n: u32, model: &str, cost_usd: f64) -> Version {
        let dir = self.path(&format!("data/lattice/repos/jobs/v{n}"));
        fs::rename(&job.work, &dir).unwrap();
        Version {
            n,
            commit: job.commit.clone(),
            branch: job.branch.clone(),
            model: model.into(),
            at: Timestamp::now(),
            cost_usd,
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(
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
}

fn config() -> Config {
    Config {
        concurrency: 2,
        ..Config::default()
    }
}

/// The phases the reports went through, each once, in order.
fn phases(reports: &[Report]) -> Vec<Phase> {
    let mut phases = Vec::new();
    for report in reports {
        if let Report::Progress(progress) = report
            && phases.last() != Some(&progress.phase)
        {
            phases.push(progress.phase);
        }
    }
    phases
}

fn logged(reports: &[Report]) -> String {
    reports
        .iter()
        .filter_map(|report| match report {
            Report::Log(line) => Some(line.as_str()),
            Report::Progress(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_build_plans_writes_checks_links_and_sums_up() {
    let home = Home::new();
    let job = home.job(JobKind::Build, config(), None);
    let (built, reports) = home.run(&job);
    let built = built.unwrap_or_else(|err| panic!("{err:#}\n{}", logged(&reports)));
    let mut calls = home.calls();
    calls.sort();
    assert_eq!(
        calls,
        [
            "fix retrying",
            "fix the-entry-point",
            "overview",
            "plan",
            "preflight",
            "section queueing-jobs",
            "section starting-up",
            "sub retrying",
            "sub the-entry-point",
            "sub the-queue"
        ]
    );
    assert_eq!(built.model, "claude-sonnet-5-5");
    assert!(
        (built.cost_usd - (9.0 * COST + 0.01)).abs() < 0.001,
        "{}",
        built.cost_usd
    );
    assert_eq!(
        phases(&reports),
        [Phase::Plan, Phase::Write, Phase::Link, Phase::Overview]
    );
    let wiki = Wiki::read(&job.work.join("wiki.json")).unwrap();
    assert_eq!(wiki.repo.name, "jobs");
    assert_eq!(wiki.repo.commit, job.commit);
    assert_eq!(wiki.generated.by, "Claude Sonnet 5.5");
    let ids: Vec<&str> = wiki.sections.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["queueing-jobs", "starting-up"]);
    assert_eq!((wiki.subsections(), wiki.diagrams()), (3, 6));
    // The link to the wrong lines was moved to `pop`, a code span was
    // linked, and the citation kept.
    let queue = &wiki.subsection("the-queue").unwrap().body_md;
    assert!(
        queue.contains("[`pop`](code:src/queue.rs#L15-L17)"),
        "{queue}"
    );
    assert!(queue.contains("[`push`](code:src/queue.rs#L11)"), "{queue}");
    assert!(
        queue.contains("[queue.rs:15-17](code:src/queue.rs#L15-L17)"),
        "{queue}"
    );
    // The diagram that didn't read was written again.
    let retry = wiki.subsection("retrying").unwrap();
    assert!(retry.diagram.as_ref().unwrap().mermaid.contains("backoff"));
    assert!(
        retry
            .body_md
            .contains("[`MAX_ATTEMPTS`](code:src/retry.rs#L2)"),
        "{}",
        retry.body_md
    );
    // A link to no part of the page was left as its words.
    let start = &wiki.subsection("the-entry-point").unwrap().body_md;
    assert!(
        start.contains("[The Queue](#the-queue) and nowhere."),
        "{start}"
    );
    assert!(
        wiki.section("starting-up")
            .unwrap()
            .summary_md
            .starts_with("The section starting-up")
    );
    assert!(
        wiki.overview
            .summary_md
            .contains("[Queueing Jobs](#queueing-jobs)")
    );
    let book: Value =
        serde_json::from_str(&fs::read_to_string(job.work.join("build.json")).unwrap()).unwrap();
    assert_eq!(book["built"]["commit"], job.commit.as_str());
    assert_eq!(book["built"]["tally"]["fixes"], 2);
    assert_eq!(book["built"]["tally"]["diagrams_fixed"], 1);
    assert!(book["built"]["blobs"]["the-queue"]["src/queue.rs"].is_string());
    assert!(book["run"].is_null());
    let log = logged(&reports);
    assert!(log.contains("[3/3]"), "{log}");
    assert!(log.contains("the checks moved"), "{log}");
}

#[test]
fn a_build_that_could_not_write_enough_stops_and_a_resume_writes_only_the_rest() {
    let home = Home::new();
    fs::write(home.path("fixtures/fail-retrying"), "").unwrap();
    let job = home.job(JobKind::Build, config(), None);
    let (built, reports) = home.run(&job);
    let err = format!("{:#}", built.unwrap_err());
    assert!(
        err.contains("1 of 3 subsections couldn't be written"),
        "{err}"
    );
    let log = logged(&reports);
    assert!(
        log.contains("is run once more: it said it couldn't do the work"),
        "{log}"
    );
    assert!(!job.work.join("wiki.json").exists());
    home.calls();
    fs::remove_file(home.path("fixtures/fail-retrying")).unwrap();
    let resume = home.job(JobKind::Resume, config(), None);
    let (built, reports) = home.run(&resume);
    built.unwrap_or_else(|err| panic!("{err:#}\n{}", logged(&reports)));
    let mut calls = home.calls();
    calls.sort();
    // Only what wasn't written is: no plan, no other writer.
    assert_eq!(
        calls,
        [
            "fix retrying",
            "overview",
            "preflight",
            "section queueing-jobs",
            "section starting-up",
            "sub retrying"
        ]
    );
    let wiki = Wiki::read(&resume.work.join("wiki.json")).unwrap();
    assert_eq!(wiki.subsections(), 3);
    // A resume with nothing stopped has nothing to carry on.
    let home = Home::new();
    let (built, _) = home.run(&home.job(JobKind::Resume, config(), None));
    assert!(format!("{:#}", built.unwrap_err()).contains("no stopped build to resume"));
}

#[test]
fn a_build_stops_at_its_budget_keeping_what_it_wrote() {
    let home = Home::new();
    let config = Config {
        concurrency: 1,
        budget_usd: 0.3,
        ..Config::default()
    };
    let job = home.job(JobKind::Build, config, None);
    let (built, _) = home.run(&job);
    let err = format!("{:#}", built.unwrap_err());
    assert!(err.contains("reached its budget of $0.30"), "{err}");
    assert!(err.contains("a resume carries on from there"), "{err}");
    let book: Value =
        serde_json::from_str(&fs::read_to_string(job.work.join("build.json")).unwrap()).unwrap();
    assert!(book["run"]["plan"].is_object());
    assert!(book["run"]["cost_usd"].as_f64().unwrap() <= 0.3);
}

#[test]
fn a_sync_writes_again_only_what_the_diff_touched() {
    let home = Home::new();
    let job = home.job(JobKind::Build, config(), None);
    let (built, reports) = home.run(&job);
    let built = built.unwrap_or_else(|err| panic!("{err:#}\n{}", logged(&reports)));
    let v1 = home.keep(&job, 1, &built.model, built.cost_usd);
    home.calls();
    // The queue gains lines above `pop`, and the retries don't change.
    let repo = home.path("repo");
    let queue = QUEUE.replace(
        "    pub fn pop",
        "    /// How many jobs wait.\n    pub fn len(&self) -> usize {\n        self.jobs.len()\n    }\n\n    pub fn pop",
    );
    fs::write(repo.join("src/queue.rs"), queue).unwrap();
    commit(&repo, "len");
    home.fixture(
        "update-the-queue",
        json!({"unchanged": false, "meaning_changed": true, "body_md": "[`Queue`](code:src/queue.rs#L2-L4) keeps jobs; [`len`](code:src/queue.rs#L15-L17) says how many wait.\n\nSources: [queue.rs:2-23](code:src/queue.rs#L2-L23)", "diagram": {"mermaid": "flowchart TD\n  a[\"Queue.len\"] --> b[\"Queue\"]", "caption": "Now with len."}, "failure": null}),
    );
    let sync = home.job(JobKind::Sync, config(), Some(v1));
    let (built, reports) = home.run(&sync);
    built.unwrap_or_else(|err| panic!("{err:#}\n{}", logged(&reports)));
    let mut calls = home.calls();
    calls.sort();
    // The one subsection whose file changed, its section, and the overview,
    // since its meaning changed.
    assert_eq!(
        calls,
        [
            "overview",
            "preflight",
            "section queueing-jobs",
            "update the-queue"
        ]
    );
    let wiki = Wiki::read(&sync.work.join("wiki.json")).unwrap();
    assert_eq!(wiki.repo.commit, sync.commit);
    assert!(
        wiki.subsection("the-queue")
            .unwrap()
            .body_md
            .contains("[`len`]")
    );
    // What wasn't written again is as it was, and the summary of the other
    // section too.
    let v1_wiki = Wiki::read(&home.path("data/lattice/repos/jobs/v1/wiki.json")).unwrap();
    assert_eq!(wiki.subsection("retrying"), v1_wiki.subsection("retrying"));
    assert_eq!(
        wiki.section("starting-up").unwrap().summary_md,
        v1_wiki.section("starting-up").unwrap().summary_md
    );
    let log = logged(&reports);
    assert!(log.contains("1 of 3 subsections changed since"), "{log}");
}

#[test]
fn a_sync_whose_writer_says_nothing_changed_keeps_the_text_with_its_links_moved() {
    let home = Home::new();
    let job = home.job(JobKind::Build, config(), None);
    let (built, _) = home.run(&job);
    let built = built.unwrap();
    let v1 = home.keep(&job, 1, &built.model, built.cost_usd);
    home.calls();
    let repo = home.path("repo");
    let queue = QUEUE.replace(
        "/// Jobs in the order they came.\n",
        "/// Jobs,\n/// in the order they came.\n",
    );
    fs::write(repo.join("src/queue.rs"), queue).unwrap();
    commit(&repo, "comment");
    home.fixture(
        "update-the-queue",
        json!({"unchanged": true, "meaning_changed": false, "body_md": "", "diagram": {"mermaid": "", "caption": ""}, "failure": null}),
    );
    let sync = home.job(JobKind::Sync, config(), Some(v1));
    let (built, reports) = home.run(&sync);
    built.unwrap_or_else(|err| panic!("{err:#}\n{}", logged(&reports)));
    // No meaning changed: no summary, no overview written again.
    assert_eq!(home.calls(), ["preflight", "update the-queue"]);
    let wiki = Wiki::read(&sync.work.join("wiki.json")).unwrap();
    let queue = &wiki.subsection("the-queue").unwrap().body_md;
    // `pop` moved down a line with the comment above it.
    assert!(
        queue.contains("[`pop`](code:src/queue.rs#L16-L18)"),
        "{queue}"
    );
}

/// A request to the server on `port`, from its own page, with `body` when
/// there's one: its status and what it answered.
fn request(port: u16, method: &str, path: &str, body: Option<&Value>) -> (u16, String) {
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((&answer, ""));
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, body.to_string())
}

/// The JSON the server on `port` answers to `method` on `path`.
fn json_of(port: u16, method: &str, path: &str, body: Option<&Value>) -> Value {
    let (status, answer) = request(port, method, path, body);
    assert_eq!(status, 200, "{method} {path}: {answer}");
    serde_json::from_str(&answer).unwrap_or_else(|_| panic!("{path}: {answer}"))
}

/// The names of the events of job `id` on the server on `port`, which
/// stream until it ends, and the data of its last.
fn events_of(port: u16, id: &Value) -> (Vec<String>, Value) {
    let (status, body) = request(port, "GET", &format!("/api/jobs/{id}/events"), None);
    assert_eq!(status, 200, "{body}");
    let mut names = Vec::new();
    let mut last = Value::Null;
    for event in body.split("\n\n") {
        let name = event.lines().find_map(|line| line.strip_prefix("event: "));
        let data = event.lines().find_map(|line| line.strip_prefix("data: "));
        if let (Some(name), Some(data)) = (name, data) {
            if names.last().is_none_or(|last: &String| last != name) {
                names.push(name.to_string());
            }
            last = serde_json::from_str(data).unwrap();
        }
    }
    (names, last)
}

#[test]
fn a_wiki_is_built_and_synced_through_the_job_api() {
    let home = Home::new();
    home.within(|| {
        let running = server::start(Ipv4Addr::LOCALHOST, Some(0), Arc::new(Claude)).unwrap();
        let port = running.port();
        let source = home.path("repo").display().to_string();
        let added = json_of(port, "POST", "/api/repos", Some(&json!({ "source": source })));
        let key = added["key"].as_str().unwrap().to_string();
        let job = json_of(
            port,
            "POST",
            &format!("/api/repos/{key}/jobs"),
            Some(&json!({"kind": "build", "model": null, "concurrency": null})),
        );
        let (names, last) = events_of(port, &job["id"]);
        assert_eq!(names.last().map(String::as_str), Some("done"), "{names:?} {last}");
        assert!(names.contains(&"progress".to_string()) && names.contains(&"log".to_string()));
        assert_eq!(last["version"], 1);
        let done = json_of(port, "GET", &format!("/api/jobs/{}", job["id"]), None);
        assert_eq!(done["state"], "done");
        assert_eq!(done["progress"]["phase"], "overview");
        let wiki = json_of(port, "GET", &format!("/api/repos/{key}/wiki"), None);
        assert_eq!(wiki["version"], 1);
        assert_eq!(wiki["sections"].as_array().unwrap().len(), 2);
        assert_eq!(wiki["generated"]["by"], "Claude Sonnet 5.5");
        let repos = json_of(port, "GET", "/api/repos", None);
        assert_eq!(repos[0]["versions"][0]["model"], "claude-sonnet-5-5");
        // A change, then a sync: version 2, with the change in it.
        let repo = home.path("repo");
        fs::write(repo.join("src/retry.rs"), RETRY.replace("5;", "7;")).unwrap();
        commit(&repo, "seven");
        home.fixture(
            "update-retrying",
            json!({"unchanged": false, "meaning_changed": false, "body_md": "[`MAX_ATTEMPTS`](code:src/retry.rs#L2) is 7 now.", "diagram": {"mermaid": "flowchart TD\n  a[\"backoff\"] --> b[\"MAX_ATTEMPTS\"]", "caption": "The cap."}, "failure": null}),
        );
        home.calls();
        let sync = json_of(
            port,
            "POST",
            &format!("/api/repos/{key}/jobs"),
            Some(&json!({"kind": "sync", "model": null, "concurrency": null})),
        );
        let (names, last) = events_of(port, &sync["id"]);
        assert_eq!(names.last().map(String::as_str), Some("done"), "{names:?} {last}");
        assert_eq!(last["version"], 2);
        assert_eq!(home.calls(), ["preflight", "update retrying"]);
        let wiki = json_of(port, "GET", &format!("/api/repos/{key}/wiki?version=2"), None);
        let retry = &wiki["sections"][0]["subsections"][1]["body_md"];
        assert!(retry.as_str().unwrap().contains("is 7 now"), "{retry}");
        running.stop();
    });
}
