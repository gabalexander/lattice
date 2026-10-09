//! Running Claude Code: one `claude -p` for each thing lattice asks of the
//! model, a build's plan or one of its pages, or a question in the chat.
//! Everything lattice does with a model goes through here, and through
//! Claude Code alone: the user's own subscription, on their machine.
//!
//! The message goes on its standard input, which takes any length, where
//! an argument has a limit. What Claude does comes back as it happens, with
//! `--output-format stream-json`, read into [`Event`]s for whoever runs it:
//! the answer's text as it's written, each tool it uses and the file it
//! reads; the run's end is its [`Answer`], or why it [`Failed`], with what
//! it cost either way.
//!
//! Each run is locked down. `--restricted` takes away every tool that runs
//! a command or fetches from the web, confines the file tools to the
//! directory it runs in, and reads none of the user's, the project's or
//! anyone's settings files, so neither their permissions nor their hooks
//! reach it; `--tools` leaves it reading alone, Read, Grep and Glob, or no
//! tool at all, the rest refused besides, and `--permission-mode dontAsk`
//! refuses whatever wasn't allowed rather than asking; no MCP server, no
//! hook, a budget, a turn cap when one's given, a time limit, and no
//! conversation kept unless the chat follows up in it. A run that's
//! cancelled or takes too long is stopped, and whatever it started with
//! it: it runs in a process group of its own.
//!
//! A run that fails for a reason that passes, Claude overloaded or rate
//! limited, or its process dying, is tried again as [`Ask::retries`] says,
//! waiting longer each time, unless it's cancelled meanwhile.
//!
//! [`preflight`] checks, before a long build spends anything, that Claude
//! Code is there, logged in, takes the flags lattice gives it, and can read
//! a file with them.
//!
//! Adapted from crystal's `src/wiki_ask.rs`, `src/distill.rs` and its wiki
//! generator's runner (MIT), and from deepwiki-by-cc's
//! `src/lib/server/ai/claude-cli.ts` and `sandbox-preflight.ts` (MIT; see
//! THIRD_PARTY_NOTICES.md): its retries, its reading of the tokens used,
//! and its preflight.

use crate::cancel::Cancel;
use crate::printable;
use serde_json::Value;
use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// The program run, found on the `PATH`.
pub const PROGRAM: &str = "claude";

/// The tools that read the directory a run is in.
const READ_TOOLS: &str = "Read,Grep,Glob";

/// Every tool of Claude Code's own that could change something, run a
/// command or reach past the directory, refused besides, for a Claude that
/// would offer one anyway.
const REFUSED: &str = "Agent,Bash,BashOutput,Edit,KillShell,MultiEdit,NotebookEdit,Task,\
                       TodoWrite,WebFetch,WebSearch,Write";

/// Settings over none: no hook runs for it.
const SETTINGS: &str = r#"{"disableAllHooks":true}"#;

/// What ties a process to a Claude Code session, or a crystal one, which a
/// run lattice starts isn't part of, whatever session started lattice.
const SESSION_VARIABLES: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_PID",
    "CRYSTAL_SESSION",
    "CRYSTAL_SESSION_ID",
];

/// How often a run looks at whether it's been cancelled or is out of time.
const LOOK: Duration = Duration::from_millis(100);

/// How long a run may say nothing before its caller is told, with
/// [`Event::Quiet`], which a page uses to find out it's still there.
const QUIET: Duration = Duration::from_secs(10);

/// How many of the last lines Claude wrote on its standard error are kept,
/// to say why it failed.
const ERROR_LINES: usize = 5;

/// The first wait before a run is tried again, doubled each time after.
const RETRY_WAIT: Duration = Duration::from_secs(2);

/// What a run may do besides answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    /// Nothing: it answers from what it's told.
    None,
    /// Read, Grep and Glob, in the directory it runs in.
    Read,
}

/// The conversation a run is part of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conversation {
    /// None kept: nothing follows up on it.
    Forget,
    /// A new one, kept to follow up in, by the id its [`Answer`] says.
    Keep,
    /// The one with this id, carried on.
    Resume(String),
}

/// One run of Claude: what it's told, and how much it may do.
#[derive(Debug, Clone)]
pub struct Ask {
    /// As `claude --model` takes it: `sonnet`, `opus`, or a full name.
    pub model: String,
    /// Appended to Claude Code's own system prompt, which says how to use
    /// its tools; none when it's empty.
    pub system: String,
    /// The message, on its standard input.
    pub message: String,
    pub tools: Tools,
    /// The JSON Schema its answer must fit, which it then gives as
    /// [`Answer::value`].
    pub schema: Option<Value>,
    pub max_turns: Option<u32>,
    /// The most it may spend, in US dollars.
    pub budget_usd: f64,
    pub conversation: Conversation,
    /// Whether its text comes as it's written, a few words an
    /// [`Event::Text`], rather than a block at a time.
    pub stream: bool,
    /// The longest one try may take.
    pub timeout: Duration,
    /// How many times a try that fails for a reason that passes is made
    /// again.
    pub retries: u32,
}

impl Ask {
    /// An ask of `model` with `message`, with nothing else: no tools, no
    /// schema, no conversation kept, a dollar to spend, ten minutes, and
    /// no retries. Its fields say the rest.
    pub fn new(model: &str, message: &str) -> Ask {
        Ask {
            model: model.to_string(),
            system: String::new(),
            message: message.to_string(),
            tools: Tools::None,
            schema: None,
            max_turns: None,
            budget_usd: 1.0,
            conversation: Conversation::Forget,
            stream: false,
            timeout: Duration::from_secs(10 * 60),
            retries: 0,
        }
    }
}

/// What a run says as it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// It started: its conversation's id, and the model it runs.
    Started {
        conversation: String,
        model: Option<String>,
    },
    /// More of the answer's text. A block after another starts with a blank
    /// line.
    Text(String),
    /// It used a tool: its name, the file or directory it's about, from the
    /// directory the run is in where it's in it, and what else it asked,
    /// like a pattern.
    Tool {
        name: String,
        path: Option<String>,
        detail: Option<String>,
    },
    /// It's said nothing for a while, and is still working.
    Quiet,
}

/// How many tokens a run took, as its result counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    /// What it was sent: the tokens read from the cache and written to it
    /// count too, since most of what a run sends comes from there.
    pub input: u64,
    pub output: u64,
}

/// What a run answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// Its last message's text.
    pub text: String,
    /// Its structured output, when it was given a schema.
    pub value: Option<Value>,
    /// Its conversation's id, to resume when it was kept.
    pub conversation: String,
    /// What it cost, every try together.
    pub cost_usd: f64,
    /// The model that did most of the work, as Claude Code names it:
    /// `claude-sonnet-5-5`.
    pub model: Option<String>,
    pub tokens: Tokens,
}

/// Why a run failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// It reached its budget before it answered.
    Budget,
    /// It ran out of turns before it answered.
    Turns,
    /// It took longer than its time.
    TimedOut,
    Cancelled,
    /// Claude was overloaded or rate limited, or its process died: trying
    /// again later may well work.
    Passing,
    /// Anything else, like a flag this Claude Code doesn't know, or not
    /// being logged in.
    Other,
}

/// A run that failed: why, in words and as a [`Reason`], and what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Failed {
    pub why: String,
    pub reason: Reason,
    pub cost_usd: f64,
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.why)
    }
}

impl std::error::Error for Failed {}

impl Failed {
    fn new(reason: Reason, why: impl Into<String>) -> Failed {
        Failed {
            why: why.into(),
            reason,
            cost_usd: 0.0,
        }
    }
}

/// Claude's arguments for `ask`.
pub fn args(ask: &Ask) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--restricted",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        &ask.model,
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    let mut add = |more: &[&str]| args.extend(more.iter().map(|arg| arg.to_string()));
    if !ask.system.is_empty() {
        add(&["--append-system-prompt", &ask.system]);
    }
    match ask.tools {
        Tools::None => add(&["--tools", ""]),
        Tools::Read => add(&["--tools", READ_TOOLS, "--allowedTools", READ_TOOLS]),
    }
    add(&[
        "--disallowedTools",
        REFUSED,
        "--permission-mode",
        "dontAsk",
        "--strict-mcp-config",
        "--settings",
        SETTINGS,
        "--max-budget-usd",
        &format!("{:.2}", ask.budget_usd.max(0.01)),
    ]);
    if let Some(schema) = &ask.schema {
        add(&["--json-schema", &schema.to_string()]);
    }
    if let Some(turns) = ask.max_turns {
        add(&["--max-turns", &turns.to_string()]);
    }
    if ask.stream {
        add(&["--include-partial-messages"]);
    }
    match &ask.conversation {
        Conversation::Forget => add(&["--no-session-persistence"]),
        Conversation::Keep => {}
        Conversation::Resume(id) => add(&["--resume", id]),
    }
    args
}

/// Whether `id` looks like a conversation's id, a UUID, which is all
/// that's passed to `--resume`.
pub fn is_conversation(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Runs Claude on `ask` in `cwd`, telling `on_event` what it does as it
/// does it, until it answers, fails, or `cancel` says to stop; tried again
/// as [`Ask::retries`] says.
pub fn run(
    ask: &Ask,
    cwd: &Path,
    cancel: &Cancel,
    on_event: &mut dyn FnMut(Event),
) -> Result<Answer, Failed> {
    if let Conversation::Resume(id) = &ask.conversation
        && !is_conversation(id)
    {
        return Err(Failed::new(
            Reason::Other,
            format!("{} isn't a conversation", printable::line(id)),
        ));
    }
    let mut spent = 0.0;
    let mut wait = RETRY_WAIT;
    let mut tries = 0;
    loop {
        match attempt(ask, cwd, cancel, on_event) {
            Ok(mut answer) => {
                answer.cost_usd += spent;
                return Ok(answer);
            }
            Err(mut failed) => {
                spent += failed.cost_usd;
                let again = failed.reason == Reason::Passing && tries < ask.retries;
                if !again || !cancel.sleep(wait) {
                    if cancel.is_cancelled() {
                        failed.reason = Reason::Cancelled;
                    }
                    failed.cost_usd = spent;
                    return Err(failed);
                }
                tries += 1;
                wait *= 2;
            }
        }
    }
}

/// One try of [`run`].
fn attempt(
    ask: &Ask,
    cwd: &Path,
    cancel: &Cancel,
    on_event: &mut dyn FnMut(Event),
) -> Result<Answer, Failed> {
    if cancel.is_cancelled() {
        return Err(Failed::new(Reason::Cancelled, "it was cancelled"));
    }
    let mut command = Command::new(PROGRAM);
    command
        .args(args(ask))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A process group of its own, to stop all of it.
        .process_group(0);
    for name in SESSION_VARIABLES {
        command.env_remove(name);
    }
    let mut child = command.spawn().map_err(|err| {
        Failed::new(
            Reason::Other,
            format!("couldn't start claude ({err}): is Claude Code installed and on the PATH?"),
        )
    })?;
    let mut stdin = child.stdin.take().expect("its input is piped");
    let message = ask.message.clone();
    thread::spawn(move || {
        // Closed once written: that's the end of the message. A claude
        // that ends before it reads it all says why on its own.
        let _ = stdin.write_all(message.as_bytes());
    });
    let errors = read_errors(child.stderr.take().expect("its errors are piped"));
    let lines = read_lines(child.stdout.take().expect("its output is piped"));
    let mut reading = Reading::new(cwd, ask);
    let deadline = Instant::now() + ask.timeout;
    let mut heard = Instant::now();
    loop {
        if cancel.is_cancelled() {
            stop(&mut child);
            return Err(Failed::new(Reason::Cancelled, "it was cancelled"));
        }
        if Instant::now() >= deadline {
            stop(&mut child);
            let why = format!(
                "it took longer than {}s, and was stopped",
                ask.timeout.as_secs()
            );
            return Err(Failed::new(Reason::TimedOut, why));
        }
        match lines.recv_timeout(LOOK) {
            Ok(line) => {
                heard = Instant::now();
                for event in reading.take(&line) {
                    on_event(event);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if heard.elapsed() >= QUIET {
                    heard = Instant::now();
                    on_event(Event::Quiet);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait();
    let errors = errors.join().unwrap_or_default();
    match reading.ended.take() {
        // A result that says nothing of why it failed: Claude Code says
        // what it refused on its standard error.
        Some(ended) => ended.map_err(|failed| match (failed.why.is_empty(), errors.last()) {
            (true, Some(said)) => Failed {
                why: format!("claude said: {said}"),
                ..failed
            },
            (true, None) => Failed {
                why: "Claude stopped without answering".to_string(),
                ..failed
            },
            _ => failed,
        }),
        None => {
            let why = match errors.last() {
                Some(said) => format!("claude said: {said}"),
                None => match status {
                    Ok(status) => format!("claude ended without answering ({status})"),
                    Err(err) => format!("claude ended without answering: {err}"),
                },
            };
            // A claude that refused its arguments, or isn't logged in,
            // ends before it starts: trying again won't change that.
            let reason = if reading.started {
                Reason::Passing
            } else {
                Reason::Other
            };
            Err(Failed::new(reason, why))
        }
    }
}

/// Reads Claude's stream-json, a line at a time, into [`Event`]s, and its
/// result into how the run ended.
#[derive(Debug, Default)]
struct Reading {
    /// The directory it runs in, as it was given and as it is, which tools'
    /// paths are given from.
    roots: Vec<PathBuf>,
    /// The text streams in pieces: the whole messages that follow them say
    /// it again, and are passed over.
    partial: bool,
    /// Some text has been told: the next block of it starts a paragraph.
    spoke: bool,
    /// Its `system` `init` came: it got as far as starting.
    started: bool,
    /// Whether its answer is to fit a schema.
    schema: bool,
    budget_usd: f64,
    /// The run's end, once its result has come.
    ended: Option<Result<Answer, Failed>>,
}

impl Reading {
    fn new(root: &Path, ask: &Ask) -> Reading {
        let mut roots = vec![root.to_path_buf()];
        roots.extend(fs::canonicalize(root).ok().filter(|real| real != root));
        Reading {
            roots,
            schema: ask.schema.is_some(),
            budget_usd: ask.budget_usd,
            ..Reading::default()
        }
    }

    /// What `line` says.
    fn take(&mut self, line: &str) -> Vec<Event> {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match event["type"].as_str() {
            Some("system") if event["subtype"] == "init" => {
                self.started = true;
                vec![Event::Started {
                    conversation: text(&event["session_id"]),
                    model: event["model"].as_str().map(str::to_string),
                }]
            }
            Some("stream_event") => self.partial_event(&event["event"]),
            Some("assistant") => self.message(&event["message"]),
            Some("result") => {
                self.ended = Some(self.result(&event));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A piece of a message as it streams: text, or a new block of it.
    fn partial_event(&mut self, event: &Value) -> Vec<Event> {
        self.partial = true;
        match event["type"].as_str() {
            Some("content_block_start") if event["content_block"]["type"] == "text" => {
                self.paragraph().into_iter().collect()
            }
            Some("content_block_delta") if event["delta"]["type"] == "text_delta" => {
                match event["delta"]["text"].as_str() {
                    Some(text) if !text.is_empty() => {
                        self.spoke = true;
                        vec![Event::Text(text.to_string())]
                    }
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// A blank line between this block of text and the one before.
    fn paragraph(&mut self) -> Option<Event> {
        self.spoke.then(|| Event::Text("\n\n".to_string()))
    }

    /// A whole message: the tools it uses, and its text when it didn't
    /// stream.
    fn message(&mut self, message: &Value) -> Vec<Event> {
        let mut events = Vec::new();
        let blocks = message["content"].as_array().map(Vec::as_slice);
        for block in blocks.unwrap_or_default() {
            match block["type"].as_str() {
                Some("text") if !self.partial => {
                    let text = block["text"].as_str().unwrap_or_default();
                    if text.is_empty() {
                        continue;
                    }
                    events.extend(self.paragraph());
                    self.spoke = true;
                    events.push(Event::Text(text.to_string()));
                }
                Some("tool_use") => events.push(self.tool(block)),
                _ => {}
            }
        }
        events
    }

    fn tool(&self, block: &Value) -> Event {
        let input = &block["input"];
        let path = ["file_path", "path", "notebook_path"]
            .iter()
            .find_map(|key| input[key].as_str())
            .map(|path| self.below_root(path));
        let detail = ["pattern", "command"]
            .iter()
            .find_map(|key| input[key].as_str())
            .map(str::to_string);
        Event::Tool {
            name: block["name"].as_str().unwrap_or("tool").to_string(),
            path,
            detail,
        }
    }

    /// `path` from the directory the run is in, when it's in it.
    fn below_root(&self, path: &str) -> String {
        let path = Path::new(path);
        let below = self
            .roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok());
        match below {
            Some(below) if below.as_os_str().is_empty() => ".".to_string(),
            Some(below) => below.display().to_string(),
            None => path.display().to_string(),
        }
    }

    /// How the run ended, by its result: what it answered, or why it
    /// failed.
    fn result(&self, event: &Value) -> Result<Answer, Failed> {
        let cost_usd = event["total_cost_usd"].as_f64().unwrap_or(0.0);
        let said = event["result"].as_str().unwrap_or_default().trim();
        let failed = event["is_error"].as_bool().unwrap_or(false)
            || event["subtype"]
                .as_str()
                .is_some_and(|kind| kind != "success");
        if failed {
            let (reason, why) = match event["subtype"].as_str() {
                Some("error_max_budget_usd") => (
                    Reason::Budget,
                    format!(
                        "it reached its budget of ${:.2} before it answered",
                        self.budget_usd
                    ),
                ),
                Some("error_max_turns") => (
                    Reason::Turns,
                    "it ran out of turns before it answered".to_string(),
                ),
                _ if passes(said) => (Reason::Passing, said.to_string()),
                _ => (Reason::Other, said.to_string()),
            };
            return Err(Failed {
                why,
                reason,
                cost_usd,
            });
        }
        let value = match &event["structured_output"] {
            Value::Null if self.schema => match serde_json::from_str(said) {
                Ok(value) => Some(value),
                Err(_) => {
                    return Err(Failed {
                        why: "its answer had no structured output".to_string(),
                        reason: Reason::Other,
                        cost_usd,
                    });
                }
            },
            Value::Null => None,
            value => Some(value.clone()),
        };
        // The model that did most of the work: the one that cost most.
        let model = event["modelUsage"].as_object().and_then(|usage| {
            usage
                .iter()
                .max_by(|a, b| {
                    let cost = |v: &Value| v["costUSD"].as_f64().unwrap_or(0.0);
                    cost(a.1).total_cmp(&cost(b.1))
                })
                .map(|(name, _)| name.clone())
        });
        let usage = &event["usage"];
        let count = |key: &str| usage[key].as_u64().unwrap_or(0);
        Ok(Answer {
            text: said.to_string(),
            value,
            conversation: text(&event["session_id"]),
            cost_usd,
            model,
            tokens: Tokens {
                input: count("input_tokens")
                    + count("cache_creation_input_tokens")
                    + count("cache_read_input_tokens"),
                output: count("output_tokens"),
            },
        })
    }
}

/// `value` as text, or empty when it isn't a string.
fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

/// Whether what a failed run said is a reason that passes: Claude
/// overloaded, rate limited, or its API failing for a moment.
fn passes(said: &str) -> bool {
    let said = said.to_ascii_lowercase();
    [
        "overloaded",
        "rate limit",
        "rate_limit",
        "529",
        "503",
        "502",
        "500",
        "timed out",
    ]
    .iter()
    .any(|word| said.contains(word))
}

/// Claude's output, a line at a time, on a channel that closes as the
/// output does.
fn read_lines(stdout: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (send, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    lines
}

/// The last lines Claude wrote on its standard error, for saying why it
/// failed.
fn read_errors(stderr: impl Read + Send + 'static) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let mut kept = VecDeque::new();
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let line = printable::line(&line);
            if line.trim().is_empty() {
                continue;
            }
            if kept.len() == ERROR_LINES {
                kept.pop_front();
            }
            kept.push_back(line.trim().to_string());
        }
        kept.into()
    })
}

/// Stops `child` and whatever it started: asked first, then made to.
fn stop(child: &mut Child) {
    let group = -(child.id() as i32);
    // SAFETY: kill has no preconditions; the group is the child's own.
    unsafe {
        libc::kill(group, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // SAFETY: as above.
    unsafe {
        libc::kill(group, libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Checks that Claude Code can do what lattice asks of it with `model`,
/// before a long build spends on it: that `claude` is there, takes the
/// flags lattice runs it with, is logged in, and can read a file with
/// them. A file holding a word made up for the check is put in a directory
/// of its own, and Claude is asked what it holds without being told: only
/// reading it gives the word back, so an answer made up can't pass.
pub fn preflight(model: &str, cancel: &Cancel) -> Result<Answer, Failed> {
    let word = format!("lattice-preflight-{}", nonce());
    let dir = std::env::temp_dir().join(format!("lattice-preflight-{}", nonce()));
    let made = fs::create_dir(&dir).and_then(|()| fs::write(dir.join("marker.txt"), &word));
    if let Err(err) = made {
        let _ = fs::remove_dir_all(&dir);
        return Err(Failed::new(
            Reason::Other,
            format!("couldn't make a file to read in {}: {err}", dir.display()),
        ));
    }
    let ask = Ask {
        tools: Tools::Read,
        max_turns: Some(4),
        budget_usd: 0.5,
        timeout: Duration::from_secs(3 * 60),
        ..Ask::new(
            model,
            "Read the file marker.txt in the current directory with your tools, and reply \
             with only what it holds, exactly.",
        )
    };
    let answered = run(&ask, &dir, cancel, &mut |_| {});
    let _ = fs::remove_dir_all(&dir);
    let answer = answered?;
    if !answer.text.contains(&word) {
        let said: String = answer.text.chars().take(200).collect();
        return Err(Failed {
            why: format!(
                "Claude couldn't read a file with the tools lattice gives it; it said: {}",
                printable::line(&said)
            ),
            reason: Reason::Other,
            cost_usd: answer.cost_usd,
        });
    }
    Ok(answer)
}

/// 16 random bytes, in hex.
fn nonce() -> String {
    let mut bytes = [0u8; 16];
    let read = fs::File::open("/dev/urandom").and_then(|mut file| file.read_exact(&mut bytes));
    if read.is_err() {
        // Unique enough for a directory's name and a word nobody guesses
        // without reading it.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let seed = now.as_nanos() ^ (u128::from(std::process::id()) << 64);
        bytes = seed.to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read(ask: &Ask, lines: &[&str]) -> (Vec<Event>, Option<Result<Answer, Failed>>) {
        let mut reading = Reading::new(Path::new("/code/app"), ask);
        let events = lines.iter().flat_map(|line| reading.take(line)).collect();
        (events, reading.ended)
    }

    fn after<'a>(args: &'a [String], flag: &str) -> &'a str {
        let at = args.iter().position(|arg| arg == flag).unwrap_or_else(|| {
            panic!("{flag} is in {args:?}");
        });
        &args[at + 1]
    }

    #[test]
    fn a_run_reads_the_directory_and_nothing_else() {
        let ask = Ask {
            tools: Tools::Read,
            schema: Some(json!({"type": "object"})),
            max_turns: Some(12),
            budget_usd: 1.5,
            system: "You write.".into(),
            ..Ask::new("sonnet", "go")
        };
        let args = args(&ask);
        assert_eq!(
            args[..5],
            [
                "-p",
                "--restricted",
                "--output-format",
                "stream-json",
                "--verbose"
            ]
        );
        assert_eq!(after(&args, "--model"), "sonnet");
        assert_eq!(after(&args, "--append-system-prompt"), "You write.");
        assert_eq!(after(&args, "--tools"), "Read,Grep,Glob");
        assert_eq!(after(&args, "--allowedTools"), "Read,Grep,Glob");
        for refused in ["Bash", "Edit", "Write", "WebFetch", "WebSearch", "Agent"] {
            assert!(
                after(&args, "--disallowedTools").contains(refused),
                "{refused}"
            );
        }
        assert_eq!(after(&args, "--permission-mode"), "dontAsk");
        assert_eq!(after(&args, "--settings"), r#"{"disableAllHooks":true}"#);
        assert_eq!(after(&args, "--max-budget-usd"), "1.50");
        assert_eq!(after(&args, "--max-turns"), "12");
        assert_eq!(after(&args, "--json-schema"), r#"{"type":"object"}"#);
        assert!(args.contains(&"--strict-mcp-config".to_string()));
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(!args.contains(&"--include-partial-messages".to_string()));
    }

    #[test]
    fn without_tools_it_has_none_and_a_chat_keeps_its_conversation() {
        let args = args(&Ask::new("opus", "hi"));
        assert_eq!(after(&args, "--tools"), "");
        assert!(!args.contains(&"--allowedTools".to_string()));
        assert!(!args.contains(&"--append-system-prompt".to_string()));
        assert!(!args.contains(&"--json-schema".to_string()));
        let chat = Ask {
            stream: true,
            conversation: Conversation::Resume("ebf4cee3-7c34".into()),
            ..Ask::new("sonnet", "and then?")
        };
        let args = super::args(&chat);
        assert_eq!(after(&args, "--resume"), "ebf4cee3-7c34");
        assert!(args.contains(&"--include-partial-messages".to_string()));
        assert!(!args.contains(&"--no-session-persistence".to_string()));
        let kept = super::args(&Ask {
            conversation: Conversation::Keep,
            ..Ask::new("sonnet", "hi")
        });
        assert!(!kept.contains(&"--no-session-persistence".to_string()));
        assert!(!kept.contains(&"--resume".to_string()));
    }

    #[test]
    fn only_a_conversation_s_id_is_resumed() {
        assert!(is_conversation("ebf4cee3-7c34-4945-a896-201b1a2fbfc4"));
        assert!(!is_conversation("--dangerously-skip-permissions"));
        assert!(!is_conversation("abc"));
        let ask = Ask {
            conversation: Conversation::Resume("-p".into()),
            ..Ask::new("sonnet", "hi")
        };
        let refused = run(&ask, Path::new("/"), &Cancel::new(), &mut |_| {}).unwrap_err();
        assert_eq!(refused.reason, Reason::Other);
        assert!(refused.why.contains("isn't a conversation"));
    }

    #[test]
    fn the_answer_streams_its_tools_and_its_text() {
        let ask = Ask {
            stream: true,
            ..Ask::new("sonnet", "how?")
        };
        let (events, ended) = read(
            &ask,
            &[
                r#"{"type":"system","subtype":"init","session_id":"0b5c-77","model":"claude-sonnet-5-5"}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/code/app/src/main.rs"}}]}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"text","text":""}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"It starts in "}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"[main](code:src/main.rs#L3)."}}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"It starts in [main](code:src/main.rs#L3)."}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"fn run","path":"/code/app/src"}}]}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"text","text":""}}}"#,
                r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Then run."}}}"#,
                "not json",
                r#"{"type":"result","subtype":"success","is_error":false,"result":"Then run.","session_id":"0b5c-77","total_cost_usd":0.031,"usage":{"input_tokens":10,"cache_creation_input_tokens":200,"cache_read_input_tokens":3000,"output_tokens":45},"modelUsage":{"claude-haiku-5-5":{"costUSD":0.001},"claude-sonnet-5-5":{"costUSD":0.03}}}"#,
            ],
        );
        let tool = |name: &str, path: &str, detail: Option<&str>| Event::Tool {
            name: name.into(),
            path: Some(path.into()),
            detail: detail.map(String::from),
        };
        assert_eq!(
            events,
            vec![
                Event::Started {
                    conversation: "0b5c-77".into(),
                    model: Some("claude-sonnet-5-5".into())
                },
                tool("Read", "src/main.rs", None),
                Event::Text("It starts in ".into()),
                Event::Text("[main](code:src/main.rs#L3).".into()),
                tool("Grep", "src", Some("fn run")),
                Event::Text("\n\n".into()),
                Event::Text("Then run.".into()),
            ]
        );
        let answer = ended.unwrap().unwrap();
        assert_eq!(answer.text, "Then run.");
        assert_eq!(answer.conversation, "0b5c-77");
        assert_eq!(answer.cost_usd, 0.031);
        assert_eq!(answer.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(
            answer.tokens,
            Tokens {
                input: 3210,
                output: 45
            }
        );
        assert_eq!(answer.value, None);
    }

    #[test]
    fn whole_messages_are_the_text_when_nothing_streamed() {
        let (events, _) = read(
            &Ask::new("sonnet", "hi"),
            &[
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"One."}]}}"#,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Two."}]}}"#,
            ],
        );
        assert_eq!(
            events,
            vec![
                Event::Text("One.".into()),
                Event::Text("\n\n".into()),
                Event::Text("Two.".into())
            ]
        );
    }

    #[test]
    fn a_schema_s_answer_is_its_structured_output() {
        let ask = Ask {
            schema: Some(json!({"type": "object"})),
            ..Ask::new("sonnet", "plan")
        };
        let answered = |line: &str| read(&ask, &[line]).1.unwrap();
        let structured = answered(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"a":1},"total_cost_usd":0.25}"#,
        );
        assert_eq!(structured.unwrap().value, Some(json!({"a": 1})));
        let in_text = answered(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"{\"a\":2}","total_cost_usd":0.25}"#,
        );
        assert_eq!(in_text.unwrap().value, Some(json!({"a": 2})));
        let none = answered(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Sure!","total_cost_usd":0.25}"#,
        )
        .unwrap_err();
        assert_eq!((none.reason, none.cost_usd), (Reason::Other, 0.25));
        assert!(none.why.contains("no structured output"));
    }

    #[test]
    fn a_run_that_failed_says_why_and_what_it_cost() {
        let ask = Ask {
            budget_usd: 0.5,
            ..Ask::new("sonnet", "hi")
        };
        let failed = |line: &str| read(&ask, &[line]).1.unwrap().unwrap_err();
        let budget = failed(
            r#"{"type":"result","subtype":"error_max_budget_usd","is_error":true,"total_cost_usd":0.51}"#,
        );
        assert_eq!(
            budget,
            Failed {
                why: "it reached its budget of $0.50 before it answered".into(),
                reason: Reason::Budget,
                cost_usd: 0.51
            }
        );
        let turns = failed(r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#);
        assert_eq!(turns.reason, Reason::Turns);
        let overloaded = failed(
            r#"{"type":"result","subtype":"success","is_error":true,"result":"API Error: 529 Overloaded"}"#,
        );
        assert_eq!(overloaded.reason, Reason::Passing);
        assert_eq!(overloaded.why, "API Error: 529 Overloaded");
        let other = failed(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Invalid API key"}"#,
        );
        assert_eq!(other.reason, Reason::Other);
    }

    #[test]
    fn a_nonce_is_new_each_time() {
        let (a, b) = (nonce(), nonce());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
