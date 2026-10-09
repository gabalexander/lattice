//! The outline of a wiki: its sections, each with its subsections, each
//! subsection with the files and directories its writer starts from (its
//! seed files) and the kind of part it is, which decides how its text
//! opens. One Claude reads the repository and plans it, from the file tree
//! and the README; what it answers is read here and put in order: paths
//! the repository doesn't have taken out, each subsection's paths deduped
//! and capped, sections and subsections past the most a repository of its
//! size has trimmed, and every source file covered by some subsection. An
//! outline that's badly off, a whole part of the code left out, is sent
//! back once with what's wrong; source files still left out then go to the
//! subsection nearest them by path.
//!
//! Adapted from crystal's wiki generator (`src/wiki/plan.rs`, MIT), with
//! deepwiki-by-cc's outline prompt and outline normalizer
//! (`src/lib/server/prompts/outline.ts` and
//! `src/lib/server/ai/outline-normalizer.ts`, MIT; see
//! THIRD_PARTY_NOTICES.md).

use super::files::{self, Files};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// The outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// What the overview is to say: what the repository is, and its
    /// architecture.
    pub overview: String,
    pub sections: Vec<PlannedSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedSection {
    pub id: String,
    pub title: String,
    /// What it covers, for whoever writes its summary.
    pub about: String,
    pub subsections: Vec<PlannedSubsection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedSubsection {
    pub id: String,
    pub title: String,
    /// What its writer is to explain, and what it leaves to its siblings.
    pub about: String,
    /// What kind of part it is, which decides how its text opens.
    #[serde(default)]
    pub kind: Kind,
    /// The files and directories it covers, a directory ending with `/`.
    pub files: Vec<String>,
}

/// What kind of part a subsection is: what a reader looks for first in it,
/// and so the element its text opens with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A lifecycle or a pipeline: a numbered sequence of its steps.
    Pipeline,
    /// Types or tables and their fields: a table of them.
    DataModel,
    /// Settings: a table of each, its default and what it does.
    Configuration,
    /// An interface others call: a table of its symbols and where they are.
    Api,
    /// A flow between parts: its diagram.
    Flow,
    /// One or two components: the opening paragraph alone.
    #[default]
    Component,
}

impl Kind {
    /// Every kind, by the word the planner gives it.
    const ALL: [(Kind, &'static str); 6] = [
        (Kind::Pipeline, "pipeline"),
        (Kind::DataModel, "data-model"),
        (Kind::Configuration, "configuration"),
        (Kind::Api, "api"),
        (Kind::Flow, "flow"),
        (Kind::Component, "component"),
    ];

    pub fn word(self) -> &'static str {
        Kind::ALL
            .iter()
            .find(|(kind, _)| *kind == self)
            .map_or("component", |(_, word)| word)
    }

    fn of(word: &str) -> Kind {
        Kind::ALL
            .iter()
            .find(|(_, w)| w.eq_ignore_ascii_case(word.trim()))
            .map_or(Kind::Component, |(kind, _)| *kind)
    }
}

/// The id the overview has on the page, which no section may take.
pub const OVERVIEW_ID: &str = "overview";

/// The most paths a subsection's writer starts from.
const MOST_PATHS: usize = 25;

/// How a repository of a given size is planned: how many sections, how
/// many subsections in each, and about how many in all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub sections: (usize, usize),
    pub subsections: (usize, usize),
    pub total: usize,
}

/// How a repository of `lines` lines of source is planned: a small one
/// can't fill the large one's page, and a subsection is about 3,500 lines
/// of it, within what the sections can hold.
pub fn bounds(lines: u64) -> Bounds {
    let (sections, subsections) = if lines < 2_000 {
        ((1, 4), (1, 4))
    } else if lines < 20_000 {
        ((3, 10), (2, 7))
    } else {
        ((6, 16), (3, 9))
    };
    let wanted = (lines / 3_500) as usize;
    let total = wanted.clamp(sections.0 * subsections.0, sections.1 * subsections.1);
    Bounds {
        sections,
        subsections,
        total: total.max(1),
    }
}

impl Plan {
    /// Every subsection, with its section, in order.
    pub fn subsections(&self) -> impl Iterator<Item = (&PlannedSection, &PlannedSubsection)> {
        self.sections
            .iter()
            .flat_map(|section| section.subsections.iter().map(move |sub| (section, sub)))
    }

    /// The subsection with the id `id`.
    pub fn subsection(&self, id: &str) -> Option<&PlannedSubsection> {
        self.subsections()
            .map(|(_, sub)| sub)
            .find(|sub| sub.id == id)
    }

    /// Every anchor on the page.
    pub fn anchors(&self) -> BTreeSet<String> {
        let mut anchors: BTreeSet<String> = [OVERVIEW_ID.to_string()].into();
        for section in &self.sections {
            anchors.insert(section.id.clone());
            anchors.extend(section.subsections.iter().map(|sub| sub.id.clone()));
        }
        anchors
    }

    /// The source files no subsection covers.
    pub fn uncovered<'a>(&self, files: &'a Files) -> Vec<&'a files::File> {
        let entries: Vec<String> = self
            .subsections()
            .flat_map(|(_, sub)| sub.files.iter().cloned())
            .collect();
        files
            .sources()
            .filter(|file| !files::covers(&entries, &file.path))
            .collect()
    }

    /// The outline as each writer is shown it: every section and
    /// subsection by its id, title and what it covers.
    pub fn outline(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            out.push_str(&format!(
                "- {} (#{}): {}\n",
                section.title, section.id, section.about
            ));
            for sub in &section.subsections {
                out.push_str(&format!(
                    "  - {} (#{}): {} Files: {}\n",
                    sub.title,
                    sub.id,
                    sub.about,
                    sub.files.join(", ")
                ));
            }
        }
        out
    }

    /// Gives each source file no subsection covers to the subsection
    /// nearest it: the one with a path sharing the most directories with
    /// it. Returns how many it gave.
    pub fn cover(&mut self, files: &Files) -> usize {
        let uncovered: Vec<String> = self
            .uncovered(files)
            .into_iter()
            .map(|file| file.path.clone())
            .collect();
        for path in &uncovered {
            let mut best: Option<(usize, usize, usize)> = None;
            for (s, section) in self.sections.iter().enumerate() {
                for (u, sub) in section.subsections.iter().enumerate() {
                    let shared = sub
                        .files
                        .iter()
                        .map(|entry| shared_dirs(entry, path))
                        .max()
                        .unwrap_or(0);
                    if best.is_none_or(|(most, _, _)| shared > most) {
                        best = Some((shared, s, u));
                    }
                }
            }
            if let Some((_, s, u)) = best {
                self.sections[s].subsections[u].files.push(path.clone());
            }
        }
        uncovered.len()
    }
}

/// How many directories `entry` and `path` share from the top.
fn shared_dirs(entry: &str, path: &str) -> usize {
    let dirs = |p: &str| -> Vec<String> {
        let mut parts: Vec<String> = p.split('/').map(str::to_string).collect();
        parts.pop();
        parts
    };
    dirs(entry)
        .iter()
        .zip(dirs(path).iter())
        .take_while(|(a, b)| a == b)
        .count()
}

/// The shape of the planner's answer, for `--json-schema`.
pub fn schema() -> Value {
    let text = json!({"type": "string"});
    let kinds: Vec<&str> = Kind::ALL.iter().map(|(_, word)| *word).collect();
    json!({
        "type": "object",
        "properties": {
            "overview": text,
            "sections": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": text,
                        "about": text,
                        "subsections": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": text,
                                    "about": text,
                                    "kind": {"type": "string", "enum": kinds},
                                    "files": {"type": "array", "items": text},
                                },
                                "required": ["title", "about", "kind", "files"],
                            },
                        },
                    },
                    "required": ["title", "about", "subsections"],
                },
            },
        },
        "required": ["overview", "sections"],
    })
}

/// What the planner is told it's doing.
pub const SYSTEM: &str = "You are planning a wiki for a software repository, in the manner of \
Google's Code Wiki: one long page that explains how the code works to an engineer who is new to \
it, section by section, each subsection a diagram and prose that names the real functions, types \
and files. You plan; other writers write each subsection from your outline, one each, starting \
from the files you give it and exploring from there, so the outline decides what the page covers \
and how a reader meets it.\n\n\
First explore: the message gives the README and every file with its lines. Read any other docs \
or agent notes (AGENTS.md, CLAUDE.md, docs/), find the entry points (the main function, the \
command line, the server, the public API), and read enough of the main modules to see what the \
system does and how its parts work together. Use Glob and Grep to find things and Read to read \
them; you can't run commands. Treat the README and docs as what the code is meant to do, which \
may be out of date; where they and the code disagree, plan by what the code does.\n\n\
Then answer with the outline:\n\
- overview: two to four sentences on what the repository is and the big parts of its \
architecture, for whoever writes the overview.\n\
- sections: each a part of what the system does (a capability, a subsystem, a flow), named for \
what it does rather than for a directory or a language: \"Sessions and the Daemon\", \"Drawing \
Diagrams as Text\", not \"src/tui\". Order them as a reader should meet them, by how a run goes \
from the outside in: the entry points people use first, then the core, then what supports it, \
then configuration, then building, testing and releasing. Each has an about: one or two \
sentences on what it covers.\n\
- subsections: each a specific component, mechanism or flow, with a title that says what it is \
(\"Handing the Daemon Over to a New Binary\"); an about; a kind; and files.\n\
- A subsection's about tells its writer what to explain, specifically: what it does, how, and \
how it connects to the rest, and the behaviour a reader would ask about (what happens on \
failure, on a duplicate, at a limit; the defaults; what runs at once). Where siblings share \
files or ideas, say what belongs here and what to leave to which sibling: \"trace the request \
end to end, but leave the worker's internals to Job Workers\".\n\
- A subsection's kind is what a reader looks for first in it: pipeline (a lifecycle or a \
sequence of stages), data-model (types or tables and their fields), configuration (settings and \
their defaults), api (an interface others call), flow (an interaction between parts), or \
component (one or two components).\n\
- A subsection's files are where its writer starts: the files and directories it covers, paths \
from the top of the repository, a directory ending with /, every file that takes part in what \
it explains (the entry point, the core, the configuration it reads, the tests that pin it \
down), at most 25. Aim for roughly 2,000 to 5,000 lines of code a subsection: split a big \
module by what its parts do, and join small related files into one.\n\
- Every file the message marks [src] must be among some subsection's files. Tests go with what \
they test, or in a section on how the project is tested. Docs, config and assets may be listed \
where they help, and needn't be covered.\n\
- Make the page as big as the code is: don't invent depth or structure the code doesn't have.\n\
- Titles in Title Case, specific, without numbers or file names. No section called Overview, \
Introduction, Miscellaneous or Other: the page has its own overview, and everything has a \
place.";

/// How much of the README the planner is given.
const README_SHOWN: usize = 16 * 1024;

/// The planner's message: the repository's README and files, with which
/// are source and their lines, and how big to plan it.
pub fn message(name: &str, commit: &str, files: &Files, readme: Option<&str>) -> String {
    let lines = files.source_lines();
    let bounds = bounds(lines);
    let readme = match readme {
        Some(text) => {
            let mut cut = text.len().min(README_SHOWN);
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            let more = if cut < text.len() {
                "\n[the rest is cut]"
            } else {
                ""
            };
            format!(
                "Its README, what it's meant to do:\n\n{}{more}\n\n",
                &text[..cut]
            )
        }
        None => String::new(),
    };
    format!(
        "Plan the wiki of {name}, at commit {commit}.\n\n\
         Plan {} to {} sections, each with {} to {} subsections, about {} subsections in all. The \
         repository has {} files, {} of them source, {lines} lines of source in all.\n\n\
         {readme}Its files, from the top, each with its lines; [src] marks the source files every \
         wiki must cover:\n\n{}",
        bounds.sections.0,
        bounds.sections.1,
        bounds.subsections.0,
        bounds.subsections.1,
        bounds.total,
        files.files.len(),
        files.sources().count(),
        listing(files),
    )
}

/// The most files listed one a line; past it, directories are.
const MOST_LISTED: usize = 1500;

/// The files, one a line, or for a big repository, each directory with
/// how many files and lines it has and the first of its files' names.
pub fn listing(files: &Files) -> String {
    let line = |file: &files::File| {
        let src = if files::is_source(file) { " [src]" } else { "" };
        match (file.text, file.generated) {
            (true, false) => format!("{} ({} lines){src}\n", file.path, file.lines),
            (true, true) => format!("{} ({} lines, generated)\n", file.path, file.lines),
            (false, _) => format!("{} (binary or large)\n", file.path),
        }
    };
    if files.files.len() <= MOST_LISTED {
        return files.files.values().map(line).collect();
    }
    let mut dirs: std::collections::BTreeMap<&str, Vec<&files::File>> = Default::default();
    for file in files.files.values() {
        let dir = file.path.rsplit_once('/').map_or("", |(dir, _)| dir);
        dirs.entry(dir).or_default().push(file);
    }
    let mut out = String::new();
    for (dir, in_dir) in dirs {
        let lines: u64 = in_dir.iter().map(|file| u64::from(file.lines)).sum();
        let sources = in_dir.iter().filter(|file| files::is_source(file)).count();
        let names: Vec<&str> = in_dir
            .iter()
            .take(8)
            .map(|file| file.path.rsplit('/').next().unwrap_or(&file.path))
            .collect();
        let more = if in_dir.len() > names.len() {
            ", …"
        } else {
            ""
        };
        let dir = if dir.is_empty() { "." } else { dir };
        out.push_str(&format!(
            "{dir}/: {} files, {sources} [src], {lines} lines: {}{more}\n",
            in_dir.len(),
            names.join(", ")
        ));
    }
    out
}

/// The message asking the planner to put right what's wrong with `plan`.
pub fn fix_message(plan: &Value, problems: &[String], files: &Files) -> String {
    format!(
        "Your outline below has problems. Answer with the whole outline again, put right.\n\n\
         Problems:\n{}\n\nYour outline:\n{}\n\nThe repository's files:\n\n{}",
        problems
            .iter()
            .map(|problem| format!("- {problem}"))
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::to_string_pretty(plan).unwrap_or_default(),
        listing(files),
    )
}

/// The planner's answer read.
#[derive(Debug, Clone)]
pub struct Read {
    pub plan: Plan,
    /// What's wrong with it that the planner should put right.
    pub problems: Vec<String>,
    /// What was put right here, for the log.
    pub notes: Vec<String>,
}

/// The share of the source's lines left out of the outline past which
/// it's sent back.
const LEFT_OUT: f64 = 0.15;

/// The planner's answer read into a plan, its ids made, put in order as
/// the module says, with what's wrong with it. It fails only for an
/// answer that isn't an outline at all.
pub fn read(answer: &Value, files: &Files) -> Result<Read> {
    let mut problems = Vec::new();
    let mut notes = Vec::new();
    let text = |value: &Value| value.as_str().unwrap_or_default().trim().to_string();
    let Some(given) = answer["sections"].as_array().filter(|s| !s.is_empty()) else {
        bail!("its outline has no sections");
    };
    let bounds = bounds(files.source_lines());
    let mut ids = Ids::default();
    let mut sections = Vec::new();
    let mut unknown = Vec::new();
    for section in given {
        let title = text(&section["title"]);
        if title.is_empty() {
            notes.push("a section with no title was left out".into());
            continue;
        }
        let id = ids.take(&title, None);
        let mut subsections = Vec::new();
        for sub in section["subsections"].as_array().into_iter().flatten() {
            let sub_title = text(&sub["title"]);
            if sub_title.is_empty() {
                notes.push(format!(
                    "a subsection of {title:?} with no title was left out"
                ));
                continue;
            }
            let mut entries: Vec<String> = Vec::new();
            for path in sub["files"].as_array().into_iter().flatten() {
                let path = text(path);
                match files.entry(&path) {
                    Some(entry) if !entries.contains(&entry) => entries.push(entry),
                    Some(_) => {}
                    None => unknown.push(path),
                }
            }
            if entries.len() > MOST_PATHS {
                notes.push(format!(
                    "the subsection {sub_title:?} named {} paths: its first {MOST_PATHS} kept",
                    entries.len()
                ));
                entries.truncate(MOST_PATHS);
            }
            if entries.is_empty() {
                notes.push(format!(
                    "the subsection {sub_title:?} covered no file the repository has, and was left out"
                ));
                continue;
            }
            subsections.push(PlannedSubsection {
                id: ids.take(&sub_title, Some(&id)),
                title: sub_title,
                about: text(&sub["about"]),
                kind: Kind::of(sub["kind"].as_str().unwrap_or_default()),
                files: entries,
            });
        }
        if subsections.len() > bounds.subsections.1 {
            notes.push(format!(
                "the section {title:?} had {} subsections: its first {} kept",
                subsections.len(),
                bounds.subsections.1
            ));
            subsections.truncate(bounds.subsections.1);
        }
        if subsections.is_empty() {
            continue;
        }
        sections.push(PlannedSection {
            id,
            title,
            about: text(&section["about"]),
            subsections,
        });
    }
    if sections.len() > bounds.sections.1 {
        notes.push(format!(
            "it had {} sections: the first {} kept",
            sections.len(),
            bounds.sections.1
        ));
        sections.truncate(bounds.sections.1);
    }
    if sections.is_empty() {
        bail!("its outline has no subsection covering a file the repository has");
    }
    if !unknown.is_empty() {
        notes.push(format!(
            "paths the repository doesn't have were left out: {}",
            unknown.join(", ")
        ));
    }
    let plan = Plan {
        overview: text(&answer["overview"]),
        sections,
    };
    let uncovered = plan.uncovered(files);
    let left_out: u64 = uncovered.iter().map(|file| u64::from(file.lines)).sum();
    let all = files.source_lines().max(1);
    if !uncovered.is_empty() && left_out as f64 / all as f64 > LEFT_OUT {
        let shown: Vec<&str> = uncovered
            .iter()
            .take(60)
            .map(|file| file.path.as_str())
            .collect();
        let more = uncovered.len() - shown.len();
        problems.push(format!(
            "no subsection covers these source files, {left_out} of the {all} lines of source: {}{}",
            shown.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ));
    }
    let planned = plan.subsections().count();
    if planned * 2 < bounds.total {
        problems.push(format!(
            "it has {planned} subsections, where a repository this size has about {}: split the \
             big ones by what their parts do",
            bounds.total
        ));
    }
    Ok(Read {
        plan,
        problems,
        notes,
    })
}

/// Where a reader of the code starts: the files that run first, by the
/// names projects give them, for the overview's writer to start from.
pub fn entrypoints(files: &Files) -> Vec<String> {
    const NAMED: &[&str] = &[
        "src/main.rs",
        "src/lib.rs",
        "main.go",
        "src/index.ts",
        "src/main.ts",
        "src/index.js",
        "src/main.py",
        "main.py",
        "app.py",
        "src/app.ts",
        "src/server.ts",
        "index.ts",
        "index.js",
        "server.py",
        "package.json",
        "Cargo.toml",
        "go.mod",
        "pyproject.toml",
    ];
    let mut found: Vec<String> = NAMED
        .iter()
        .filter(|path| files.get(path).is_some())
        .map(|path| path.to_string())
        .collect();
    for file in files.files.values() {
        let parts: Vec<&str> = file.path.split('/').collect();
        if let ["cmd", _, "main.go"] = parts.as_slice()
            && !found.contains(&file.path)
        {
            found.push(file.path.clone());
        }
    }
    found.truncate(6);
    found
}

/// The ids given so far, so that each is given once.
#[derive(Default)]
struct Ids {
    taken: BTreeSet<String>,
}

impl Ids {
    /// An id for `title`, its slug, or under `parent`'s when that's taken,
    /// or numbered.
    fn take(&mut self, title: &str, parent: Option<&str>) -> String {
        let slug = slug(title);
        let slug = if slug.is_empty() {
            "part".to_string()
        } else {
            slug
        };
        let mut tries = vec![slug.clone()];
        if let Some(parent) = parent {
            tries.push(format!("{parent}-{slug}"));
        }
        let id = tries
            .into_iter()
            .find(|id| id != OVERVIEW_ID && !self.taken.contains(id))
            .unwrap_or_else(|| {
                (2..)
                    .map(|n| format!("{slug}-{n}"))
                    .find(|id| !self.taken.contains(id))
                    .expect("a number nobody has")
            });
        self.taken.insert(id.clone());
        id
    }
}

/// `title` as an anchor: lower case, words joined by `-`.
pub fn slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let mut cut: String = slug.chars().take(64).collect();
    while cut.ends_with('-') {
        cut.pop();
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::files::{File, tests::file};

    fn files(paths: &[&str]) -> Files {
        let mut files = Files::default();
        for path in paths {
            files.add(File {
                lines: 100,
                ..file(path)
            });
        }
        files
    }

    #[test]
    fn titles_make_ids_once_each() {
        assert_eq!(
            slug("Handing the Daemon Over (to a New Binary)"),
            "handing-the-daemon-over-to-a-new-binary"
        );
        assert_eq!(slug("  C++ & Rust!  "), "c-rust");
        let mut ids = Ids::default();
        assert_eq!(ids.take("Sessions", None), "sessions");
        assert_eq!(ids.take("Sessions", Some("core")), "core-sessions");
        assert_eq!(ids.take("Sessions", Some("core")), "sessions-2");
        assert_eq!(ids.take("Overview", None), "overview-2");
        assert_eq!(ids.take("???", None), "part");
    }

    #[test]
    fn an_outline_is_put_in_order_as_it_is_read() {
        let files = files(&[
            "src/main.rs",
            "src/tui/app.rs",
            "src/tui/ui.rs",
            "src/db.rs",
            "README.md",
        ]);
        let answer = json!({
            "overview": "An app.",
            "sections": [
                {"title": "The Interface", "about": "What you see.", "subsections": [
                    {"title": "Drawing", "about": "How it draws.", "kind": "flow", "files": ["src/tui", "src/gone.rs", "./src/tui/"]},
                    {"title": "Starting", "about": "Main.", "kind": "nonsense", "files": ["./src/main.rs", "README.md", "src/db.rs"]}
                ]},
                {"title": "Nothing", "about": "", "subsections": [
                    {"title": "Ghost", "about": "", "kind": "api", "files": ["nowhere/"]}
                ]}
            ]
        });
        let read = read(&answer, &files).unwrap();
        let plan = &read.plan;
        assert_eq!(plan.sections.len(), 1);
        let section = &plan.sections[0];
        assert_eq!(section.id, "the-interface");
        assert_eq!(section.subsections[0].files, ["src/tui/"]);
        assert_eq!(section.subsections[0].kind, Kind::Flow);
        assert_eq!(
            section.subsections[1].files,
            ["src/main.rs", "README.md", "src/db.rs"]
        );
        assert_eq!(section.subsections[1].kind, Kind::Component);
        assert!(read.problems.is_empty(), "{:?}", read.problems);
        let notes = read.notes.join(" | ");
        assert!(notes.contains("src/gone.rs, nowhere/"), "{notes}");
        assert!(notes.contains("\"Ghost\" covered no file"), "{notes}");
        assert_eq!(plan.anchors().len(), 4);
        assert!(
            plan.outline()
                .contains("  - Drawing (#drawing): How it draws. Files: src/tui/\n")
        );
        assert!(super::read(&json!({"sections": []}), &files).is_err());
    }

    #[test]
    fn an_outline_that_leaves_out_much_of_the_code_or_is_too_small_is_sent_back() {
        let mut big = Files::default();
        for n in 0..40 {
            big.add(File {
                lines: 1_000,
                ..file(&format!("src/m{n}.rs"))
            });
        }
        let answer = json!({"overview": "", "sections": [{"title": "Core", "about": "", "subsections": [
            {"title": "One", "about": "", "kind": "api", "files": ["src/m0.rs"]}
        ]}]});
        let read = read(&answer, &big).unwrap();
        assert_eq!(read.problems.len(), 2, "{:?}", read.problems);
        assert!(read.problems[0].contains("39000 of the 40000 lines"));
        assert!(
            read.problems[1].contains("about 18: split"),
            "{:?}",
            read.problems
        );
    }

    #[test]
    fn what_overshoots_a_repository_s_size_is_trimmed() {
        let files = files(&["a.rs"]);
        let subsection = json!({"title": "S", "about": "", "kind": "api", "files": ["a.rs"]});
        let sections: Vec<Value> = (0..6)
            .map(|n| json!({"title": format!("Part {n}"), "about": "", "subsections": vec![subsection.clone(); 6]}))
            .collect();
        let read = read(&json!({"overview": "", "sections": sections}), &files).unwrap();
        assert_eq!(read.plan.sections.len(), 4);
        assert!(read.plan.sections.iter().all(|s| s.subsections.len() == 4));
        assert!(read.notes.iter().any(|note| note.contains("first 4 kept")));
    }

    #[test]
    fn source_files_left_out_go_to_the_nearest_subsection() {
        let files = files(&[
            "src/tui/app.rs",
            "src/tui/sub/deep.rs",
            "src/db.rs",
            "tests/cli.rs",
        ]);
        let sub = |id: &str, path: &str| PlannedSubsection {
            id: id.into(),
            title: id.into(),
            about: String::new(),
            kind: Kind::Component,
            files: vec![path.into()],
        };
        let mut plan = Plan {
            overview: String::new(),
            sections: vec![PlannedSection {
                id: "s".into(),
                title: "S".into(),
                about: String::new(),
                subsections: vec![sub("a", "src/db.rs"), sub("b", "src/tui/app.rs")],
            }],
        };
        assert_eq!(plan.cover(&files), 2);
        let subs = &plan.sections[0].subsections;
        assert_eq!(subs[0].files, ["src/db.rs", "tests/cli.rs"]);
        assert_eq!(subs[1].files, ["src/tui/app.rs", "src/tui/sub/deep.rs"]);
        assert!(plan.uncovered(&files).is_empty());
        assert_eq!(plan.subsection("b").unwrap().title, "b");
    }

    #[test]
    fn a_repository_is_planned_by_its_size() {
        assert_eq!(
            bounds(500),
            Bounds {
                sections: (1, 4),
                subsections: (1, 4),
                total: 1
            }
        );
        assert_eq!(bounds(10_000).total, 6);
        assert_eq!(bounds(180_000).total, 51);
        assert_eq!(bounds(2_000_000).total, 144);
        let message = message(
            "app",
            "abc",
            &files(&["src/main.rs", "README.md"]),
            Some("# App\nIt does things."),
        );
        assert!(message.contains("Plan 1 to 4 sections"), "{message}");
        assert!(message.contains("about 1 subsections in all"), "{message}");
        assert!(
            message.contains("Its README, what it's meant to do:\n\n# App"),
            "{message}"
        );
        assert!(
            message.contains("src/main.rs (100 lines) [src]\n"),
            "{message}"
        );
        assert!(message.contains("README.md (100 lines)\n"), "{message}");
    }

    #[test]
    fn the_entry_points_are_found_by_their_names() {
        let files = files(&["src/main.rs", "cmd/tool/main.go", "src/x.rs", "Cargo.toml"]);
        assert_eq!(
            entrypoints(&files),
            ["src/main.rs", "Cargo.toml", "cmd/tool/main.go"]
        );
    }
}
