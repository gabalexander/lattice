//! Drives the real binary, and the library's `claude` runner, against
//! directories of the test's own: its settings, its data and its cache in
//! a temporary directory, a fake `claude` first on the `PATH` that speaks
//! stream-json as Claude Code does, and never the user's real data nor the
//! network.

use lattice::cancel::Cancel;
use lattice::claude::{self, Ask, Event, Reason, Tools};
use lattice::index::Def;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const LATTICE: &str = env!("CARGO_BIN_EXE_lattice");

/// The library's runs find `claude` on this process's `PATH`, which only
/// one test at a time may set.
static PATH_HELD: Mutex<()> = Mutex::new(());

/// The system's own programs, after the fakes on a test's `PATH`.
const SYSTEM_PATH: &str = "/usr/bin:/bin";

/// A home of the test's own: its settings, data and cache under it, and a
/// `bin` directory for the fakes, put first on the `PATH`.
struct Home {
    dir: TempDir,
}

impl Home {
    fn new() -> Home {
        let home = Home {
            dir: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(home.bin()).unwrap();
        home
    }

    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn config_file(&self) -> PathBuf {
        self.dir.path().join("config/lattice/config.toml")
    }

    fn configure(&self, toml: &str) {
        std::fs::create_dir_all(self.config_file().parent().unwrap()).unwrap();
        std::fs::write(self.config_file(), toml).unwrap();
    }

    /// The `PATH`: the fakes first, then the system's own, for `sh`, `cat`
    /// and git, where no real `claude` is.
    fn path(&self) -> String {
        format!("{}:{SYSTEM_PATH}", self.bin().display())
    }

    /// Writes an executable called `name` into `bin`, running `script`.
    fn fake(&self, name: &str, script: &str) {
        let path = self.bin().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    /// A `claude` that writes down its arguments and its message, then
    /// answers as Claude Code would, with what reading `marker.txt` gives
    /// when there's one, its result the line `RESULT` says.
    fn fake_claude(&self) {
        let record = self.dir.path().join("record");
        self.fake(
            "claude",
            &format!(
                r#"if [ "$1" = "--version" ]; then echo "2.1.295 (Claude Code)"; exit 0; fi
printf '%s\n' "$@" > {record}.args
cat > {record}.message
printf '%s\n' "$PWD" > {record}.cwd
env > {record}.env
echo '{{"type":"system","subtype":"init","session_id":"0b5c-77","model":"claude-sonnet-5-5"}}'
if [ -f marker.txt ]; then
  echo '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Read","input":{{"file_path":"'"$PWD"'/marker.txt"}}}}]}}}}'
  said=$(cat marker.txt)
else
  said="It starts in main."
fi
echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"'"$said"'"}}]}}}}'
echo '{{"type":"result","subtype":"success","is_error":false,"result":"'"$said"'","session_id":"0b5c-77","total_cost_usd":0.03,"modelUsage":{{"claude-sonnet-5-5":{{"costUSD":0.03}}}}}}'
"#,
                record = record.display()
            ),
        );
    }

    /// What the fake `claude` wrote down: `args`, `message`, `cwd` or `env`.
    fn recorded(&self, what: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(format!("record.{what}"))).unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(LATTICE);
        command
            .args(args)
            .current_dir(self.dir.path())
            .env_clear()
            .env("HOME", self.dir.path())
            .env("PATH", self.path())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            .env("XDG_CACHE_HOME", self.dir.path().join("cache"))
            .env("LATTICE_NO_DOWNLOAD", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Runs git in `dir`, as the tests need it, with no settings but the
    /// test's, and gives what it printed.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=lattice",
                "-c",
                "user.email=lattice@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .env("HOME", self.dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
        stdout(&out).trim().to_string()
    }

    /// A git repository of the test's own called `name`, under `code/`,
    /// with a commit.
    fn repo(&self, name: &str) -> PathBuf {
        let root = self.dir.path().join("code").join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/x.rs"), "fn x() {}\n").unwrap();
        self.git(&root, &["init", "-q", "-b", "main"]);
        self.git(&root, &["add", "-A"]);
        self.git(&root, &["commit", "-q", "-m", "first"]);
        root
    }

    /// The fixture repository `tests/index/<name>/`, but its `spans.tsv`,
    /// committed in a directory of the test's own.
    fn fixture_repo(&self, name: &str) -> PathBuf {
        let repo = self.dir.path().join(name);
        copy_dir(&fixture(name), &repo);
        std::fs::remove_file(repo.join("spans.tsv")).unwrap();
        self.git(&repo, &["init", "-q"]);
        self.git(&repo, &["add", "."]);
        self.git(&repo, &["commit", "-q", "-m", name]);
        repo
    }

    /// `lattice index --json` on `repo`, its report.
    fn index(&self, repo: &Path, args: &[&str]) -> Value {
        let mut all = vec!["index", "--json", repo.to_str().unwrap()];
        all.extend(args);
        let out = self.run(&all);
        assert!(out.status.success(), "{}", stderr(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

/// `tests/index/<name>`, a fixture repository for the symbol index.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/index")
        .join(name)
}

/// Copies the directory `from`, all of it, to `to`.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        match path.is_dir() {
            true => copy_dir(&path, &to.join(entry.file_name())),
            false => {
                std::fs::copy(&path, to.join(entry.file_name())).unwrap();
            }
        }
    }
}

/// Where a lookup in an index's report links: its one definition's
/// target, or `-` for none.
fn linked(lookup: &Value) -> String {
    match lookup["found"].as_str() {
        Some("unique") => {
            let def: Def = serde_json::from_value(lookup["defs"][0].clone()).unwrap();
            def.target()
        }
        _ => "-".to_string(),
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn it_says_its_version_and_its_commands() {
    let home = Home::new();
    let out = home.run(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        format!("lattice {}\n", env!("CARGO_PKG_VERSION"))
    );
    let help = stdout(&home.run(&["--help"]));
    for command in [
        "serve", "open", "export", "build", "sync", "status", "index", "doctor",
    ] {
        assert!(help.contains(command), "{command}: {help}");
    }
    let wrong = home.run(&["frobnicate"]);
    assert_eq!(wrong.status.code(), Some(1));
}

#[test]
fn build_runs_a_job_here_and_status_says_how_it_went() {
    let home = Home::new();
    let said = stdout(&home.run(&["status"]));
    assert!(said.starts_with("no repositories yet"), "{said}");
    let root = home.repo("app");
    let commit = home.git(&root, &["rev-parse", "HEAD"]);
    // With no Claude Code, a build gets as far as checking it can read.
    let out = home.run(&["build", root.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let said = stderr(&out);
    assert!(
        said.starts_with("lattice: Claude Code couldn't read a file: couldn't start claude"),
        "{said}"
    );
    let said = stdout(&out);
    assert!(said.contains("job 1: build of app\n"), "{said}");
    assert!(said.contains(&format!("at {commit} on main\n")), "{said}");
    let said = stdout(&home.run(&["status"]));
    assert!(said.starts_with("app  app  ~/code/app\n"), "{said}");
    assert!(
        said.contains("  job 1: build failed: Claude Code couldn't read a file"),
        "{said}"
    );
    assert_eq!(said, stdout(&home.run(&["status", "app"])));
    let refused = home.run(&["sync", "app"]);
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(
        stderr(&refused),
        "lattice: there's no version to sync from: build one first\n"
    );
    // A build killed outright leaves its job running, which holds nothing.
    let db = lattice::db::Db::open_at(&home.dir.path().join("data/lattice/lattice.db")).unwrap();
    let left = db
        .add_job("app", lattice::db::JobKind::Build, None, None)
        .unwrap();
    assert!(db.start_job(left.id).unwrap());
    let again = home.run(&["build", "app"]);
    assert!(stdout(&again).contains(&format!("job {}: build of app", left.id + 1)));
    let left = db.job(left.id).unwrap().unwrap();
    assert_eq!(left.error.as_deref(), Some(lattice::db::INTERRUPTED));
    let unknown = stderr(&home.run(&["status", "nothing"]));
    assert!(
        unknown.contains("nothing isn't one of lattice's repositories"),
        "{unknown}"
    );
    let export = stderr(&home.run(&["export", "app", "site"]));
    assert_eq!(
        export,
        "lattice: app has no wiki yet: `lattice build app` writes one\n"
    );
}

#[test]
fn serve_runs_one_server_a_data_directory_and_stops_on_sigterm() {
    let home = Home::new();
    let mut server = home
        .command(&["serve", "--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(server.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    let port = port_in(&first);
    assert_eq!(
        first,
        format!("serving lattice at http://127.0.0.1:{port}/ (ctrl+c stops it)\n")
    );
    let (status, said) = get(port, "/api/server");
    assert_eq!(status, 200);
    assert!(said.contains(env!("CARGO_PKG_VERSION")), "{said}");
    let again = home.run(&["serve", "--port", "0"]);
    assert_eq!(again.status.code(), Some(1));
    assert_eq!(
        stderr(&again),
        format!(
            "lattice: a lattice server runs already, at http://127.0.0.1:{port}/: \
             `lattice serve --stop` stops it\n"
        )
    );
    // SAFETY: kill has no preconditions; it's the test's own server.
    unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    assert!(server.wait().unwrap().success());
    assert!(!home.dir.path().join("data/lattice/serve.json").exists());
}

#[test]
fn open_starts_a_server_once_and_over_ssh_says_how_to_reach_it() {
    let home = Home::new();
    let root = home.repo("app");
    // Over ssh, which also keeps the test from opening a browser.
    let open = || {
        let out = home
            .command(&["open", root.to_str().unwrap()])
            .env("SSH_TTY", "/dev/ttys999")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", stderr(&out));
        stdout(&out)
    };
    let said = open();
    let url = said.lines().last().unwrap().to_string();
    let port = port_in(&url);
    let pid = serving_pid(&home);
    let _server = Kill(pid);
    assert_eq!(url, format!("http://127.0.0.1:{port}/app"));
    assert!(
        said.contains(&format!("ssh -L {port}:127.0.0.1:{port} ")),
        "{said}"
    );
    assert_eq!(get(port, "/api/repos").0, 200);
    assert!(
        stdout(&home.run(&["status"])).starts_with("app  app"),
        "it was added"
    );
    // Once one runs, it's the one opened.
    assert_eq!(open().lines().last().unwrap(), url);
    assert_eq!(serving_pid(&home), pid);
    let stopped = home.run(&["serve", "--stop"]);
    assert_eq!(
        stdout(&stopped),
        format!("stopped the lattice server on port {port}\n")
    );
    let stopped = home.run(&["serve", "--stop"]);
    assert_eq!(stdout(&stopped), "no lattice server is running\n");
}

/// The port in what a server said: `http://127.0.0.1:<port>/…`.
fn port_in(said: &str) -> u16 {
    said.split("http://127.0.0.1:")
        .nth(1)
        .and_then(|rest| rest.split(['/', ' ', '\n']).next())
        .and_then(|port| port.parse().ok())
        .unwrap_or_else(|| panic!("it didn't say where: {said:?}"))
}

/// The pid `serve.json` says the server running has.
fn serving_pid(home: &Home) -> i32 {
    let serving = std::fs::read_to_string(home.dir.path().join("data/lattice/serve.json")).unwrap();
    let serving: serde_json::Value = serde_json::from_str(&serving).unwrap();
    serving["pid"].as_i64().unwrap() as i32
}

/// Kills the process it holds as it goes: a server a test started in the
/// background, stopped whatever the test does.
struct Kill(i32);

impl Drop for Kill {
    fn drop(&mut self) {
        // SAFETY: kill has no preconditions; it's the test's own server.
        unsafe { libc::kill(self.0, libc::SIGKILL) };
    }
}

/// What the server on `port` answers to a GET of `path`: its status and
/// its body.
fn get(port: u16, path: &str) -> (u16, String) {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
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

#[test]
fn doctor_checks_everything_and_asks_claude_to_read_a_file() {
    let home = Home::new();
    home.fake_claude();
    home.configure("model = \"opus\"\n");
    let out = home.run(&["doctor"]);
    let said = stdout(&out);
    assert!(out.status.success(), "{said}{}", stderr(&out));
    assert!(
        said.contains("ok    settings: ~/config/lattice/config.toml"),
        "{said}"
    );
    assert!(said.contains("ok    data: ~/data/lattice"), "{said}");
    assert!(said.contains("ok    git: git version"), "{said}");
    assert!(
        said.contains("ok    claude: 2.1.295 (Claude Code)"),
        "{said}"
    );
    assert!(
        said.contains("ok    reading: claude-sonnet-5-5 read a file ($0.030)"),
        "{said}"
    );
    assert!(home.dir.path().join("data/lattice/lattice.db").is_file());
    let args = home.recorded("args");
    assert!(args.starts_with("-p\n--restricted\n"), "{args}");
    assert!(args.contains("--model\nopus\n"), "{args}");
    assert!(args.contains("--tools\nRead,Grep,Glob\n"), "{args}");
    assert!(home.recorded("message").contains("marker.txt"));
    let cwd = home.recorded("cwd");
    assert!(cwd.contains("lattice-preflight-"), "{cwd}");
    assert!(!Path::new(cwd.trim()).exists(), "the file it read is gone");
}

#[test]
fn doctor_fails_when_claude_can_t_read_or_isn_t_there() {
    let home = Home::new();
    // A Claude that answers without reading.
    home.fake(
        "claude",
        r#"if [ "$1" = "--version" ]; then echo "2.1.295 (Claude Code)"; exit 0; fi
cat > /dev/null
echo '{"type":"result","subtype":"success","is_error":false,"result":"I cannot read files.","session_id":"x","total_cost_usd":0.01}'
"#,
    );
    let out = home.run(&["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    let said = stdout(&out);
    assert!(
        said.contains("FAIL  reading: Claude couldn't read a file"),
        "{said}"
    );
    assert!(said.contains("I cannot read files."), "{said}");
    assert!(stderr(&out).contains("1 of the checks failed"));
    std::fs::remove_file(home.bin().join("claude")).unwrap();
    let real = SYSTEM_PATH
        .split(':')
        .any(|dir| Path::new(dir).join("claude").exists());
    assert!(!real, "a real claude is on the tests' PATH");
    let out = home.run(&["doctor"]);
    let said = stdout(&out);
    assert!(said.contains("FAIL  claude: couldn't run claude"), "{said}");
    assert!(
        said.contains("FAIL  reading: couldn't start claude"),
        "{said}"
    );
    assert!(stderr(&out).contains("2 of the checks failed"));
}

#[test]
fn settings_that_make_no_sense_are_named() {
    let home = Home::new();
    home.fake_claude();
    home.configure("concurrency = 99\n");
    let out = home.run(&["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    let said = stdout(&out);
    assert!(said.contains("FAIL  settings: in "), "{said}");
    assert!(said.contains("concurrency is 99"), "{said}");
}

/// Runs `ask` in `dir` with the fake `claude` of `home`, gathering what it
/// says.
fn run_with(
    home: &Home,
    ask: &Ask,
    dir: &Path,
    cancel: &Cancel,
) -> (Result<claude::Answer, claude::Failed>, Vec<Event>, Duration) {
    run_in(home, ask, dir, cancel, &[])
}

/// [`run_with`], with the variables `env` set in this process meanwhile,
/// and how long the run took, waiting for the other tests' not counted.
fn run_in(
    home: &Home,
    ask: &Ask,
    dir: &Path,
    cancel: &Cancel,
    env: &[(&str, &str)],
) -> (Result<claude::Answer, claude::Failed>, Vec<Event>, Duration) {
    let _held = PATH_HELD.lock().unwrap_or_else(|err| err.into_inner());
    let started = Instant::now();
    let path = std::env::var_os("PATH");
    // SAFETY: the tests that change the environment hold PATH_HELD while
    // they do, and nothing else here reads it meanwhile.
    unsafe {
        std::env::set_var("PATH", home.path());
        for (name, value) in env {
            std::env::set_var(name, value);
        }
    }
    let mut events = Vec::new();
    let answered = claude::run(ask, dir, cancel, &mut |event| events.push(event));
    let took = started.elapsed();
    // SAFETY: as above.
    unsafe {
        for (name, _) in env {
            std::env::remove_var(name);
        }
        if let Some(path) = path {
            std::env::set_var("PATH", path);
        }
    }
    (answered, events, took)
}

#[test]
fn a_run_streams_what_claude_does_and_leaves_its_session_behind() {
    let home = Home::new();
    home.fake_claude();
    let repo = home.dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(repo.join("marker.txt"), "the word").unwrap();
    let ask = Ask {
        tools: Tools::Read,
        system: "Answer briefly.".into(),
        ..Ask::new("sonnet", "What does marker.txt say?")
    };
    let (answered, events, _) = run_in(&home, &ask, &repo, &Cancel::new(), &[("CLAUDECODE", "1")]);
    let answer = answered.unwrap();
    assert_eq!(answer.text, "the word");
    assert_eq!(answer.cost_usd, 0.03);
    assert_eq!(answer.conversation, "0b5c-77");
    assert_eq!(answer.model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(
        events,
        vec![
            Event::Started {
                conversation: "0b5c-77".into(),
                model: Some("claude-sonnet-5-5".into())
            },
            Event::Tool {
                name: "Read".into(),
                path: Some("marker.txt".into()),
                detail: None
            },
            Event::Text("the word".into()),
        ]
    );
    assert_eq!(home.recorded("message"), "What does marker.txt say?");
    assert!(
        !home.recorded("env").contains("CLAUDECODE="),
        "it isn't part of the session that started lattice"
    );
}

#[test]
fn a_run_cancelled_or_out_of_time_is_stopped_with_what_it_started() {
    let home = Home::new();
    let pids = home.dir.path().join("pids");
    home.fake(
        "claude",
        &format!(
            "sleep 60 &\necho $! > {pids}\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"s\"}}'\nwait\n",
            pids = pids.display()
        ),
    );
    let cancel = Cancel::new();
    let later = cancel.clone();
    let started_it = pids.clone();
    // Cancelled once it has started what it runs.
    std::thread::spawn(move || {
        while !started_it.exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
        later.cancel();
    });
    let started = Instant::now();
    let (answered, _, _) = run_with(&home, &Ask::new("sonnet", "hi"), home.dir.path(), &cancel);
    let failed = answered.unwrap_err();
    assert_eq!(failed.reason, Reason::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(10));
    let sleeper: i32 = std::fs::read_to_string(&pids)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: kill with no signal only asks whether the process is there.
    let alive = || unsafe { libc::kill(sleeper, 0) } == 0;
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(), "what it started was stopped too");
    let ask = Ask {
        timeout: Duration::from_millis(300),
        ..Ask::new("sonnet", "hi")
    };
    let (answered, _, _) = run_with(&home, &ask, home.dir.path(), &Cancel::new());
    assert_eq!(answered.unwrap_err().reason, Reason::TimedOut);
}

#[test]
fn a_run_that_fails_for_a_passing_reason_is_tried_again() {
    let home = Home::new();
    let tries = home.dir.path().join("tries");
    // Overloaded the first time, answering the second.
    home.fake(
        "claude",
        &format!(
            r#"cat > /dev/null
if [ -f {tries} ]; then
  echo '{{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"s","total_cost_usd":0.02}}'
else
  touch {tries}
  echo '{{"type":"result","subtype":"success","is_error":true,"result":"API Error: 529 Overloaded","session_id":"s","total_cost_usd":0.01}}'
fi
"#,
            tries = tries.display()
        ),
    );
    let once = Ask::new("sonnet", "hi");
    let (answered, _, _) = run_with(&home, &once, home.dir.path(), &Cancel::new());
    assert_eq!(answered.unwrap_err().reason, Reason::Passing);
    std::fs::remove_file(&tries).unwrap();
    let again = Ask {
        retries: 1,
        ..Ask::new("sonnet", "hi")
    };
    let (answered, _, _) = run_with(&home, &again, home.dir.path(), &Cancel::new());
    let answer = answered.unwrap();
    assert_eq!(answer.text, "done");
    assert!(
        (answer.cost_usd - 0.03).abs() < 1e-9,
        "both tries are counted"
    );
}

#[test]
fn a_claude_that_refuses_its_arguments_says_why() {
    let home = Home::new();
    home.fake(
        "claude",
        "cat > /dev/null\necho \"error: unknown option '--restricted'\" >&2\nexit 1\n",
    );
    let ask = Ask {
        retries: 3,
        ..Ask::new("sonnet", "hi")
    };
    let (answered, _, took) = run_with(&home, &ask, home.dir.path(), &Cancel::new());
    let failed = answered.unwrap_err();
    assert_eq!(failed.reason, Reason::Other, "it isn't tried again");
    assert_eq!(
        failed.why,
        "claude said: error: unknown option '--restricted'"
    );
    assert!(took < Duration::from_secs(2));
}

#[test]
fn the_index_links_each_language_s_spans_where_they_re_defined() {
    let home = Home::new();
    let mut wrong = Vec::new();
    for language in [
        "rust",
        "go",
        "python",
        "typescript",
        "java",
        "cpp",
        "keywords",
    ] {
        let repo = home.fixture_repo(language);
        let listed = std::fs::read_to_string(fixture(language).join("spans.tsv")).unwrap();
        // The spans, by the files they're near.
        let mut near: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
        for line in listed.lines().filter(|line| !line.starts_with('#')) {
            let [span, files, target] = line.split('\t').collect::<Vec<_>>()[..] else {
                panic!("{language}: {line:?} isn't a span, its files and its target");
            };
            near.entry(files).or_default().push((span, target));
        }
        for (files, spans) in near {
            let mut args: Vec<&str> = Vec::new();
            for file in files.split(',').filter(|file| *file != "-") {
                args.extend(["--near", file]);
            }
            args.push("--");
            args.extend(spans.iter().map(|(span, _)| *span));
            let report = home.index(&repo, &args);
            let lookups = report["lookups"].as_array().unwrap();
            for ((span, target), lookup) in spans.iter().zip(lookups) {
                let got = linked(lookup);
                if got != *target {
                    wrong.push(format!(
                        "{language}: {span:?} near {files}: {got}, not {target}"
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_index_says_how_each_language_was_indexed_and_reads_again_only_what_changed() {
    let home = Home::new();
    let repo = home.fixture_repo("rust");
    let out = home.run(&["index", repo.to_str().unwrap()]);
    let said = stdout(&out);
    assert!(out.status.success(), "{said}{}", stderr(&out));
    assert!(said.starts_with("indexed 4 files, "), "{said}");
    assert!(
        said.contains(
            "Rust: syntactic (3 files, 41 definitions): rust-analyzer isn't installed \
             (rustup component add rust-analyzer)"
        ),
        "{said}"
    );
    let caches = home.dir.path().join("cache/lattice/index");
    let kept: Vec<PathBuf> = (std::fs::read_dir(&caches).unwrap())
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let syntax = kept[0].join("syntax.json");
    // What's kept is read in place of the blob: a blob kept as having
    // something else is taken at its word.
    let text = std::fs::read_to_string(&syntax).unwrap();
    assert!(text.contains("SessionSettings"));
    std::fs::write(&syntax, text.replace("SessionSettings", "KeptFromBefore")).unwrap();
    let look = |spans: &[&str]| -> Vec<String> {
        let mut args = vec!["--"];
        args.extend(spans);
        let report = home.index(&repo, &args);
        report["lookups"]
            .as_array()
            .unwrap()
            .iter()
            .map(linked)
            .collect()
    };
    assert_eq!(look(&["KeptFromBefore"]), ["src/config.rs#L19-L21"]);
    // A file that changed is read again; the others are still the cache's.
    let session = repo.join("src/session.rs");
    let mut text = std::fs::read_to_string(&session).unwrap();
    text.push_str("\npub fn ring_twice() {}\n");
    std::fs::write(&session, text).unwrap();
    home.git(&repo, &["commit", "-q", "-am", "twice"]);
    assert_eq!(
        look(&["ring_twice", "KeptFromBefore", "Session::stop"]),
        [
            "src/session.rs#L37",
            "src/config.rs#L19-L21",
            "src/session.rs#L16-L18"
        ]
    );
    // An earlier commit is indexed as it was.
    let report = home.index(&repo, &["--commit", "HEAD~1", "--", "ring_twice"]);
    assert_eq!(linked(&report["lookups"][0]), "-");
    let none = home.run(&[
        "index",
        repo.to_str().unwrap(),
        "--commit",
        "no-such-commit",
    ]);
    assert_eq!(none.status.code(), Some(1));
    assert!(
        stderr(&none).contains("there's no commit no-such-commit to index"),
        "{}",
        stderr(&none)
    );
    let outside = home.run(&["index", home.bin().to_str().unwrap()]);
    assert!(
        stderr(&outside).contains("isn't in a git checkout"),
        "{}",
        stderr(&outside)
    );
}

#[test]
fn a_scip_indexer_makes_its_language_precise_and_runs_again_only_once_its_files_change() {
    let home = Home::new();
    let repo = home.fixture_repo("rust");
    let runs = home.dir.path().join("indexer.runs");
    // rust-analyzer as it's run, writing what it wrote on the fixture.
    home.fake(
        "rust-analyzer",
        &format!(
            r#"if [ "$1" = "--version" ]; then echo "rust-analyzer 0.3.3073"; exit 0; fi
[ "$1" = scip ] && [ "$2" = . ] && [ "$3" = --output ] || exit 2
cp '{scip}' "$4"
echo "$PWD" >> '{runs}'
"#,
            scip = fixture("rust.scip").display(),
            runs = runs.display()
        ),
    );
    let look = || {
        home.index(
            &repo,
            &["--near", "src/main.rs", "--", "new", "Session::stop"],
        )
    };
    let report = look();
    let rust = &report["languages"][0];
    assert_eq!(rust["language"], "Rust");
    assert_eq!(
        (rust["tier"].as_str(), rust["precise_files"].as_u64()),
        (Some("precise"), Some(3))
    );
    let note = rust["note"].as_str().unwrap();
    assert!(
        note.starts_with("by rust-analyzer 0.3.3073-standalone"),
        "{note}"
    );
    // `new` is ambiguous to a grammar, but main.rs calls `Session::new`.
    let lookups = report["lookups"].as_array().unwrap();
    assert_eq!(linked(&lookups[0]), "src/session.rs#L8-L13");
    assert_eq!(lookups[0]["defs"][0]["precise"], true);
    assert_eq!(linked(&lookups[1]), "src/session.rs#L16-L18");
    // It ran in the checkout, which is at the commit.
    let ran = std::fs::read_to_string(&runs).unwrap();
    assert_eq!(
        ran.lines().collect::<Vec<_>>(),
        [repo.canonicalize().unwrap().to_str().unwrap()]
    );
    look();
    assert_eq!(
        std::fs::read_to_string(&runs).unwrap(),
        ran,
        "what it said is kept"
    );
    std::fs::write(repo.join("README.md"), "# bell\n").unwrap();
    home.git(&repo, &["add", "."]);
    home.git(&repo, &["commit", "-q", "-m", "readme"]);
    look();
    assert_eq!(
        std::fs::read_to_string(&runs).unwrap(),
        ran,
        "nothing it reads changed"
    );
    let without = home.index(&repo, &["--near", "src/main.rs", "--", "new"]);
    assert_eq!(linked(&without["lookups"][0]), "src/session.rs#L8-L13");
    home.configure("[index]\nprecise = false\n");
    let without = home.index(&repo, &["--near", "src/main.rs", "--", "new"]);
    assert_eq!(linked(&without["lookups"][0]), "-", "a grammar can't tell");
    assert_eq!(without["languages"][0]["tier"], "syntactic");
}

#[test]
fn an_indexer_that_fails_or_hangs_leaves_its_language_to_its_grammar() {
    let home = Home::new();
    let repo = home.fixture_repo("rust");
    home.fake(
        "rust-analyzer",
        r#"if [ "$1" = "--version" ]; then echo "rust-analyzer 0.3.3073"; exit 0; fi
echo "error: no cargo here" >&2
exit 101
"#,
    );
    let report = home.index(&repo, &["--", "Session::stop"]);
    let note = report["languages"][0]["note"].as_str().unwrap();
    assert!(
        note.starts_with(
            "rust-analyzer failed (exit status: 101): error: no cargo here, so the grammar \
             read it instead; its log is "
        ),
        "{note}"
    );
    assert_eq!(report["languages"][0]["tier"], "syntactic");
    assert_eq!(linked(&report["lookups"][0]), "src/session.rs#L16-L18");
    home.fake(
        "rust-analyzer",
        r#"if [ "$1" = "--version" ]; then echo "rust-analyzer 0.3.3073"; exit 0; fi
sleep 30
"#,
    );
    home.configure("[index]\nindexer_timeout_secs = 1\n");
    let started = Instant::now();
    let report = home.index(&repo, &["--", "Session::stop"]);
    assert!(started.elapsed() < Duration::from_secs(15));
    let note = report["languages"][0]["note"].as_str().unwrap();
    assert!(
        note.starts_with("rust-analyzer took longer than 1s, so the grammar read it instead"),
        "{note}"
    );
    assert_eq!(linked(&report["lookups"][0]), "src/session.rs#L16-L18");
}
