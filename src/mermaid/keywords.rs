//! Names in a diagram that are words mermaid keeps for itself: renamed
//! before a diagram is kept ([`rename`]), and refused by [`super::check`]
//! when one is left ([`reserved_name`]).
//!
//! Mermaid's lexers try their keywords first wherever a token starts, so a
//! node called `call` begins a click's callback, `end` closes a subgraph
//! and a participant called `Note` starts a note: each is a parse error in
//! the browser, though the readers here take them for names. Such a name
//! gets a `_` after its keyword, the same everywhere in the diagram (`call`
//! to `call_`, `end-x` to `end_-x`), and still shows what it did: a
//! flowchart's node or subgraph gets its old name as its label when it had
//! none, a participant as its alias, a class as its label, a state as its
//! description; an entity is quoted instead. Labels, quoted text and edges
//! are left as they are, but for two slips of the same kind in a
//! flowchart: an id starting with o or x right after an edge with no head,
//! which mermaid takes for the edge's head (`a---order` is a
//! circle-headed edge to `rder`), and a node `o` or `x` touching its edge.
//!
//! The words are mermaid 11.17.2's, from its flow, sequence, class, state
//! and er grammars, where a keyword ends where a word does. The web app
//! repairs a diagram the same way before mermaid draws it
//! (`web/src/lib/mermaid-keywords.ts`), for the wikis written before this,
//! and `tests/mermaid/keywords.json` holds the cases both are held to.

use super::{Diagram, Kind};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// The words `kind` keeps for itself; those of a kind whose lexer ignores
/// case are in lower case.
pub fn keywords(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Flowchart => &[
            "call",
            "class",
            "classDef",
            "click",
            "default",
            "end",
            "flowchart",
            "graph",
            "href",
            "interpolate",
            "linkStyle",
            "style",
            "subgraph",
            "_blank",
            "_parent",
            "_self",
            "_top",
        ],
        Kind::Sequence => &[
            "activate",
            "actor",
            "alt",
            "and",
            "autonumber",
            "box",
            "break",
            "create",
            "critical",
            "deactivate",
            "destroy",
            "details",
            "else",
            "end",
            "link",
            "links",
            "loop",
            "note",
            "off",
            "opt",
            "option",
            "over",
            "par",
            "par_over",
            "participant",
            "properties",
            "rect",
            "sequencediagram",
            "title",
        ],
        Kind::Class => &[
            "call",
            "callback",
            "class",
            "classDef",
            "click",
            "cssClass",
            "href",
            "link",
            "namespace",
            "note",
            "o",
            "style",
            "_blank",
            "_parent",
            "_self",
            "_top",
        ],
        Kind::State => &[
            "class",
            "classdef",
            "click",
            "default",
            "href",
            "note",
            "scale",
            "state",
            "statediagram",
            "style",
        ],
        Kind::Er => &[
            "class",
            "classdef",
            "end",
            "erdiagram",
            "many",
            "one",
            "style",
            "subgraph",
            "to",
        ],
    }
}

/// How long the keyword `name` starts with is, when it does and the word
/// ends there; otherwise 0.
pub fn reserved(kind: Kind, name: &str) -> usize {
    let caseless = matches!(kind, Kind::Sequence | Kind::State | Kind::Er);
    let folded = if caseless {
        name.to_ascii_lowercase()
    } else {
        name.to_string()
    };
    keywords(kind)
        .iter()
        .find(|word| {
            folded.starts_with(*word)
                && !folded[word.len()..]
                    .bytes()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        })
        .map_or(0, |word| word.len())
}

/// A name in `diagram` that is a keyword of its kind, which mermaid can't
/// read: a node's, a participant's, a class's, a state's or an entity's.
pub fn reserved_name(diagram: &Diagram) -> Option<String> {
    let kind = diagram.kind();
    let names: Vec<&str> = match diagram {
        Diagram::Sequence(d) => d.participants.iter().map(|p| p.id.as_str()).collect(),
        Diagram::Graph { graph, .. } => graph.nodes.iter().map(|n| n.id.as_str()).collect(),
    };
    names
        .into_iter()
        .find(|name| reserved(kind, name) > 0)
        .map(str::to_string)
}

/// `name`, a keyword of `kind`, with a `_` after its keyword.
pub fn renamed(kind: Kind, name: &str) -> String {
    let cut = reserved(kind, name);
    format!("{}_{}", &name[..cut], &name[cut..])
}

/// What a name can be given to show its old name, where nothing shows one.
#[derive(Debug, Clone, Copy)]
enum Give {
    /// `["old"]`, after a node, a subgraph or a class.
    Label,
    /// ` as old`, after a participant.
    Alias,
    /// `"old" as `, before a state.
    Description,
}

/// Where a name is, on its line.
#[derive(Debug, Clone)]
struct Mention {
    line: usize,
    start: usize,
    end: usize,
    /// Whether the name shows a text of its own here: a label, an alias, a
    /// description.
    shows: bool,
    /// Where on this line it can be given its old name to show, and how.
    give: Option<(usize, Give)>,
}

/// A text put in the place of `del` bytes at `at`, on a line.
#[derive(Debug, Clone)]
struct Edit {
    line: usize,
    at: usize,
    del: usize,
    text: String,
}

/// What a kind's scan found: its names, and its own repairs.
#[derive(Debug, Default)]
struct Scan {
    mentions: Vec<Mention>,
    edits: Vec<Edit>,
}

impl Scan {
    fn mention(&mut self, line: usize, start: usize, end: usize) {
        self.mentions.push(Mention {
            line,
            start,
            end,
            shows: false,
            give: None,
        });
    }
}

/// `source` with every name that is a keyword of its kind renamed, or
/// quoted, as the module says.
pub fn rename(source: &str) -> String {
    let lines: Vec<&str> = source.split('\n').collect();
    let (Some(head), Some(Ok(kind))) = (first_statement(&lines), super::detect(source)) else {
        return source.to_string();
    };
    let mut scan = Scan::default();
    match kind {
        Kind::Flowchart => scan_flowchart(&lines, head, &mut scan),
        Kind::Sequence => scan_sequence(&lines, head, &mut scan),
        Kind::Class => scan_class(&lines, head, &mut scan),
        Kind::State => scan_state(&lines, head, &mut scan),
        Kind::Er => scan_er(&lines, head, &mut scan),
    }
    let name = |m: &Mention| &lines[m.line][m.start..m.end];
    let mut taken: HashSet<String> = scan.mentions.iter().map(|m| name(m).to_string()).collect();
    let mut by_name: Vec<(&str, Vec<&Mention>)> = Vec::new();
    for m in &scan.mentions {
        let old = name(m);
        if reserved(kind, old) == 0 {
            continue;
        }
        match by_name.iter_mut().find(|(n, _)| *n == old) {
            Some((_, found)) => found.push(m),
            None => by_name.push((old, vec![m])),
        }
    }
    let mut edits = scan.edits.clone();
    let mut before: Vec<(usize, String)> = Vec::new();
    for (old, mentions) in by_name {
        if kind == Kind::Er {
            for m in mentions {
                edits.push(Edit {
                    line: m.line,
                    at: m.start,
                    del: m.end - m.start,
                    text: format!("\"{old}\""),
                });
            }
            continue;
        }
        let cut = reserved(kind, old);
        let mut mark = "_".to_string();
        while taken.contains(&format!("{}{mark}{}", &old[..cut], &old[cut..])) {
            mark.push('_');
        }
        let fresh = format!("{}{mark}{}", &old[..cut], &old[cut..]);
        taken.insert(fresh.clone());
        for m in &mentions {
            edits.push(Edit {
                line: m.line,
                at: m.start,
                del: m.end - m.start,
                text: fresh.clone(),
            });
        }
        if mentions.iter().any(|m| m.shows) {
            continue;
        }
        if let Some((m, (at, give))) = mentions.iter().find_map(|m| Some((m, m.give?))) {
            let text = match give {
                Give::Label => format!("[\"{old}\"]"),
                Give::Alias => format!(" as {old}"),
                Give::Description => format!("\"{old}\" as "),
            };
            edits.push(Edit {
                line: m.line,
                at,
                del: 0,
                text,
            });
        } else if let Some(line) = declare(kind, &fresh, old) {
            let first = mentions[0].line;
            let indent = &lines[first][..lines[first].len() - lines[first].trim_start().len()];
            before.push((first, format!("{indent}{line}")));
        }
    }
    if edits.is_empty() && before.is_empty() {
        return source.to_string();
    }
    let mut out = Vec::with_capacity(lines.len() + before.len());
    for (n, line) in lines.iter().enumerate() {
        out.extend(
            before
                .iter()
                .filter(|(at, _)| *at == n)
                .map(|(_, text)| text.clone()),
        );
        let mut mine: Vec<&Edit> = edits.iter().filter(|e| e.line == n).collect();
        mine.sort_by_key(|e| (e.at, e.del));
        let mut text = String::new();
        let mut at = 0;
        for e in mine {
            text.push_str(&line[at..e.at]);
            text.push_str(&e.text);
            at = e.at + e.del;
        }
        text.push_str(&line[at..]);
        out.push(text);
    }
    out.join("\n")
}

/// The line that declares `fresh` showing `old`, for a kind whose names
/// show themselves, when no mention of it could.
fn declare(kind: Kind, fresh: &str, old: &str) -> Option<String> {
    match kind {
        Kind::Sequence => Some(format!("participant {fresh} as {old}")),
        Kind::Class => Some(format!("class {fresh}[\"{old}\"]")),
        Kind::State => Some(format!("state \"{old}\" as {fresh}")),
        Kind::Flowchart | Kind::Er => None,
    }
}

/// The diagram's first statement: past blank lines, `%%` comments and a
/// `---` front matter block.
fn first_statement(lines: &[&str]) -> Option<usize> {
    let mut front = false;
    for (n, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line == "---" {
            front = !front;
            continue;
        }
        if front || line.is_empty() || line.starts_with("%%") {
            continue;
        }
        return Some(n);
    }
    None
}

/// Each line after the header, trimmed, with where its text starts; blank
/// lines and comments left out.
fn statements<'a>(
    lines: &'a [&'a str],
    head: usize,
) -> impl Iterator<Item = (usize, &'a str, usize)> + 'a {
    lines
        .iter()
        .enumerate()
        .skip(head + 1)
        .filter_map(|(n, line)| {
            let text = line.trim();
            (!text.is_empty() && !text.starts_with("%%"))
                .then(|| (n, text, line.len() - line.trim_start().len()))
        })
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the pattern reads")
}

// --- flowchart ---

/// The shapes a node's label opens with.
const OPENERS: &[&str] = &[
    "(((", "((", "([", "[[", "[(", "[/", "[\\", "{{", "[", "(", "{", ">",
];

/// The edges that close a `-- text -->` label.
const CLOSERS: &[&str] = &["-->", "--x", "--o", "---", "==>", "===", ".->", "-.-", ".-"];

/// The statements that style a node by its id.
const STYLING: &[&str] = &["style", "classDef", "linkStyle", "class", "click"];

fn scan_flowchart(lines: &[&str], head: usize, scan: &mut Scan) {
    for (n, line) in lines.iter().enumerate().skip(head) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        for (k, (at, text)) in split_statements(line).into_iter().enumerate() {
            if n == head && k == 0 {
                continue;
            }
            let lead = text.len() - text.trim_start().len();
            flow_statement(scan, n, at + lead, text.trim());
        }
    }
}

/// A line cut at the `;` that end statements, outside quotes and brackets:
/// where each starts, and its text.
fn split_statements(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            _ if quoted => {}
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            ';' if depth <= 0 => {
                out.push((start, &line[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push((start, &line[start..]));
    out.retain(|(_, text)| !text.trim().is_empty());
    out
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// How long the node id `t` starts with is: letters, digits, `_`, `.` and
/// anything past ASCII, with a `-` only where an edge doesn't start.
fn node_id(t: &str) -> usize {
    let b = t.as_bytes();
    let id = |c: u8| is_word(c) || c >= 0x80;
    if !b.first().is_some_and(|&c| id(c)) {
        return 0;
    }
    let mut i = 1;
    while i < b.len() {
        if id(b[i]) || b[i] == b'.' || b[i] == b'-' && b.get(i + 1).is_some_and(|&c| is_word(c)) {
            i += 1;
        } else {
            break;
        }
    }
    i
}

fn flow_statement(scan: &mut Scan, n: usize, off: usize, t: &str) {
    let word_len = match t.as_bytes().first() {
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {
            t.bytes().take_while(|&c| is_word(c)).count()
        }
        _ => 0,
    };
    let (word, after) = t.split_at(word_len);
    if word == "end" && after.trim().is_empty() {
        return;
    }
    if word == "direction"
        && after.starts_with(char::is_whitespace)
        && matches!(after.trim(), "TB" | "TD" | "BT" | "LR" | "RL")
    {
        return;
    }
    if matches!(word, "accTitle" | "accDescr") && after.trim_start().starts_with([':', '{']) {
        return;
    }
    // A statement word, unless an edge, a `&` or a shape follows it: then it
    // names a node.
    let statement = after.starts_with(char::is_whitespace)
        && after
            .trim_start()
            .starts_with(|c: char| !"&<~=.-".contains(c));
    let rest = after.trim_start();
    let at = off + word.len() + (after.len() - rest.len());
    if word == "subgraph" && (rest.is_empty() || statement) {
        subgraph(scan, n, at, rest.trim_end());
        return;
    }
    if STYLING.contains(&word) && statement {
        // `style id css`, `class a,b name`, `click id ...`: the ids are
        // renamed with the nodes'.
        if matches!(word, "classDef" | "linkStyle") {
            return;
        }
        let ids = rest.split(char::is_whitespace).next().unwrap_or("");
        let list: Vec<&str> = if word == "class" {
            ids.split(',').collect()
        } else {
            vec![ids]
        };
        let mut from = at;
        for id in list {
            if !id.is_empty() {
                scan.mention(n, from, from + id.len());
            }
            from += id.len() + 1;
        }
        return;
    }
    nodes(scan, n, off, t);
}

/// `subgraph id`, `subgraph id [title]` or `subgraph a title of words`, the
/// last quoted when one of its words is a keyword, since it's read as ids
/// would be.
fn subgraph(scan: &mut Scan, n: usize, off: usize, rest: &str) {
    if rest.is_empty() || rest.starts_with('"') {
        return;
    }
    let len = node_id(rest);
    if len == 0 {
        return;
    }
    let after = &rest[len..];
    if after.trim_start().starts_with('[') {
        scan.mentions.push(Mention {
            line: n,
            start: off,
            end: off + len,
            shows: true,
            give: None,
        });
    } else if after.trim().is_empty() {
        scan.mentions.push(Mention {
            line: n,
            start: off,
            end: off + len,
            shows: false,
            give: Some((off + len, Give::Label)),
        });
    } else if rest
        .split_whitespace()
        .any(|word| reserved(Kind::Flowchart, word) > 0)
    {
        scan.edits.push(Edit {
            line: n,
            at: off,
            del: rest.len(),
            text: format!("\"{}\"", rest.replace('"', "#quot;")),
        });
    }
}

/// A statement of nodes and edges: `a["A"] --> b & c -.->|label| d`.
fn nodes(scan: &mut Scan, n: usize, off: usize, t: &str) {
    let b = t.as_bytes();
    let mut i = 0;
    let mut node = true;
    while i < b.len() {
        while matches!(b.get(i), Some(b' ' | b'\t')) {
            i += 1;
        }
        if i >= b.len() {
            return;
        }
        if node {
            let len = node_id(&t[i..]);
            if len == 0 {
                return;
            }
            let start = i;
            i += len;
            let mut shows = false;
            let mut shaped = false;
            if t[i..].starts_with("@{") {
                let Some(end) = close_of(t, i + 1, b'{', b'}') else {
                    return;
                };
                static LABEL: LazyLock<Regex> = LazyLock::new(|| regex(r"\blabel\s*:"));
                shows = LABEL.is_match(&t[i..end]);
                shaped = true;
                i = end;
            } else if OPENERS.iter().any(|open| t[i..].starts_with(open)) {
                let end = match b[i] {
                    b'>' => t[i..].find(']').map(|k| i + k + 1),
                    b'[' => close_of(t, i, b'[', b']'),
                    b'(' => close_of(t, i, b'(', b')'),
                    _ => close_of(t, i, b'{', b'}'),
                };
                let Some(end) = end else {
                    return;
                };
                shows = true;
                shaped = true;
                i = end;
            }
            if t[i..].starts_with(":::") {
                i += 3;
                while b.get(i).is_some_and(|&c| is_word(c) || c == b'-') {
                    i += 1;
                }
            }
            let end = off + start + len;
            scan.mentions.push(Mention {
                line: n,
                start: off + start,
                end,
                shows,
                give: (!shaped).then_some((end, Give::Label)),
            });
            // A node `o` or `x` touching its edge is read as the edge's
            // tail: `o-->b`.
            if matches!(&t[start..start + len], "o" | "x")
                && i == start + 1
                && matches!(b.get(i), Some(b'-' | b'=' | b'.'))
            {
                scan.edits.push(Edit {
                    line: n,
                    at: end,
                    del: 0,
                    text: " ".into(),
                });
            }
            node = false;
        } else if b[i] == b'&' {
            i += 1;
            node = true;
        } else {
            let Some((end, fix)) = edge_at(t, i) else {
                return;
            };
            if let Some((at, text)) = fix {
                scan.edits.push(Edit {
                    line: n,
                    at: off + at,
                    del: 0,
                    text,
                });
            }
            i = end;
            node = true;
        }
    }
}

/// Where the bracket opened at `at` closes, past brackets the label opens
/// itself and quoted text.
fn close_of(t: &str, at: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0;
    let mut quoted = false;
    for (i, &c) in t.as_bytes().iter().enumerate().skip(at) {
        if c == b'"' {
            quoted = !quoted;
        } else if quoted {
            continue;
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
    }
    None
}

/// The edge at `i`, as the reader takes it ([`super::flowchart`]): where it
/// ends, past its label, and the space it needs when an id starting with o
/// or x touches a headless one.
fn edge_at(t: &str, mut i: usize) -> Option<(usize, Option<(usize, String)>)> {
    let b = t.as_bytes();
    let line = |c: Option<&u8>| matches!(c, Some(b'-' | b'=' | b'.'));
    if t[i..].starts_with("~~~") {
        while b.get(i) == Some(&b'~') {
            i += 1;
        }
        return Some((i, None));
    }
    let mut tail = false;
    if b[i] == b'<' || matches!(b[i], b'x' | b'o') && line(b.get(i + 1)) && line(b.get(i + 2)) {
        tail = true;
        i += 1;
    }
    let from = i;
    while line(b.get(i)) {
        i += 1;
    }
    let body = &t[from..i];
    if body.len() < 2 {
        return None;
    }
    let mut head = false;
    let mut fix = None;
    match b.get(i) {
        Some(b'>') => {
            head = true;
            i += 1;
        }
        Some(b'x' | b'o') if b.get(i + 1).is_some_and(|&c| is_word(c)) => {
            static HEADLESS: LazyLock<Regex> = LazyLock::new(|| regex(r"^(-{2,}|={2,}|-?\.+-)$"));
            if HEADLESS.is_match(body) {
                let more = match body {
                    "--" => "-",
                    "==" => "=",
                    _ => "",
                };
                fix = Some((i, format!("{more} ")));
            }
        }
        Some(b'x' | b'o') => {
            head = true;
            i += 1;
        }
        _ => {}
    }
    // `-- text -->`: an opener, the text, then the edge that closes it.
    if !head && !tail && body.len() == 2 && matches!(b.get(i), Some(b' ' | b'\t')) {
        let best = CLOSERS
            .iter()
            .filter_map(|closer| closer_at(&t[i..], closer))
            .min();
        if let Some(k) = best {
            i += k;
            while line(b.get(i)) {
                i += 1;
            }
            if matches!(b.get(i), Some(b'>' | b'x' | b'o')) {
                i += 1;
            }
            return Some((i, None));
        }
    }
    let mut j = i;
    while matches!(b.get(j), Some(b' ' | b'\t')) {
        j += 1;
    }
    if b.get(j) == Some(&b'|') {
        let end = t[j + 1..].find('|').map_or(t.len(), |k| j + 1 + k + 1);
        return Some((end, fix));
    }
    Some((i, fix))
}

/// Where `closer` starts in `rest` after a space or an edge's own
/// character, so `step-by-step` isn't an edge.
fn closer_at(rest: &str, closer: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(k) = rest[from..].find(closer).map(|k| from + k) {
        if k > 0 && matches!(rest.as_bytes()[k - 1], b' ' | b'-' | b'=' | b'.') {
            return Some(k);
        }
        from = k + 1;
    }
    None
}

// --- sequenceDiagram ---

/// The message arrows, longest first where two start at once.
const ARROWS: &[&str] = &[
    "<<-->>", "<<->>", "-->>", "->>", "--x", "-x", "--)", "-)", "-->", "->",
];

/// The words whose statement runs to the end of the line, arrows and all:
/// a frame's text, a title.
const FREE_TEXT: &[&str] = &[
    "alt", "and", "box", "break", "critical", "else", "loop", "opt", "option", "par", "par_over",
    "rect", "title",
];

/// The name in `text`, trimmed, at `at` on line `n`.
fn mention_trimmed(scan: &mut Scan, n: usize, at: usize, text: &str) {
    let id = text.trim();
    if !id.is_empty() {
        let from = at + text.len() - text.trim_start().len();
        scan.mention(n, from, from + id.len());
    }
}

fn scan_sequence(lines: &[&str], head: usize, scan: &mut Scan) {
    static NOTE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)^\s+(over|left of|right of)\s"));
    static DECLARED: LazyLock<Regex> =
        LazyLock::new(|| regex(r"(?i)^(?:create\s+)?(?:participant|actor)\s+"));
    static ALIAS: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)\s+as\s+"));
    for (n, t, at) in statements(lines, head) {
        let before = &t[..t.find(':').unwrap_or(t.len())];
        let first = t.split(char::is_whitespace).next().unwrap_or("");
        let word = first.to_ascii_lowercase();
        let rest = before.get(first.len()..).unwrap_or("");
        let arrow = arrow_in(before);
        // `Loop->>B` and `Loop ->> B` are messages, `loop every 5 -> 10s` a
        // frame.
        let statement = FREE_TEXT.contains(&word.as_str())
            && arrow
                .is_none_or(|(k, _)| !before.get(first.len()..k).unwrap_or("").trim().is_empty());
        let note = if word == "note" {
            NOTE.find(rest)
        } else {
            None
        };
        if let Some((k, len)) = arrow
            && !statement
            && note.is_none()
        {
            mention_trimmed(scan, n, at, &before[..k]);
            let mut k = k + len;
            while before.as_bytes().get(k) == Some(&b' ') {
                k += 1;
            }
            if matches!(before.as_bytes().get(k), Some(b'+' | b'-')) {
                k += 1;
            }
            mention_trimmed(scan, n, at + k, &before[k..]);
            continue;
        }
        if let Some(declared) = DECLARED.find(t) {
            let decl = &t[declared.end()..];
            let alias = ALIAS.find(decl);
            let config = decl.find("@{");
            let id = decl[..alias.map(|a| a.start()).or(config).unwrap_or(decl.len())].trim_end();
            let from = at + declared.end();
            if !id.is_empty() {
                scan.mentions.push(Mention {
                    line: n,
                    start: from,
                    end: from + id.len(),
                    shows: alias.is_some() || config.is_some(),
                    give: Some((from + id.len(), Give::Alias)),
                });
            }
        } else if matches!(word.as_str(), "destroy" | "activate" | "deactivate") {
            mention_trimmed(scan, n, at + first.len(), rest);
        } else if let Some(place) = note {
            let mut from = first.len() + place.end();
            for who in before[from..].split(',') {
                mention_trimmed(scan, n, at + from, who);
                from += who.len() + 1;
            }
        } else if matches!(word.as_str(), "links" | "link" | "properties" | "details") {
            mention_trimmed(scan, n, at + first.len(), rest);
        }
    }
}

/// The first message arrow in `text`: where it starts, and how long it is.
fn arrow_in(text: &str) -> Option<(usize, usize)> {
    text.bytes()
        .enumerate()
        .filter(|(_, c)| matches!(c, b'-' | b'<'))
        .find_map(|(i, _)| {
            ARROWS
                .iter()
                .find(|arrow| text[i..].starts_with(*arrow))
                .map(|arrow| (i, arrow.len()))
        })
}

// --- classDiagram ---

/// How long the class name `t` starts with is.
fn class_name(t: &str) -> usize {
    t.bytes().take_while(|&c| is_word(c) || c >= 0x80).count()
}

fn scan_class(lines: &[&str], head: usize, scan: &mut Scan) {
    static LAST: LazyLock<Regex> =
        LazyLock::new(|| regex(r"([A-Za-z0-9_\x{80}-\x{10FFFF}]+)(~[^~]*~)?\s*$"));
    static DECLARED: LazyLock<Regex> = LazyLock::new(|| regex(r"^class\s+"));
    static GENERIC: LazyLock<Regex> = LazyLock::new(|| regex(r"^~[^~]*~"));
    static NAMED: LazyLock<Regex> = LazyLock::new(|| {
        regex(r"^(?:note\s+for\s+|<<[^>]*>>\s*|(?:style|click|link|callback)\s+)")
    });
    let mut body = false;
    for (n, t, at) in statements(lines, head) {
        if body {
            body = !t.starts_with('}');
            continue;
        }
        let len = class_name(t);
        if len > 0 && t[len..].trim_start().starts_with(':') {
            scan.mention(n, at, at + len);
            continue;
        }
        // The line with its quoted text blanked, byte for byte.
        let mut plain = String::with_capacity(t.len());
        let mut quoted = false;
        for c in t.chars() {
            if c == '"' {
                quoted = !quoted;
            }
            if quoted || c == '"' {
                plain.extend(std::iter::repeat_n(' ', c.len_utf8()));
            } else {
                plain.push(c);
            }
        }
        if plain.contains("--") || plain.contains("..") {
            // `A "1" <|-- "*" B : label`: the names that start and end the
            // relation.
            if len > 0 {
                scan.mention(n, at, at + len);
            }
            let rel = &plain[..plain.find(':').unwrap_or(plain.len())];
            if let Some(last) = LAST.captures(rel).and_then(|caps| caps.get(1)) {
                scan.mention(n, at + last.start(), at + last.end());
            }
            continue;
        }
        if let Some(declared) = DECLARED.find(t) {
            let from = declared.end();
            let len = class_name(&t[from..]);
            if len == 0 {
                continue;
            }
            let mut slot = from + len;
            if let Some(generic) = GENERIC.find(&t[slot..]) {
                slot += generic.end();
            }
            scan.mentions.push(Mention {
                line: n,
                start: at + from,
                end: at + from + len,
                shows: t[slot..].starts_with('['),
                give: Some((at + slot, Give::Label)),
            });
            body = t.contains('{') && !t.contains('}');
            continue;
        }
        if let Some(named) = NAMED.find(t) {
            let len = class_name(&t[named.end()..]);
            if len > 0 {
                scan.mention(n, at + named.end(), at + named.end() + len);
            }
        }
    }
}

// --- stateDiagram ---

/// How long the state id `t` starts with is.
fn state_id(t: &str) -> usize {
    t.find(|c: char| c.is_whitespace() || ":{<[-".contains(c))
        .unwrap_or(t.len())
}

fn mention_state(scan: &mut Scan, n: usize, at: usize, text: &str) {
    let lead = text.len() - text.trim_start().len();
    let len = state_id(&text[lead..]);
    if len > 0 {
        scan.mention(n, at + lead, at + lead + len);
    }
}

fn scan_state(lines: &[&str], head: usize, scan: &mut Scan) {
    static END_NOTE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)^end note$"));
    static DECLARED: LazyLock<Regex> =
        LazyLock::new(|| regex(r#"(?i)^state\s+("[^"]*"\s+as\s+)?"#));
    static FORK: LazyLock<Regex> = LazyLock::new(|| regex(r"^\s*(<<|\[\[)"));
    static PLACE: LazyLock<Regex> = LazyLock::new(|| regex(r"(?i)^note\s+(left|right)\s+of\s+"));
    static STYLED: LazyLock<Regex> =
        LazyLock::new(|| regex(r"(?i)^(class|style|click)\s+([^\s:]\S*)"));
    static FLOATING: LazyLock<Regex> = LazyLock::new(|| regex(r#"(?i)^note\s+""#));
    let mut note = false;
    for (n, t, at) in statements(lines, head) {
        if note {
            note = !END_NOTE.is_match(t);
            continue;
        }
        let colon = t.find(':');
        if let Some(arrow) = t.find("-->")
            && colon.is_none_or(|colon| arrow < colon)
        {
            mention_state(scan, n, at, &t[..arrow]);
            mention_state(scan, n, at + arrow + 3, &t[arrow + 3..]);
            continue;
        }
        let word = t
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let declared = DECLARED
            .captures(t)
            .map(|caps| (caps.get(0).map_or(0, |m| m.end()), caps.get(1).is_some()))
            .filter(|(end, _)| state_id(&t[*end..]) > 0);
        if let Some((from, described)) = declared {
            let len = state_id(&t[from..]);
            scan.mentions.push(Mention {
                line: n,
                start: at + from,
                end: at + from + len,
                shows: described || FORK.is_match(&t[from + len..]),
                give: Some((at + from, Give::Description)),
            });
        } else if let Some(place) = PLACE.find(t) {
            mention_state(scan, n, at + place.end(), &t[place.end()..]);
            note = colon.is_none();
        } else if let Some(styled) = STYLED.captures(t) {
            let ids = styled.get(2).expect("the pattern has it");
            let list: Vec<&str> = if word == "click" {
                vec![ids.as_str()]
            } else {
                ids.as_str().split(',').collect()
            };
            let mut from = at + ids.start();
            for id in list {
                mention_state(scan, n, from, id);
                from += id.len() + 1;
            }
        } else if !FLOATING.is_match(t)
            && !matches!(
                word.as_str(),
                "classdef"
                    | "direction"
                    | "scale"
                    | "hide"
                    | "acctitle"
                    | "accdescr"
                    | "--"
                    | "{"
                    | "}"
            )
        {
            mention_state(scan, n, at, t);
        }
    }
}

// --- erDiagram ---

fn scan_er(lines: &[&str], head: usize, scan: &mut Scan) {
    static WORD: LazyLock<Regex> = LazyLock::new(|| regex(r"\S+"));
    let entity = |scan: &mut Scan, n: usize, at: usize, id: &str| {
        if !id.is_empty() && !id.starts_with('"') {
            scan.mention(n, at, at + id.len());
        }
    };
    let mut body = false;
    for (n, t, at) in statements(lines, head) {
        if body {
            body = !t.starts_with('}');
            continue;
        }
        if t.ends_with('{') {
            let len = t
                .find(|c: char| c.is_whitespace() || c == '[' || c == '{')
                .unwrap_or(t.len());
            entity(scan, n, at, &t[..len]);
            body = true;
            continue;
        }
        let colon = t.find(':');
        let words: Vec<_> = WORD.find_iter(&t[..colon.unwrap_or(t.len())]).collect();
        match words.as_slice() {
            [a, op, b] if op.as_str().contains("--") || op.as_str().contains("..") => {
                entity(scan, n, at + a.start(), a.as_str());
                entity(scan, n, at + b.start(), b.as_str());
            }
            [only]
                if colon.is_none()
                    && !matches!(
                        t.to_ascii_lowercase().as_str(),
                        "style" | "classdef" | "class" | "direction"
                    ) =>
            {
                entity(scan, n, at + only.start(), only.as_str());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// `tests/mermaid/keywords.json`: the words, a diagram for each kind
    /// with `{w}` where a word goes, and the cases.
    fn fixture() -> Value {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mermaid/keywords.json");
        serde_json::from_str(&std::fs::read_to_string(path).expect("the cases")).expect("JSON")
    }

    const KINDS: &[(&str, Kind)] = &[
        ("flowchart", Kind::Flowchart),
        ("sequence", Kind::Sequence),
        ("class", Kind::Class),
        ("state", Kind::State),
        ("er", Kind::Er),
    ];

    #[test]
    fn the_keywords_are_the_ones_the_web_app_knows() {
        let fixture = fixture();
        for (name, kind) in KINDS {
            let words: Vec<&str> = fixture["words"][name]
                .as_array()
                .expect("the words")
                .iter()
                .map(|w| w.as_str().expect("a word"))
                .collect();
            assert_eq!(keywords(*kind), words.as_slice(), "{name}");
        }
    }

    #[test]
    fn every_keyword_is_renamed_where_it_names_something_and_shows_its_old_name() {
        let fixture = fixture();
        for (name, kind) in KINDS {
            let template = &fixture["templates"][name];
            let fill = |key: &str, word: &str| {
                template[key]
                    .as_str()
                    .expect("a template")
                    .replace("{w}", word)
            };
            for word in keywords(*kind) {
                let fixed = rename(&fill("before", word));
                assert_eq!(fixed, fill("after", word), "{name}: {word}");
                let read = super::super::parse(&fixed).expect("it reads");
                assert_eq!(reserved_name(&read), None, "{name}: {word}");
            }
        }
    }

    #[test]
    fn the_cases_both_ends_agree_on() {
        for case in fixture()["cases"].as_array().expect("the cases") {
            let given = case["before"].as_str().expect("before");
            assert_eq!(
                rename(given),
                case["after"].as_str().expect("after"),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn a_name_is_reserved_only_as_a_whole_word_and_in_its_kind_s_case() {
        assert_eq!(reserved(Kind::Flowchart, "call"), 4);
        assert_eq!(reserved(Kind::Flowchart, "end-x"), 3);
        assert_eq!(reserved(Kind::Flowchart, "end.x"), 3);
        assert_eq!(reserved(Kind::Flowchart, "ending"), 0);
        assert_eq!(reserved(Kind::Flowchart, "end_x"), 0);
        assert_eq!(reserved(Kind::Flowchart, "End"), 0);
        assert_eq!(reserved(Kind::Sequence, "Note"), 4);
        assert_eq!(reserved(Kind::Sequence, "Note taker"), 4);
        assert_eq!(reserved(Kind::Sequence, "Notes"), 0);
        assert_eq!(reserved(Kind::Er, "CLASS"), 5);
        assert_eq!(renamed(Kind::Flowchart, "end-x"), "end_-x");
    }

    #[test]
    fn a_name_left_a_keyword_is_found() {
        let read = |src: &str| super::super::parse(src).expect("it reads");
        assert_eq!(
            reserved_name(&read("flowchart LR\n  a --> call\n  call --> b")),
            Some("call".into())
        );
        assert_eq!(
            reserved_name(&read("sequenceDiagram\n  A->>Note: hi")),
            Some("Note".into())
        );
        assert_eq!(
            reserved_name(&read("erDiagram\n  \"CLASS\" ||--o{ STUDENT : has")),
            None
        );
        assert_eq!(reserved_name(&read("flowchart LR\n  calls --> End")), None);
    }
}
