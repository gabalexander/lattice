//! What the wiki's writers are told, and what they answer: one writer a
//! subsection, given the outline and the files it starts from, which it
//! explores from with read-only tools; one a section's summary, the hub
//! its subsections hang from, given its subsections; one the overview,
//! given the sections; the writer of a sync, given its text before and
//! the diff; and the writer told what the checks found, asked once to put
//! it right.
//!
//! The instructions decide the page. Code Wiki's is concrete, names every
//! function, type and file it talks about, links each to its lines, and
//! explains how the parts work together rather than listing them; the
//! writer is shown an example of a subsection in that manner. A writer
//! verifies what it says against code it read, opens with what a reader of
//! its kind of part looks for first, cites where the code says what isn't
//! obvious, and reports in its answer, not in the page, when it can't read
//! the code.
//!
//! Adapted from crystal's wiki generator (`src/wiki/write.rs`, MIT), with
//! deepwiki-by-cc's prompts (`src/lib/server/prompts/shared.ts`, `page.ts`
//! and `update.ts`, MIT; see THIRD_PARTY_NOTICES.md): the research steps,
//! the opening element by kind, the citations and Sources lines, the
//! diagram rules, the hub of a section and the failure field.

use super::diagram::Room;
use super::files::Files;
use super::plan::{Kind, Plan, PlannedSection, PlannedSubsection};
use super::repo;
use crate::index::{DefKind, Index};
use crate::wiki::Diagram;
use serde_json::{Value, json};
use std::path::Path;

/// How a writer finds out what to write.
const RESEARCH: &str = "\
How to find out what to write. You run inside a checkout of the repository at the wiki's commit, \
with Read, Grep and Glob; you can't run anything.
1. Read the files you're given first, keeping their line numbers in mind: your links need them.
2. Trace the behaviour end to end: follow calls and imports to their definitions, Grep for \
callers and for whoever uses what it makes, and read the configuration, constants and defaults \
that change what happens.
3. Go beyond the files you're given wherever the flow leads: they're where to start, not the \
edge.
4. Note what a reader would ask about: errors and what's done with them, fallbacks, limits, \
timeouts, retries, what runs at once, edge cases, each with its value and where it's defined.
5. Say only what you read in the code here. Before you restate a value (a default, a limit, a \
key), read the line that defines it; never infer one from what would be sensible. Where the \
README or docs disagree with the code, write what the code does.
Keep to what your part needs: don't read the whole repository.";

/// How every writer writes.
const WRITING: &str = "\
How to write:
- The reader is an engineer new to this codebase who wants to understand how it works, well \
enough to find their way in the code and change it.
- Be concrete: name the real functions, methods, types, fields, constants, files, flags, config \
keys and commands, each in backticks. A sentence that could be said of any project (\"handles \
errors robustly\") says nothing: say which error, what's done with it, where.
- Explain how things work and work together: what calls what, in what order, what data goes \
where, what's decided where and why, when the code, a comment or a commit says why.
- Prefer prose and links over quoting code: quote a snippet, in a fence with its language, at \
most ten lines, only when the exact code is clearer than any description of it.
- Paragraphs of two to four sentences. A bullet list, each bullet starting with its thing in \
bold or linked, where you go through several things of a kind; a table (two to four columns, \
short cells, symbols named without line numbers) where you map names to places or settings to \
defaults; a numbered list for steps in order. Not everything in one shape.
- When another part of the page covers something, link it by its id from the outline, [Its \
Title](#its-id), and leave it to that part rather than explaining it again; when this part uses \
a function or type another part covers, say so with that link. Never make up an id.
- No title and no headings of levels one to three: the page has them. #### headings only to \
split a long text, in plain words, never a code span alone, never about how you wrote it \
(\"Source of Truth\", \"Code vs Docs\"). No summary or conclusion at the end, no \"In summary\". \
Never mention the outline, these instructions or how you wrote the text: where the outline's \
description of your part is wrong, write what the code does. No words like robust, seamless, \
powerful, comprehensive, leverage, crucial, streamline, facilitate. Plain, exact English.";

/// How a writer links into the code.
const LINKS: &str = "\
Links into the code:
- Link each function, type, constant, field, flag, config key and file where you first name it, \
and again where it matters: [`Name`](code:PATH#L10-L42), PATH from the top of the repository, \
the lines its definition takes, from its first to its last, as Read numbers them. A file is \
[`src/a.rs`](code:src/a.rs), a directory [`src/tui/`](code:src/tui/), one line #L10.
- After a claim a reader can't take for granted (a default, a limit, an edge case, an order), \
cite where the code says it: ([retry.go:52-58](code:internal/queue/retry.go#L52-L58)), the \
label the file's name or path, a colon and its lines.
- End with a Sources line: \"Sources:\" and the citations of the code the text rests on, \
comma-separated, the most important first.
- Never link to lines you haven't read: a wrong link is worse than none.";

/// What every diagram must be.
const DIAGRAMS: &str = "\
Diagrams, in mermaid:
- Your diagram card, `diagram`, is drawn above your text. Choose its kind by what it shows: a \
flowchart (TD or LR) for structure, or for how a call or data flows; a sequenceDiagram when the \
point is an exchange between a few parties over time; a classDiagram or erDiagram when types or \
tables and how they relate are the point; a stateDiagram-v2 for states and what moves between \
them.
- Boxes are real components named by their code, with their file or directory on a second \
line: worker[\"Worker.run<br/>(internal/queue/worker.go)\"]; never ideas like \"Processing\". \
Arrows say in a few words what goes along them: a -->|claims due jobs| b. A dashed arrow (-.->) \
for what happens on failure or seldom. A subgraph may group the boxes of one component.
- 4 to 12 boxes, 15 at most. Its caption: one sentence on what it shows.
- Syntax mermaid draws: node ids of letters, digits and underscores, never a mermaid keyword \
such as end, call, class, click, default, graph, subgraph, style (suffix it: call_[\"call\"]), \
nor a participant, class, state or entity named one (Note, Loop, End, Link, Box, Option, Class: \
`participant Note_ as Note`); every node label in double quotes, a[\"Label\"], with no spaces \
inside the shape's brackets, a{\"Choice\"} not a{ \"Choice\" }; no <placeholder> in a label \
(write {kind}); no %% comments, style, classDef, click or colours; no HTML but <br/>; no \
semicolons or double quotes inside labels or messages; in a sequenceDiagram, `participant A as \
Name`, and arrows ->> with exactly two >.";

/// What a writer may draw in its text besides its card.
fn more_diagrams(room: Room) -> &'static str {
    match room {
        Room::Part => {
            "- One more diagram in your text, only when your part has a second structure or flow \
             worth drawing that the card doesn't show: a ```mermaid fence right after the \
             paragraph that introduces it, a bold title line just before it naming what it shows \
             (**Retry Backoff**). Never the card's diagram again."
        }
        Room::Overview => {
            "- Up to three more diagrams in your text where they help, a typical run as a \
             sequenceDiagram, the data model as a classDiagram: each a ```mermaid fence right \
             after the paragraph that introduces it, a bold title line just before it naming \
             what it shows (**A Build, Start to Finish**). Never the card's diagram again."
        }
    }
}

/// What a writer answers with, its text under `key`.
fn output(key: &str) -> String {
    format!(
        "Answer with the structured output: {key}, your text; diagram, your card, with its \
         mermaid and caption; and failure, null. Only if you genuinely can't read the \
         repository's files with your tools (they fail, or give back nothing): failure is a \
         sentence saying why, and {key} is empty. Never write apologies or notes about your \
         tools into the text."
    )
}

/// An example of a subsection in the manner wanted, from a made-up
/// repository so that nothing of it is copied into a real one.
const EXAMPLE: &str = r#"An example of a good subsection, "Retrying Failed Jobs" of kind pipeline, from another repository, a job queue written in Go. Its body_md:

Retries live in [`internal/queue/retry.go`](code:internal/queue/retry.go): every failed job goes through [`Retrier.Handle`](code:internal/queue/retry.go#L48-L96) before anything else sees it. A worker that gets an error back from a job's [`Run`](code:internal/queue/job.go#L22) doesn't decide what happens next; it hands the job and the error to the retrier, which schedules another attempt or moves the job to the dead letters described in [Dead Letters and Replays](#dead-letters-and-replays).

1. **Classifying:** [`IsRetryable`](code:internal/queue/errors.go#L40-L58) sorts the error into transient or permanent.
2. **Counting:** the job's [`Attempts`](code:internal/queue/job.go#L15) goes up by one.
3. **Backing off:** [`RetryPolicy.Next`](code:internal/queue/policy.go#L35-L52) picks the delay before the next attempt.
4. **Rescheduling:** the job is written back with a later `run_at`, or dead-lettered.

The decision rests on two things the job carries, its attempt count and the [`RetryPolicy`](code:internal/queue/policy.go#L9-L31) its type registered with:

- **Retryable errors.** [`IsRetryable`](code:internal/queue/errors.go#L40-L58) treats timeouts, `ECONNRESET` and HTTP 5xx answers as transient; a validation error, or anything wrapped in [`Permanent`](code:internal/queue/errors.go#L12), goes straight to the dead letters, however many attempts are left.
- **Backoff.** [`RetryPolicy.Next`](code:internal/queue/policy.go#L35-L52) doubles the delay from `InitialDelay` up to `MaxDelay`, then adds up to 20% jitter, so that a burst of failures doesn't come back as a burst ([policy.go:44-49](code:internal/queue/policy.go#L44-L49)).
- **The cap.** Once `Attempts` reaches `MaxAttempts`, 5 unless the job type says otherwise ([policy.go:12](code:internal/queue/policy.go#L12)), the job is dead whatever the error.

Scheduling another attempt doesn't hold a worker. [`Retrier.Handle`](code:internal/queue/retry.go#L48-L96) writes the job back with `run_at` set in the future, in the same transaction that records the error in `job_errors` ([retry.go:81-90](code:internal/queue/retry.go#L81-L90)), so a crash between the two can neither lose the job nor run it twice. The poller only claims jobs whose `run_at` has passed ([`Poller.claim`](code:internal/queue/poller.go#L61-L88)), which is all it takes for the delay to hold, and because the attempt count goes up in that same `UPDATE`, two workers that both time out on one job can't both count it.

The retrier is the one place errors are classified, so the counters in [Queue Metrics](#queue-metrics), `queue_retries_total` and `queue_dead_total`, are incremented there, labelled by job type and by the reason [`IsRetryable`](code:internal/queue/errors.go#L40-L58) gave.

Sources: [retry.go:48-96](code:internal/queue/retry.go#L48-L96), [policy.go:9-52](code:internal/queue/policy.go#L9-L52), [errors.go:12-58](code:internal/queue/errors.go#L12-L58), [poller.go:61-88](code:internal/queue/poller.go#L61-L88)

Its diagram:

flowchart TD
  worker["Worker.run<br/>(internal/queue/worker.go)"] -->|job failed| retrier["Retrier.Handle<br/>(internal/queue/retry.go)"]
  retrier -->|retryable, attempts left| policy["RetryPolicy.Next<br/>(internal/queue/policy.go)"]
  policy -->|run_at = now + backoff| jobs[("jobs table")]
  retrier -.->|permanent, or out of attempts| dead["DeadLetters.Put<br/>(internal/queue/dead.go)"]
  poller["Poller.claim<br/>(internal/queue/poller.go)"] -->|claims jobs whose run_at passed| jobs

Its caption: A failed job goes back to the jobs table with a later run_at, or to the dead letters."#;

/// What the element a subsection of `kind` opens with is.
fn opening(kind: Kind) -> &'static str {
    match kind {
        Kind::Pipeline => {
            "a numbered list of its 4 to 8 steps, each a bold label and one sentence \
             (1. **Claiming:** ...)"
        }
        Kind::DataModel => "a table of its types' fields: Field | Type | Meaning",
        Kind::Configuration => "a table of its settings: Parameter | Default | Effect",
        Kind::Api => "a table of its symbols: Symbol | Where | What it's for",
        Kind::Flow => "nothing more: your card shows the flow, so go on to how it works",
        Kind::Component => "nothing more, when it covers one or two things",
    }
}

/// What a subsection's writer is told.
pub fn subsection_system() -> String {
    format!(
        "You are writing one subsection of a wiki about a software repository, in the manner of \
         Google's Code Wiki: one long page that explains how the code works, section by section, \
         each subsection a diagram card and prose about one part of the code. The message gives \
         you the whole outline (so you can link the other parts and leave them to their \
         writers), your subsection's title, kind and what it's to explain, the files to start \
         from with what they define and on which lines, and the commits that touched them \
         lately.\n\n{RESEARCH}\n\n\
         What to write, body_md: 600 to 1,200 words.\n\
         - Open with one to three sentences: what this part is for, where it lives, and the \
         main code it covers.\n\
         - Right after, the element its kind calls for, so that a reader tells at a glance what \
         kind of part it is (the message says which).\n\
         - Then how it works, in the order a reader needs it.\n\
         - End with the Sources line.\n\n{WRITING}\n\n{LINKS}\n\n{DIAGRAMS}\n{}\n\n{}\n\n{EXAMPLE}",
        more_diagrams(Room::Part),
        output("body_md"),
    )
}

/// What a section's summary writer is told: the hub its subsections hang
/// from.
pub fn section_system() -> String {
    format!(
        "You are writing the introduction of one top section of a wiki about a software \
         repository, in the manner of Google's Code Wiki: the hub its subsections hang from. The \
         message gives you the outline, and the section's subsections as their writers wrote \
         them. You have Read, Grep and Glob over the repository, to check what you say.\n\n\
         What to write, summary_md: 200 to 450 words, besides its tables.\n\
         - Open with one or two sentences on what this part of the system does as a whole.\n\
         - Then map its parts: a table Component | Location | Purpose, each component its main \
         type or function linked into the code, each location its file or directory.\n\
         - Then a table Subsection | What it covers: each subsection linked by its id, [Its \
         Title](#its-id), with a line on what a reader finds there.\n\
         - Then a paragraph or two on how the parts work together: how a typical request, run \
         or event goes through them, linking the subsections where their subjects come up. \
         Introduce and connect; leave the depth to the subsections, and don't say again what \
         they say.\n\
         - End with the Sources line.\n\
         The card shows how the section's parts fit together: a box for each main component, \
         often one a subsection, and arrows for the calls, data or control between them.\n\n\
         {WRITING}\n\n{LINKS}\n\n{DIAGRAMS}\n{}\n\n{}",
        more_diagrams(Room::Part),
        output("summary_md"),
    )
}

/// What the overview's writer is told.
pub fn overview_system() -> String {
    format!(
        "You are writing the overview at the top of a wiki about a software repository, in the \
         manner of Google's Code Wiki. The message gives you the outline, every section's \
         summary, and the files a reader of the code starts from. You have Read, Grep and Glob \
         over the repository, to check what you say.\n\n\
         What to write, summary_md: 400 to 800 words.\n\
         - What the repository is and what it's for, in two or three sentences.\n\
         - Its architecture: the main parts, and how a typical request, run or session goes \
         through them, from where it comes in to where it ends.\n\
         - How it's built, configured and run, briefly.\n\
         - Link every section by its id where its subject comes up, so that the overview sends \
         the reader to each: \"These are detailed in [Its Title](#its-id).\" Link the main entry \
         points into the code.\n\
         - End with the Sources line.\n\
         The card is the architecture: the main components, 6 to 15 boxes, each labelled with \
         its name and its directory or file, and how they connect.\n\n\
         {WRITING}\n\n{LINKS}\n\n{DIAGRAMS}\n{}\n\n{}",
        more_diagrams(Room::Overview),
        output("summary_md"),
    )
}

/// What a sync adds to what a writer is told: the text before, the diff,
/// and the choice to leave the text as it is.
pub const UPDATE: &str = "\
This is an update, after the code changed. The message gives the text as it was, its diagram, \
the commits since, and the diff of its files. The diff shows what changed; the checkout, at the \
new commit, is the truth.
1. Read the diff, then the changed files as they are now.
2. Check the text's claims against the code now: defaults, limits, behaviour and flow may have \
moved even where the diff doesn't contradict the text.
3. Follow the change outward where it matters: a changed function's callers, or the \
configuration it reads, may make other sentences wrong.
If nothing in the text needs to change, set unchanged to true and leave the text empty. \
Otherwise set unchanged to false and write the whole text again: keep what still holds, word for \
word where you can, keep its shape and its citations, change what the code changed, and read \
again the lines of every link into code the diff touched, since they may have moved. Set \
meaning_changed to true only when what this part does, its components or how they connect \
changed, so that what sums it up elsewhere must change too; not for wording, a private helper \
renamed, or lines moved.";

/// What a writer asked to put its text right is told, besides what it
/// was told before.
pub fn fix_system(first: &str) -> String {
    format!(
        "{first}\n\nThis time you are given a text you wrote, and the problems the checks found \
         in it. Put right each problem and change nothing else: a link at the wrong lines goes \
         where the thing is defined (find it with Grep and Read), a path the repository doesn't \
         have is replaced by the right one or left unlinked, an id the page doesn't have by one \
         from the outline or no link, a diagram that doesn't read is written again in the subset \
         above. Answer with the whole text and the diagram, as before."
    )
}

/// The shape of a writer's answer: its text, `body_md` or with `summary`,
/// `summary_md`, its diagram and its failure; with `update`, whether it's
/// unchanged and whether its meaning changed.
pub fn schema(summary: bool, update: bool) -> Value {
    let text_key = if summary { "summary_md" } else { "body_md" };
    let mut properties = serde_json::Map::new();
    properties.insert(text_key.into(), json!({"type": "string"}));
    properties.insert(
        "diagram".into(),
        json!({
            "type": "object",
            "properties": {
                "mermaid": {"type": "string"},
                "caption": {"type": "string"},
            },
            "required": ["mermaid", "caption"],
        }),
    );
    properties.insert("failure".into(), json!({"type": ["string", "null"]}));
    let mut required = vec![json!(text_key), json!("diagram"), json!("failure")];
    if update {
        properties.insert("unchanged".into(), json!({"type": "boolean"}));
        properties.insert("meaning_changed".into(), json!({"type": "boolean"}));
        required.push(json!("unchanged"));
        required.push(json!("meaning_changed"));
    }
    json!({"type": "object", "properties": properties, "required": required})
}

/// What a writer answered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Written {
    pub text: String,
    pub diagram: Option<Diagram>,
    /// For a sync, whether what it says changed in meaning.
    pub meaning_changed: bool,
    /// For a sync, whether it left the text as it was.
    pub unchanged: bool,
    /// Why it couldn't do the work, when it said it couldn't.
    pub failure: Option<String>,
}

/// The answer read: its text, at `body_md` or `summary_md`, its diagram
/// when it has one, whether it's unchanged and its meaning changed, and its
/// failure.
pub fn read(answer: &Value) -> Written {
    let text = answer["body_md"]
        .as_str()
        .or_else(|| answer["summary_md"].as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let diagram = answer["diagram"]["mermaid"]
        .as_str()
        .filter(|mermaid| !mermaid.trim().is_empty())
        .map(|mermaid| Diagram {
            mermaid: mermaid.trim().to_string(),
            caption: answer["diagram"]["caption"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string(),
        });
    Written {
        text,
        diagram,
        meaning_changed: answer["meaning_changed"].as_bool().unwrap_or(true),
        unchanged: answer["unchanged"].as_bool().unwrap_or(false),
        failure: answer["failure"]
            .as_str()
            .map(str::trim)
            .filter(|why| !why.is_empty())
            .map(str::to_string),
    }
}

/// A writer's text and diagram given back to it with what's wrong.
pub fn fix_message(task: &str, written: &Written, problems: &[String], summary: bool) -> String {
    let key = if summary { "summary_md" } else { "body_md" };
    let diagram = written.diagram.as_ref().map_or_else(
        || "(none)".to_string(),
        |diagram| format!("{}\n\nIts caption: {}", diagram.mermaid, diagram.caption),
    );
    format!(
        "{task}\n\nThe problems found:\n{}\n\nYour {key}:\n\n{}\n\nYour diagram:\n\n{diagram}",
        problems
            .iter()
            .map(|problem| format!("- {problem}"))
            .collect::<Vec<_>>()
            .join("\n"),
        written.text,
    )
}

/// What a subsection's writer is told it's writing.
pub fn subsection_task(name: &str, section: &PlannedSection, sub: &PlannedSubsection) -> String {
    format!(
        "Write the subsection \"{}\" (#{}) of the section \"{}\" (#{}) in the wiki of {name}.",
        sub.title, sub.id, section.title, section.id
    )
}

/// A subsection writer's message, its seeds and what an update adds after
/// it.
pub fn subsection_message(task: &str, plan: &Plan, sub: &PlannedSubsection) -> String {
    format!(
        "{task}\n\nIts kind: {}, so right after its opening sentences comes {}.\n\nWhat it's to \
         explain: {}\n\nThe outline of the whole page:\n\n{}",
        sub.kind.word(),
        opening(sub.kind),
        sub.about,
        plan.outline()
    )
}

/// What a sync's writer is given besides: its text and diagram before, and
/// what changed in its files between the commits.
pub fn update_part(before: &str, diagram: Option<&Diagram>, diff: &str, commits: &str) -> String {
    let diagram = diagram.map_or("(none)", |diagram| diagram.mermaid.as_str());
    format!(
        "\n\nThe text before:\n\n{before}\n\nThe diagram before:\n\n{diagram}\n\nThe commits \
         since:\n{commits}\n\nWhat changed in its files:\n\n```diff\n{diff}```\n"
    )
}

/// How many of the commits that touched its files a writer is shown.
const COMMITS_SHOWN: usize = 15;

/// The most definitions a writer is shown the lines of.
const DEFS_SHOWN: usize = 600;

/// What a writer is shown of the files it starts from: which they are with
/// their lines, the commits that touched them lately, and what they define
/// on which lines. It reads them itself.
pub fn seeds(tree: &Path, files: &Files, index: &Index, entries: &[String]) -> String {
    let covered = files.covered(entries);
    let lines: u64 = covered.iter().map(|file| u64::from(file.lines)).sum();
    let mut out = format!(
        "\n\nThe files to start from ({} files, {lines} lines), not the edge of what you may \
         read:\n{}\n",
        covered.len(),
        covered
            .iter()
            .map(|file| format!("- {} ({} lines)", file.path, file.lines))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let log = repo::log(tree, entries, COMMITS_SHOWN);
    if !log.trim().is_empty() {
        out.push_str(&format!(
            "\nThe commits that touched them lately, newest first:\n{log}"
        ));
    }
    let defs: Vec<_> = index
        .outline(entries)
        .into_iter()
        .filter(|def| !matches!(def.kind, DefKind::Field | DefKind::Variant))
        .collect();
    if !defs.is_empty() {
        out.push_str("\nWhat they define, with its lines:\n");
        let mut path = "";
        for def in defs.iter().take(DEFS_SHOWN) {
            if def.path != path {
                path = &def.path;
                out.push_str(&format!("\n{path}:"));
            }
            let lines = if def.end > def.start {
                format!("{}-{}", def.start, def.end)
            } else {
                def.start.to_string()
            };
            out.push_str(&format!(" {} {} L{lines};", def.kind, def.name));
        }
        out.push('\n');
        if defs.len() > DEFS_SHOWN {
            out.push_str(&format!("(and {} more)\n", defs.len() - DEFS_SHOWN));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::diagram;

    #[test]
    fn the_example_s_diagram_reads_as_every_writer_s_must() {
        let mermaid: String = EXAMPLE
            .split("Its diagram:\n\n")
            .nth(1)
            .and_then(|rest| rest.split("\n\nIts caption").next())
            .unwrap()
            .to_string();
        let checked = diagram::check(&Diagram {
            mermaid,
            caption: String::new(),
        })
        .unwrap()
        .mermaid;
        assert!(
            checked.contains("retrier[\"Retrier.Handle<br/>(internal/queue/retry.go)\"]"),
            "{checked}"
        );
    }

    #[test]
    fn an_answer_is_read_with_its_diagram_or_without() {
        let answer = json!({"body_md": " Text. ", "diagram": {"mermaid": "flowchart TD\n a --> b", "caption": "C."}, "failure": null});
        let written = read(&answer);
        assert_eq!(written.text, "Text.");
        assert_eq!(written.diagram.unwrap().caption, "C.");
        assert!(written.meaning_changed && !written.unchanged);
        assert_eq!(written.failure, None);
        let none = json!({"summary_md": "S.", "diagram": {"mermaid": " ", "caption": ""}, "meaning_changed": false, "unchanged": true, "failure": " "});
        let written = read(&none);
        assert_eq!(
            (
                written.diagram,
                written.meaning_changed,
                written.unchanged,
                written.failure
            ),
            (None, false, true, None)
        );
        let failed = read(&json!({"body_md": "", "failure": "Read failed"}));
        assert_eq!(failed.failure.as_deref(), Some("Read failed"));
    }

    #[test]
    fn the_schema_asks_for_the_text_the_diagram_the_failure_and_for_a_sync_more() {
        assert_eq!(
            schema(true, true)["required"],
            json!([
                "summary_md",
                "diagram",
                "failure",
                "unchanged",
                "meaning_changed"
            ])
        );
        assert_eq!(
            schema(false, false)["required"],
            json!(["body_md", "diagram", "failure"])
        );
        assert_eq!(
            schema(false, false)["properties"]["failure"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn each_writer_is_told_its_part_and_how_it_opens() {
        let subsection = subsection_system();
        assert!(subsection.contains("Dead Letters and Replays"));
        assert!(subsection.contains("One more diagram in your text"));
        assert!(subsection.contains("failure is a"));
        assert!(section_system().contains("Component | Location | Purpose"));
        assert!(overview_system().contains("Up to three more diagrams"));
        let plan = Plan {
            overview: String::new(),
            sections: Vec::new(),
        };
        let sub = PlannedSubsection {
            id: "retries".into(),
            title: "Retries".into(),
            about: "How a failed job comes back.".into(),
            kind: Kind::Configuration,
            files: vec!["retry.go".into()],
        };
        let message = subsection_message("Write it.", &plan, &sub);
        assert!(message.contains("Its kind: configuration, so right after its opening sentences comes a table of its settings: Parameter | Default | Effect."), "{message}");
    }
}
