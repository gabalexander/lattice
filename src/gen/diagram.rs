//! What a diagram must be before it's kept on a page, and how many a text
//! may have.
//!
//! A diagram is made what mermaid in the browser draws: taken out of a
//! fence if it was put in one, its colours and styles dropped, the slips a
//! model makes most put right ([`sanitize`]), a flowchart's labels quoted
//! ([`quote_labels`]), and then it must read with lattice's own mermaid
//! reader ([`crate::mermaid::check`]); one that doesn't is a problem the
//! writer is asked to fix once, then left out. Credentials are taken out of
//! it as out of the prose.
//!
//! Besides its diagram card, a text may draw one in its prose, in a
//! ```` ```mermaid ```` fence ([`super::fences`]): a subsection or a
//! section one more, the overview up to three. A diagram that says what
//! another on the same text says, by its [`signature`], is taken out, and
//! so are those past the cap.
//!
//! The sanitizing, the signature and the cap are adapted from
//! deepwiki-by-cc's `src/lib/mermaid-sanitize.ts` and
//! `src/lib/server/ai/diagram-policy.ts` (MIT; see THIRD_PARTY_NOTICES.md);
//! the label quoting from crystal's wiki generator (MIT).

use super::fences;
use crate::secrets;
use crate::wiki::Diagram;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// How many diagrams a text may draw in its prose, besides its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Room {
    /// A subsection or a section's summary: one.
    Part,
    /// The overview, the architecture: three.
    Overview,
}

impl Room {
    fn most(self) -> usize {
        match self {
            Room::Part => 1,
            Room::Overview => 3,
        }
    }
}

/// `diagram` as the page will have it, or why it doesn't read.
pub fn check(diagram: &Diagram) -> Result<Diagram, String> {
    let mermaid = checked_source(&diagram.mermaid)?;
    Ok(Diagram {
        mermaid,
        caption: secrets::redact(diagram.caption.trim()),
    })
}

/// A diagram's source as the page will have it, or why it doesn't read.
pub fn checked_source(source: &str) -> Result<String, String> {
    let mut source = source.trim().to_string();
    if source.starts_with("```") || source.starts_with("~~~") {
        source = source
            .lines()
            .filter(|line| {
                !line.trim_start().starts_with("```") && !line.trim_start().starts_with("~~~")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let source = source
        .lines()
        .filter(|line| {
            let first = line.split_whitespace().next().unwrap_or("");
            !matches!(
                first,
                "classDef" | "class" | "style" | "linkStyle" | "click"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let source = sanitize(source.trim());
    crate::mermaid::check(&source)?;
    let first = source.split_whitespace().next().unwrap_or_default();
    let source = if matches!(first, "flowchart" | "graph") {
        quote_labels(&source)
    } else {
        source
    };
    crate::mermaid::check(&source).map_err(|why| format!("{why} (once its labels were quoted)"))?;
    Ok(secrets::redact(&source))
}

/// A quoted label with spaces inside its shape: `a[ "x" ]`.
static QUOTED_SHAPE_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(^|[^\w"])([A-Za-z][\w-]*)([\[{(])\s*"([^"\n]*)"\s*([\]})])"#)
        .expect("the pattern reads")
});

/// Elements a label may hold whose tag closes nowhere.
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// `source` with the slips models make most put right: `->>>` arrows made
/// `->>`, a `<word>` placeholder (which mermaid takes for a tag) made
/// `{word}`, or in a class diagram `~word~`, and the spaces inside a quoted
/// label's shape, `a[ "x" ]`, taken out.
pub fn sanitize(source: &str) -> String {
    let class = source
        .lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.trim().to_ascii_lowercase().starts_with("classdiagram"));
    let mut arrows = source.to_string();
    while arrows.contains("->>>") {
        arrows = arrows.replace("->>>", "->>");
    }
    let placeholders = replace_placeholders(&arrows, class);
    QUOTED_SHAPE_LABEL
        .replace_all(&placeholders, |caps: &regex::Captures| {
            let (open, close) = (&caps[3], &caps[5]);
            let matching = matches!((open, close), ("[", "]") | ("{", "}") | ("(", ")"));
            if matching {
                format!("{}{}{open}\"{}\"{close}", &caps[1], &caps[2], &caps[4])
            } else {
                caps[0].to_string()
            }
        })
        .into_owned()
}

/// Whether `source` has a `<word>` placeholder [`sanitize`] would rewrite.
pub fn has_placeholder(source: &str) -> bool {
    placeholders(source).next().is_some()
}

/// `source` with each placeholder made `{word}`, or with `class`,
/// `~word~`.
fn replace_placeholders(source: &str, class: bool) -> String {
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for (start, end, word) in placeholders(source).collect::<Vec<_>>() {
        out.push_str(&source[at..start]);
        if class {
            out.push_str(&format!("~{word}~"));
        } else {
            out.push_str(&format!("{{{word}}}"));
        }
        at = end;
    }
    out.push_str(&source[at..]);
    out
}

/// Each `<word>` in `source` that isn't an HTML element a label may hold
/// (one that closes later, or a void one like `<br>`), nor part of `<<`
/// or `>>`: where it starts and ends, and its word.
fn placeholders(source: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let bytes = source.as_bytes();
    source.match_indices('<').filter_map(move |(start, _)| {
        if start > 0 && bytes[start - 1] == b'<' {
            return None;
        }
        let rest = &source[start + 1..];
        let first = rest.chars().next()?;
        if !first.is_ascii_alphabetic() {
            return None;
        }
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(rest.len());
        let word = &rest[..len];
        let end = start + 1 + len;
        if bytes.get(end) != Some(&b'>') || bytes.get(end + 1) == Some(&b'>') {
            return None;
        }
        let tag = word.to_ascii_lowercase();
        let closes = source[end + 1..]
            .to_ascii_lowercase()
            .contains(&format!("</{tag}"));
        (!VOID_TAGS.contains(&tag.as_str()) && !closes).then_some((start, end + 1, word))
    })
}

/// A diagram's source made loose enough that two that say the same thing,
/// differing in spacing, case or a plainly quoted label, compare equal.
pub fn signature(source: &str) -> String {
    static PLAIN_LABEL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\[\s*"([a-z0-9 _-]+)"\s*\]"#).expect("the pattern reads"));
    let joined = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let spaced = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    PLAIN_LABEL
        .replace_all(&spaced, |caps: &regex::Captures| {
            format!(
                "[{}]",
                caps[1].split_whitespace().collect::<Vec<_>>().join(" ")
            )
        })
        .trim()
        .to_string()
}

/// The diagrams in a text's prose, checked against the policy.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inline {
    /// The text with its diagrams as the page will have them.
    pub text: String,
    /// What's wrong with one that the writer can put right.
    pub problems: Vec<String>,
    /// How many were taken out: past the cap, the same as another, or with
    /// `last`, still not reading.
    pub dropped: usize,
}

/// Checks the mermaid fences in `md` against `card`, the text's diagram
/// card, and `room`: each must read, none may say what the card or one
/// before it says, and no more than the room's may stay. One that doesn't
/// read is a problem, left as it is, or with `last`, taken out.
pub fn inline(md: &str, card: Option<&Diagram>, room: Room, last: bool) -> Inline {
    let mut seen: HashSet<String> = card
        .map(|card| signature(&card.mermaid))
        .into_iter()
        .collect();
    let mut kept = 0;
    let mut edits = Vec::new();
    let mut problems = Vec::new();
    let mut dropped = 0;
    for (n, fence) in fences::mermaid(md).into_iter().enumerate() {
        let take_out = |edits: &mut Vec<(std::ops::Range<usize>, String)>| {
            // The newline after it goes too, so no gap is left.
            let mut range = fence.range.clone();
            if md[range.end..].starts_with('\n') {
                range.end += 1;
            }
            edits.push((range, String::new()));
        };
        match checked_source(&fence.code) {
            Ok(source) => {
                if !seen.insert(signature(&source)) || kept >= room.most() {
                    dropped += 1;
                    take_out(&mut edits);
                    continue;
                }
                kept += 1;
                edits.push((fence.range.clone(), format!("```mermaid\n{source}\n```")));
            }
            Err(_) if last => {
                dropped += 1;
                take_out(&mut edits);
            }
            Err(why) => problems.push(format!(
                "the diagram in the text, the {} one, doesn't read: {why}",
                ordinal(n + 1)
            )),
        }
    }
    Inline {
        text: super::prose::splice(md, edits),
        problems,
        dropped,
    }
}

fn ordinal(n: usize) -> String {
    match n {
        1 => "first".into(),
        2 => "second".into(),
        3 => "third".into(),
        n => format!("{n}th"),
    }
}

/// A flowchart with every node's label that isn't quoted quoted, and every
/// edge's label that has more than words: mermaid reads `a[Loader
/// (src/x.rs)]` as a shape it doesn't know, `a["Loader (src/x.rs)"]` as
/// meant.
pub fn quote_labels(source: &str) -> String {
    let mut out = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let first = line.split_whitespace().next().unwrap_or("");
        if index == 0
            || matches!(first, "subgraph" | "end" | "direction" | "click")
            || first.starts_with("%%")
        {
            out.push(line.to_string());
            continue;
        }
        out.push(quote_line(line));
    }
    out.join("\n")
}

/// The shapes a node's label is in, longest first, each with its close.
const SHAPES: &[(&str, &str)] = &[
    ("(((", ")))"),
    ("([", "])"),
    ("[(", ")]"),
    ("[[", "]]"),
    ("((", "))"),
    ("{{", "}}"),
    ("[/", "/]"),
    ("[\\", "\\]"),
    ("[", "]"),
    ("(", ")"),
    ("{", "}"),
];

fn quote_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // Not `-`, which ends an arrow: `-->` is no node.
    let is_id = |c: char| c.is_alphanumeric() || c == '_';
    while i < chars.len() {
        let c = chars[i];
        // An edge's label: `-->|label|`.
        if c == '|'
            && let Some(close) = chars[i + 1..].iter().position(|&c| c == '|')
        {
            let label: String = chars[i + 1..i + 1 + close].iter().collect();
            let plain = label
                .chars()
                .all(|c| c.is_alphanumeric() || " _-,.'".contains(c));
            if plain || label.trim().starts_with('"') {
                out.push('|');
                out.push_str(&label);
            } else {
                out.push_str(&format!("|\"{}\"", label.replace('"', "'")));
            }
            out.push('|');
            i += close + 2;
            continue;
        }
        // A node's label, right after its id.
        let after_id = i > 0 && is_id(chars[i - 1]);
        let rest: String = chars[i..].iter().collect();
        if after_id
            && let Some((open, close)) = SHAPES.iter().find(|(open, _)| rest.starts_with(open))
        {
            let body = &rest[open.len()..];
            if let Some(end) = closing(body, open, close) {
                let label = &body[..end];
                out.push_str(open);
                if label.trim_start().starts_with('"') {
                    out.push_str(label);
                } else {
                    out.push('"');
                    out.push_str(&label.replace('"', "'"));
                    out.push('"');
                }
                out.push_str(close);
                i += open.chars().count() + label.chars().count() + close.chars().count();
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Where in `body`, a label after its shape's `open`, the shape closes:
/// at `close`, past any brackets the label opens and closes itself, or a
/// quoted part.
fn closing(body: &str, open: &str, close: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    let opener = open.chars().last()?;
    let closer = close.chars().next()?;
    for (at, c) in body.char_indices() {
        if c == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        if depth == 0 && body[at..].starts_with(close) {
            return Some(at);
        }
        if c == opener {
            depth += 1;
        } else if c == closer {
            depth -= 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagram(mermaid: &str) -> Diagram {
        Diagram {
            mermaid: mermaid.into(),
            caption: String::new(),
        }
    }

    #[test]
    fn a_diagram_is_read_its_labels_quoted_and_its_colours_dropped() {
        let given = Diagram {
            mermaid: "```mermaid\nflowchart TD\n  loader[Loader (src/loader.rs)] -->|loads (lazily)| core[\"Core\"]\n  core -.->|calls| out((Output))\n  classDef hot fill:#f00\n  style core fill:#fff\n```".into(),
            caption: " How it loads. ".into(),
        };
        let checked = check(&given).unwrap();
        assert_eq!(
            checked.mermaid,
            "flowchart TD\n  loader[\"Loader (src/loader.rs)\"] -->|\"loads (lazily)\"| core[\"Core\"]\n  core -.->|calls| out((\"Output\"))"
        );
        assert_eq!(checked.caption, "How it loads.");
        assert!(
            check(&diagram(
                "sequenceDiagram\n  participant C as CLI (src/main.rs)\n  C->>D: build"
            ))
            .is_ok()
        );
        assert!(check(&diagram("flowchart TD\n  a --> b\n  end")).is_err());
        assert!(check(&diagram("pie\n \"a\": 1")).is_err());
    }

    #[test]
    fn the_slips_models_make_are_put_right() {
        assert_eq!(
            sanitize("sequenceDiagram\n  A->>>B: hi"),
            "sequenceDiagram\n  A->>B: hi"
        );
        assert_eq!(
            sanitize("flowchart TD\n  a[\"out/<kind>/<id>\"] --> b[\"a<br>b\"]"),
            "flowchart TD\n  a[\"out/{kind}/{id}\"] --> b[\"a<br>b\"]"
        );
        assert_eq!(
            sanitize("classDiagram\n  class Box<T>"),
            "classDiagram\n  class Box~T~"
        );
        assert_eq!(
            sanitize("flowchart TD\n  pick{ \"Which model\" } --> a[ \"A\" ]"),
            "flowchart TD\n  pick{\"Which model\"} --> a[\"A\"]"
        );
        // An element that closes, and `<<interface>>`, stay.
        assert_eq!(
            sanitize("classDiagram\n  class A {\n    <<interface>>\n  }\n  note \"<b>x</b>\""),
            "classDiagram\n  class A {\n    <<interface>>\n  }\n  note \"<b>x</b>\""
        );
        assert!(has_placeholder("a[\"<id>\"]"));
        assert!(!has_placeholder("a[\"x<br/>y\"]"));
    }

    #[test]
    fn two_diagrams_that_say_the_same_compare_equal() {
        assert_eq!(
            signature("flowchart TD\n  A[\"Load  Config\"] --> B\n%% note"),
            signature("flowchart td\n    a[load config] -->   b")
        );
        assert_ne!(
            signature("flowchart TD\n  a --> b"),
            signature("flowchart TD\n  a --> c")
        );
    }

    #[test]
    fn a_text_keeps_the_diagrams_its_room_has_once_each() {
        let card = diagram("flowchart TD\n  a --> b");
        let md = "One.\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\nTwo.\n\n```mermaid\nsequenceDiagram\n  A->>>B: go\n```\n\nThree.\n\n```mermaid\nflowchart LR\n  x --> y\n```\nEnd.";
        let part = inline(md, Some(&card), Room::Part, false);
        assert!(part.problems.is_empty(), "{:?}", part.problems);
        assert_eq!(part.dropped, 2);
        assert_eq!(
            part.text,
            "One.\n\n\nTwo.\n\n```mermaid\nsequenceDiagram\n  A->>B: go\n```\n\nThree.\n\nEnd."
        );
        let overview = inline(md, Some(&card), Room::Overview, false);
        assert_eq!(overview.dropped, 1);
        assert_eq!(fences::mermaid(&overview.text).len(), 2);
    }

    #[test]
    fn a_diagram_in_the_text_that_does_not_read_is_a_problem_then_taken_out() {
        let md = "Before.\n\n```mermaid\npie\n  \"a\": 1\n```\n\nAfter.";
        let first = inline(md, None, Room::Part, false);
        assert_eq!(first.text, md);
        assert!(
            first.problems[0].contains("the first one, doesn't read"),
            "{:?}",
            first.problems
        );
        let last = inline(md, None, Room::Part, true);
        assert_eq!(last.text, "Before.\n\n\nAfter.");
        assert_eq!(last.dropped, 1);
    }
}
