//! Resolving a code span to what it names. A span is read as each thing it
//! could be, in turn, and the first that resolves wins:
//!
//! - a path: `src/db.rs`, `src/index/`, `db.rs`, `src/db.rs:12`;
//! - a command line: `lattice build`, `lattice doctor --model`, its first
//!   word one of the repository's programs or one of their subcommands
//!   (`doctor --model`), each subcommand followed from the one before;
//! - a flag: `--model`, `-p`;
//! - a config key: `[sessions] stop_idle_after`, `sessions.stop_idle_after`,
//!   `[[profile]]`, each table followed from the field before it by the
//!   field's type;
//! - a name: `Session`, `Session::stop()`, `fn stop`, `engine.Server`,
//!   `(*Server).Serve`, `out!`, matching a definition whose name and what
//!   it's in end the same way;
//! - a value a constant holds: `LATTICE_NO_DOWNLOAD`, `wiki.json`.
//!
//! Several definitions it could be are narrowed down, a step at a time: to
//! those of the kind the span says (`()`, `!`, `struct`), and for a
//! capitalized name to types; to a method over a field of its name in the
//! same type, as a getter has; to those in the files the prose is about,
//! those that aren't tests', definitions over declarations; to the one
//! those files refer to, as a SCIP indexer says; and to a type in the file
//! named after it. A subcommand is a top-level one unless the words before
//! it say which it's under, and a config key a serde field's over another
//! field's of its name. What's still more than one is left unlinked.

use super::{Def, DefKind, Index, Lookup, grammar};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// What a span might be.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Query {
    Path {
        path: String,
        dir: bool,
        lines: Option<(u32, u32)>,
    },
    Command {
        words: Vec<String>,
        flags: Vec<String>,
    },
    Flag(String),
    /// A config key, its tables first; `table` when it's a table itself,
    /// `[sessions]` and `[[profile]]`.
    Key {
        keys: Vec<String>,
        table: bool,
    },
    Name {
        segments: Vec<String>,
        hint: Option<Hint>,
    },
    /// What a constant holds: an environment variable's name, a file's.
    Value(String),
}

/// What a span says of the kind of what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hint {
    /// `stop()`, `fn stop`.
    Callable,
    /// `out!`.
    Macro,
    /// `struct Session`, `mod tui`.
    Kind(DefKind),
}

impl Hint {
    fn fits(self, kind: DefKind) -> bool {
        match self {
            Hint::Callable => kind.is_callable(),
            Hint::Macro => kind == DefKind::Macro,
            Hint::Kind(DefKind::Class) => kind.is_type(),
            Hint::Kind(DefKind::Type) => kind.is_type(),
            Hint::Kind(DefKind::Function) => matches!(kind, DefKind::Function | DefKind::Method),
            Hint::Kind(DefKind::Const) => {
                matches!(kind, DefKind::Const | DefKind::Static | DefKind::Variable)
            }
            Hint::Kind(want) => kind == want,
        }
    }
}

pub fn lookup(index: &Index, span: &str, near: &[String]) -> Lookup {
    for query in queries(span, &index.programs, &index.roots) {
        let found = match &query {
            Query::Path { path, dir, lines } => self::path(index, path, *dir, *lines, near),
            Query::Command { words, flags } => command(index, words, flags, near),
            Query::Flag(flag) => self::flag(index, flag, None, near),
            Query::Key { keys, table } => key(index, keys, *table, near),
            Query::Name { segments, hint } => {
                let dotted = span.contains('.') && !span.contains("::");
                name(index, segments, *hint, dotted, near)
            }
            Query::Value(value) => self::value(index, value, near),
        };
        if found != Lookup::Missing {
            return found;
        }
    }
    Lookup::Missing
}

/// What `span` might be, in the order it's tried, `programs` being what
/// the repository's command lines are called and `roots` their
/// subcommands' words.
fn queries(span: &str, programs: &HashSet<String>, roots: &HashSet<String>) -> Vec<Query> {
    let mut span = span.trim();
    let mut quoted = false;
    if let Some(inner) = (span
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"')))
    .or_else(|| {
        span.strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix('\''))
    }) {
        span = inner.trim();
        quoted = true;
    }
    let span = span
        .strip_prefix("$ ")
        .unwrap_or(span)
        .trim_end_matches([',', ';']);
    if span.is_empty() || span.len() > 200 {
        return Vec::new();
    }
    let mut queries = Vec::new();
    if let Some((keys, table)) = table_key(span) {
        queries.push(Query::Key { keys, table });
        return queries;
    }
    if span.contains(char::is_whitespace) {
        let words: Vec<&str> = span.split_whitespace().collect();
        if programs.contains(words[0]) {
            queries.push(command_query(&words[1..]));
        } else if roots.contains(words[0]) && words.iter().all(|word| COMMAND_WORD.is_match(word)) {
            // Without the program before it, only what reads as nothing
            // but a command line is one.
            queries.push(command_query(&words));
        }
        if let Some((key, _)) = span.split_once('=')
            && let Some(keys) = dotted_key(key.trim())
        {
            queries.push(Query::Key { keys, table: false });
        }
        queries.extend(name_query(span));
        return queries;
    }
    if span.starts_with('-') {
        let flag = span.split('=').next().unwrap_or(span);
        if FLAG.is_match(flag) {
            queries.push(Query::Flag(flag.to_string()));
        }
        return queries;
    }
    if let Some(path) = path_query(span) {
        queries.push(path);
    }
    // A quoted word is a string, a value rather than a name.
    if quoted {
        queries.extend(value_query(span));
        return queries;
    }
    queries.extend(name_query(span));
    // Ruby's and Elixir's names end in `!` or `?`: `save!`, `valid?`.
    if let Some(name) = span.strip_suffix(['!', '?'])
        && let Some(Query::Name { mut segments, .. }) = name_query(name)
        && let Some(last) = segments.last_mut()
    {
        last.push_str(&span[name.len()..]);
        queries.push(Query::Name {
            segments,
            hint: None,
        });
    }
    // A word alone is a key by the name it goes by, above, but for one
    // with a dash in it, which no name has: `sigstore-staging`.
    if let Some(keys) = dotted_key(span).filter(|keys| keys.len() > 1 || span.contains('-')) {
        queries.push(Query::Key { keys, table: false });
    }
    queries.extend(value_query(span));
    queries
}

/// What a constant would hold if `span` names one by its value:
/// `$XDG_CACHE_HOME` and `${XDG_CACHE_HOME}` are `XDG_CACHE_HOME`.
fn value_query(span: &str) -> Option<Query> {
    let value = span.trim_start_matches('$');
    let value = (value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}')))
    .unwrap_or(value);
    grammar::names_a_value(value).then(|| Query::Value(value.to_string()))
}

/// A word a command line written in prose has: a subcommand, a flag, a
/// `<placeholder>` or a quoted argument.
static COMMAND_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:[a-z][a-z0-9-]*|--?[A-Za-z0-9][\w-]*(?:=\S*)?|<[^>]*>|"[^"]*"|'[^']*'|…)$"#)
        .expect("a valid regex")
});

static FLAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^--?[A-Za-z0-9][\w-]*$").expect("a valid regex"));

/// `[section] key`, `[section.table] key = value`, `[[section]]`: the keys,
/// and whether the last is a table's.
fn table_key(span: &str) -> Option<(Vec<String>, bool)> {
    static TABLE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\[\[?\s*([\w.-]+)\s*\]\]?(?:\s+([\w.-]+)(?:\s*=.*)?)?$")
            .expect("a valid regex")
    });
    let found = TABLE.captures(span)?;
    let mut keys: Vec<String> = found[1].split('.').map(String::from).collect();
    let table = found.get(2).is_none();
    if let Some(key) = found.get(2) {
        keys.extend(key.as_str().split('.').map(String::from));
    }
    Some((keys, table)).filter(|(keys, _)| keys.iter().all(|key| !key.is_empty()))
}
/// `sessions.stop_idle_after`: lowercase words joined by dots, as a config
/// file's keys are written.
fn dotted_key(span: &str) -> Option<Vec<String>> {
    static DOTTED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[a-z_][a-z0-9_-]*(?:\.[a-z_][a-z0-9_-]*)*$").expect("a valid regex")
    });
    DOTTED
        .is_match(span)
        .then(|| span.split('.').map(String::from).collect())
}

/// A command line's subcommands and flags, the words after its program, up
/// to a shell's `|`, `;` or `&&`.
fn command_query(words: &[&str]) -> Query {
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").expect("a valid regex"));
    let mut subcommands = Vec::new();
    let mut flags = Vec::new();
    let end = (words.iter())
        .position(|word| matches!(*word, "|" | "||" | "&&" | ";" | ">" | ">>" | "2>&1"))
        .unwrap_or(words.len());
    let mut words = words[..end].iter();
    for word in words.by_ref() {
        if WORD.is_match(word) {
            subcommands.push(word.to_string());
        } else {
            if word.starts_with('-') {
                flags.push(word.split('=').next().unwrap_or(word).to_string());
            }
            break;
        }
    }
    flags.extend(
        words
            .filter(|word| word.starts_with('-'))
            .map(|word| word.split('=').next().unwrap_or(word).to_string())
            .filter(|flag| FLAG.is_match(flag)),
    );
    Query::Command {
        words: subcommands,
        flags,
    }
}

/// A path, with a line or two after it: `src/x.rs`, `src/tui/`,
/// `src/x.rs:12`, `src/x.rs:12:5`, `src/x.rs#L12-L20`, `daemon.rs`.
fn path_query(span: &str) -> Option<Query> {
    static LINES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(.+?)(?::(\d+)(?::\d+|-(\d+))?|#L(\d+)(?:-L(\d+))?|\((\d+),\s*\d+\))$")
            .expect("a valid regex")
    });
    static EXTENSION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\.[A-Za-z][\w+-]{0,9}$").expect("a valid regex"));
    let (path, lines) = match LINES.captures(span) {
        Some(found) => {
            let number = |at: usize| {
                found
                    .get(at)
                    .and_then(|number| number.as_str().parse::<u32>().ok())
            };
            let start = number(2).or(number(4)).or(number(6));
            let end = number(3).or(number(5)).or(start);
            (
                found.get(1).map_or(span, |path| path.as_str()),
                start.zip(end),
            )
        }
        None => (span, None),
    };
    let path = path.strip_prefix("./").unwrap_or(path);
    let looks_like_one = path.contains('/') || EXTENSION.is_match(path) || lines.is_some();
    if !looks_like_one || path.contains("::") || path.contains(['*', '?', '<', '>', '{', '$', '"'])
    {
        return None;
    }
    let dir = path.ends_with('/');
    let path = path.trim_end_matches('/').to_string();
    (!path.is_empty()).then_some(Query::Path { path, dir, lines })
}

/// A name, from a span that names one in the ways code is written about:
/// `Session::stop()`, `pub fn stop`, `&mut Session`, `self.sessions`,
/// `(*Server).Serve`, `out!`, `Vec<Session>` (which names `Vec`).
fn name_query(span: &str) -> Option<Query> {
    static KEYWORD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:pub(?:\([\w\s]+\))?\s+|export\s+|async\s+|unsafe\s+|const\s+(?:fn\s)|default\s+)*(fn|func|def|function|struct|enum|trait|impl|class|interface|type|mod|module|namespace|package|const|static|let|var|macro_rules!)\s+(.+)$")
            .expect("a valid regex")
    });
    static GO_METHOD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\(\s*(?:\w+\s+)?\*?\s*(\w+)(?:\[[^\]]*\])?\s*\)\s*\.?\s*(\w+)")
            .expect("a valid regex")
    });
    static IDENTIFIER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_$][A-Za-z0-9_$]*$").expect("a valid regex"));
    let mut text = span.trim();
    let mut hint = None;
    if let Some(found) = KEYWORD.captures(text) {
        hint = Some(match &found[1] {
            "fn" | "func" | "def" | "function" => Hint::Callable,
            "struct" => Hint::Kind(DefKind::Struct),
            "enum" => Hint::Kind(DefKind::Enum),
            "trait" => Hint::Kind(DefKind::Trait),
            "interface" => Hint::Kind(DefKind::Interface),
            "class" | "impl" | "type" => Hint::Kind(DefKind::Type),
            "mod" | "module" | "namespace" | "package" => Hint::Kind(DefKind::Module),
            "macro_rules!" => Hint::Macro,
            _ => Hint::Kind(DefKind::Const),
        });
        text = found.get(2).map_or("", |rest| rest.as_str()).trim();
        if &found[1] == "impl" && text.contains(" for ") {
            return None;
        }
    }
    // Go: `func (s *Server) Serve(...)` and `(*Server).Serve`.
    let receiver;
    if let Some(found) = GO_METHOD.captures(text) {
        receiver = format!("{}.{}", &found[1], &found[2]);
        text = &receiver;
        hint = hint.or(Some(Hint::Callable));
    }
    // A call's arguments and a type's parameters: `stop(&mut self)`,
    // `Vec<Session>`; a variable's type: `port: u16`.
    if let Some(at) = text.find('(') {
        if text[at..].trim_end().ends_with(')') || !text[at..].contains(')') {
            hint = hint.or(Some(Hint::Callable));
        }
        text = &text[..at];
    }
    if let Some(at) = text
        .find(['<', '[', ':'])
        .filter(|&at| !text[at..].starts_with("::"))
    {
        text = &text[..at];
    }
    let mut text = text.trim().trim_start_matches(['&', '*', '@']).trim();
    text = text.strip_prefix("mut ").unwrap_or(text).trim();
    if let Some(rest) = text.strip_suffix('!') {
        text = rest;
        hint = Some(Hint::Macro);
    }
    let text = text.trim_end_matches('?');
    for prefix in [
        "crate::", "self::", "super::", "Self::", "self.", "this.", "$this->", "::",
    ] {
        if let Some(rest) = text.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return name_query_of(rest, hint, &IDENTIFIER);
        }
    }
    name_query_of(text, hint, &IDENTIFIER)
}

fn name_query_of(text: &str, hint: Option<Hint>, identifier: &Regex) -> Option<Query> {
    let segments: Vec<String> = text
        .split("::")
        .flat_map(|part| part.split("->"))
        .flat_map(|part| part.split(['.', '#', '\\']))
        .map(String::from)
        .collect();
    let named = !segments.is_empty() && segments.iter().all(|segment| identifier.is_match(segment));
    named.then_some(Query::Name { segments, hint })
}

/// The definitions of `ids` by their places.
fn defs(index: &Index, ids: &[u32]) -> Vec<Def> {
    ids.iter()
        .map(|&id| index.defs[id as usize].clone())
        .collect()
}

/// The one of `ids` the span names, narrowed down as the module says, or
/// those it could be. `types` says the span is a capitalized name, which
/// names a type over what else is called so.
fn choose(index: &Index, ids: &[u32], near: &[String], types: bool) -> Lookup {
    let mut ids = distinct(index, ids);
    if ids.is_empty() {
        return Lookup::Missing;
    }
    let alone = ids.len() == 1;
    let test = |id: u32| index.meta[id as usize].test;
    let kind = |id: u32| index.defs[id as usize].kind;
    if types {
        narrow(&mut ids, |id| kind(id).is_type() && !test(id));
    }
    let near: HashSet<&str> = near.iter().map(String::as_str).collect();
    let is_near = |id: u32| near.contains(index.defs[id as usize].path.as_str());
    narrow(&mut ids, is_near);
    narrow(&mut ids, |id| !test(id));
    narrow(&mut ids, |id| !index.meta[id as usize].decl);
    // A getter, over the field it gets.
    let methods: HashSet<&[String]> = (ids.iter().copied())
        .filter(|&id| kind(id) == DefKind::Method)
        .map(|id| owner(index, id))
        .collect();
    narrow(&mut ids, |id| {
        kind(id) != DefKind::Field || !methods.contains(owner(index, id))
    });
    if ids.len() > 1 {
        let referred: Vec<u32> = (ids.iter().copied())
            .filter(|id| {
                near.iter()
                    .any(|path| index.refs.get(*path).is_some_and(|refs| refs.contains(id)))
            })
            .collect();
        if referred.len() == 1 {
            ids = referred;
        }
    }
    narrow(&mut ids, |id| {
        named_after_its_file(&index.defs[id as usize])
    });
    match ids.as_slice() {
        // A test's is meant only in prose about it, or, when nothing else
        // is called so, a test function by its name: a test's types and
        // constants stand in for the real ones.
        [id] if test(*id)
            && !is_near(*id)
            && !(alone && matches!(kind(*id), DefKind::Function | DefKind::Method)) =>
        {
            Lookup::Ambiguous(defs(index, &ids))
        }
        [id] => Lookup::Unique(index.defs[*id as usize].clone()),
        _ => Lookup::Ambiguous(defs(index, &ids)),
    }
}

/// Whether `def` is a type in the file named after it, as `viewer.rs` is
/// `Viewer`'s, or `split-tree.ts` `SplitTree`'s: the type the file is
/// about.
fn named_after_its_file(def: &Def) -> bool {
    let plain = |text: &str| -> String {
        (text.chars())
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let name = def.path.rsplit('/').next().unwrap_or(&def.path);
    let stem = name.split('.').next().unwrap_or(name);
    def.kind.is_type() && plain(stem) == plain(&def.name)
}

/// Keeps those of `ids` that `keep` says, unless that's none of them.
fn narrow(ids: &mut Vec<u32>, keep: impl Fn(u32) -> bool) {
    if ids.len() < 2 {
        return;
    }
    let kept: Vec<u32> = ids.iter().copied().filter(|&id| keep(id)).collect();
    if !kept.is_empty() {
        *ids = kept;
    }
}

/// `ids` with each definition once: two of the same kind and name in the
/// same lines of the same file, as two tiers can find, are one, the precise
/// one kept.
fn distinct(index: &Index, ids: &[u32]) -> Vec<u32> {
    let mut sorted: Vec<u32> = ids.to_vec();
    sorted.sort_by_key(|&id| (!index.defs[id as usize].precise, id));
    sorted.dedup();
    let mut kept: Vec<u32> = Vec::new();
    for id in sorted {
        let def = &index.defs[id as usize];
        let same = kept.iter().any(|&other| {
            let other = &index.defs[other as usize];
            other.path == def.path
                && other.name == def.name
                && other.kind == def.kind
                && other.start <= def.end.max(def.start)
                && def.start <= other.end.max(other.start)
        });
        if !same {
            kept.push(id);
        }
    }
    kept.sort_unstable();
    kept
}

fn path(
    index: &Index,
    path: &str,
    dir: bool,
    lines: Option<(u32, u32)>,
    near: &[String],
) -> Lookup {
    let file = |path: &str| {
        let (start, end) = match (lines, index.lines.get(path)) {
            (Some((start, end)), Some(&count)) if start >= 1 && end >= start && end <= count => {
                (start, end)
            }
            (Some((start, end)), None) if start >= 1 && end >= start => (start, end),
            _ => (0, 0),
        };
        Def {
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            qualified: path.to_string(),
            kind: DefKind::File,
            path: path.to_string(),
            start,
            end,
            precise: true,
        }
    };
    let directory = |path: &str| Def {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        qualified: format!("{path}/"),
        kind: DefKind::Directory,
        path: path.to_string(),
        start: 0,
        end: 0,
        precise: true,
    };
    if !dir && index.files.contains(path) {
        return Lookup::Unique(file(path));
    }
    if index.dirs.contains(path) {
        return Lookup::Unique(directory(path));
    }
    // A path's end: `daemon.rs`, `tui/app.rs`, `tui/`.
    let ending = format!("/{path}");
    let mut found: Vec<Def> = Vec::new();
    if !dir {
        found.extend(
            index
                .files
                .iter()
                .filter(|file| file.ends_with(&ending))
                .map(|path| file(path)),
        );
    }
    if found.is_empty() {
        found.extend(
            index
                .dirs
                .iter()
                .filter(|dir| dir.ends_with(&ending))
                .map(|path| directory(path)),
        );
    }
    if found.len() > 1 {
        let in_near: Vec<Def> = found
            .iter()
            .filter(|def| {
                near.iter()
                    .any(|near| *near == def.path || near.starts_with(&format!("{}/", def.path)))
            })
            .cloned()
            .collect();
        if !in_near.is_empty() {
            found = in_near;
        }
    }
    match found.len() {
        0 => Lookup::Missing,
        1 => Lookup::Unique(found.remove(0)),
        _ => Lookup::Ambiguous(found),
    }
}

/// The definitions named `name` of one of `kinds`, by their own names or the
/// others they go by.
fn named(index: &Index, name: &str, kinds: &[DefKind]) -> Vec<u32> {
    let mut ids: Vec<u32> = (index.by_alias.get(name).into_iter().flatten())
        .chain(index.by_name.get(name).into_iter().flatten())
        .copied()
        .filter(|&id| kinds.contains(&index.defs[id as usize].kind))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Whether the definition `id` goes by `alias`, beside its own name.
fn goes_by(index: &Index, id: u32, alias: &str) -> bool {
    index
        .by_alias
        .get(alias)
        .is_some_and(|ids| ids.contains(&id))
}

/// What the name `segments` is, `dotted` when the span wrote them with
/// dots.
fn name(
    index: &Index,
    segments: &[String],
    hint: Option<Hint>,
    dotted: bool,
    near: &[String],
) -> Lookup {
    let Some(last) = segments.last() else {
        return Lookup::Missing;
    };
    let mut ids: Vec<u32> = (index.by_name.get(last).into_iter().flatten())
        .copied()
        .filter(|&id| index.meta[id as usize].segments.ends_with(segments))
        .collect();
    // A word alone may be the key a field goes by: `worker_count`.
    if segments.len() == 1 {
        let keys = (index.by_alias.get(last).into_iter().flatten()).copied();
        ids.extend(keys.filter(|&id| index.defs[id as usize].kind == DefKind::Field));
        ids.sort_unstable();
        ids.dedup();
    }
    // What the span says it is, it is.
    if let Some(hint) = hint {
        ids.retain(|&id| hint.fits(index.defs[id as usize].kind));
    }
    // A language that writes `session::ended` reaches only members with
    // dots, `Session.stop`; `session.ended` is something else.
    if dotted && segments.len() > 1 {
        ids.retain(|&id| {
            let def = &index.defs[id as usize];
            !def.qualified.contains("::")
                || matches!(
                    def.kind,
                    DefKind::Field | DefKind::Method | DefKind::Variant
                )
        });
    }
    let types =
        hint.is_none() && segments.len() == 1 && last.starts_with(|c: char| c.is_uppercase());
    let found = choose(index, &ids, near, types);
    match &found {
        Lookup::Unique(def) if segments.len() == 1 && !near.contains(&def.path) => {
            let keyed =
                (ids.iter()).any(|&id| index.defs[id as usize] == *def && goes_by(index, id, last));
            match bare_word_means_it(last, def, keyed) {
                true => found,
                false => Lookup::Missing,
            }
        }
        _ => found,
    }
}

/// Whether the bare word `word`, in prose about other files, means `def`,
/// its one definition, `keyed` when it's the key a field goes by. A field or a
/// variable somewhere is most often something else in prose (`all`,
/// `latest`, `IMAGE`), so it's meant only as a key. And in Go, where every
/// field of a type the program reads or writes has a tag and a name
/// starting lowercase is its package's own, one that reads as a word
/// (`external`, `gid`), not as code writes names (`confirmBooking`), is the
/// word.
fn bare_word_means_it(word: &str, def: &Def, keyed: bool) -> bool {
    let plain = word
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let go_word = def.path.ends_with(".go") && plain;
    match def.kind {
        DefKind::Field | DefKind::Variable => keyed && !go_word,
        DefKind::File | DefKind::Directory | DefKind::Module | DefKind::Command => true,
        _ => !go_word,
    }
}

/// What a constant that holds `value` is: `NO_DOWNLOAD` for
/// `LATTICE_NO_DOWNLOAD`.
fn value(index: &Index, value: &str, near: &[String]) -> Lookup {
    let ids = named(
        index,
        value,
        &[
            DefKind::Const,
            DefKind::Static,
            DefKind::Variable,
            DefKind::Macro,
            DefKind::Field,
        ],
    );
    let ids: Vec<u32> = (ids.into_iter())
        .filter(|&id| goes_by(index, id, value))
        .collect();
    choose(index, &ids, near, false)
}

/// What the definition `id` is in, its segments but its own.
fn owner(index: &Index, id: u32) -> &[String] {
    let segments = &index.meta[id as usize].segments;
    &segments[..segments.len().saturating_sub(1)]
}

/// What `name` names in `ty`: whether `ty`, a type as written, has `name`
/// as a word of it.
fn mentions(ty: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    ty.match_indices(name).any(|(at, _)| {
        let before = ty[..at].chars().next_back();
        let after = ty[at + name.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

/// The words of a type as written: `Option<Vec<KeyCommand>>` has `Option`,
/// `Vec` and `KeyCommand`.
pub fn words(ty: &str) -> impl Iterator<Item = &str> {
    ty.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
}

/// What the definition `id` is in: the struct a field is a field of, the
/// enum a variant is of.
fn container(index: &Index, id: u32) -> Option<&str> {
    owner(index, id).last().map(String::as_str)
}

/// Whether the field `id`'s type is a table of its own: a struct with
/// fields, a list of them, or a map.
fn is_table(index: &Index, id: u32) -> bool {
    const MAPS: &[&str] = &[
        "HashMap", "BTreeMap", "IndexMap", "Map", "map", "dict", "Dict", "Record", "Mapping",
    ];
    let Some(ty) = index.meta[id as usize].ty.as_deref() else {
        return false;
    };
    words(ty).any(|word| {
        MAPS.contains(&word)
            || (index.members.get(word).into_iter().flatten())
                .any(|&member| index.defs[member as usize].kind == DefKind::Field)
    })
}

/// Keeps those of `ids` each of `parents` leads to, the words before
/// theirs, from the last back, each word what `named` says it could be and
/// `leads` saying whether one leads to the next: a key's tables, a
/// subcommand's commands. One that can't be followed is left out.
fn follow(
    ids: &mut Vec<u32>,
    parents: &[String],
    named: impl Fn(&str) -> Vec<u32>,
    leads: impl Fn(u32, u32) -> bool,
) {
    ids.retain(|&id| {
        let mut inner = id;
        for parent in parents.iter().rev() {
            match (named(parent).into_iter()).find(|&outer| leads(outer, inner)) {
                Some(outer) => inner = outer,
                None => return false,
            }
        }
        true
    });
}

/// Whether the config key `field`'s table is the field `table`: its type
/// is the struct `field` is a field of.
fn holds_key(index: &Index, table: u32, field: u32) -> bool {
    let ty = index.meta[table as usize].ty.as_deref();
    container(index, field).is_some_and(|inner| ty.is_some_and(|ty| mentions(ty, inner)))
}

fn key(index: &Index, keys: &[String], table: bool, near: &[String]) -> Lookup {
    let Some((last, parents)) = keys.split_last() else {
        return Lookup::Missing;
    };
    // A key is a field that goes by it: a serde type's, by its key; a Go
    // struct's, by its tag.
    let keys = |key: &str| -> Vec<u32> {
        (index.by_alias.get(key).into_iter().flatten())
            .copied()
            .filter(|&id| index.defs[id as usize].kind == DefKind::Field)
            .collect()
    };
    let mut ids = keys(last);
    if table {
        ids.retain(|&id| is_table(index, id));
    }
    follow(&mut ids, parents, keys, |table, field| {
        holds_key(index, table, field)
    });
    choose(index, &ids, near, false)
}

const COMMANDS: &[DefKind] = &[DefKind::Variant, DefKind::Command];

/// Whether the subcommand `inner` is under the command `outer`: a clap
/// variant holding the enum `inner` is a variant of, in its own type, its
/// fields' or those of the struct it holds; a cobra command `inner` is
/// added to.
fn holds_command(index: &Index, outer: u32, inner: u32) -> bool {
    let (outer_def, inner_def) = (&index.defs[outer as usize], &index.defs[inner as usize]);
    if inner_def.kind == DefKind::Command {
        return outer_def.kind == DefKind::Command && index.meta[inner as usize].of == Some(outer);
    }
    let Some(inner) = container(index, inner) else {
        return false;
    };
    index.held_by(outer).any(|ty| mentions(ty, inner))
}

fn command(index: &Index, words: &[String], flags: &[String], near: &[String]) -> Lookup {
    let Some((last, parents)) = words.split_last() else {
        return match flags.first() {
            Some(flag) => self::flag(index, flag, None, near),
            None => Lookup::Missing,
        };
    };
    // A subcommand is a variant only where it goes by its word.
    let mut ids: Vec<u32> = (index.by_alias.get(last.as_str()).into_iter().flatten())
        .copied()
        .filter(|&id| COMMANDS.contains(&index.defs[id as usize].kind))
        .collect();
    match parents.is_empty() {
        true => narrow(&mut ids, |id| !index.is_nested(id)),
        false => follow(
            &mut ids,
            parents,
            |word| named(index, word, COMMANDS),
            |outer, inner| holds_command(index, outer, inner),
        ),
    }
    let found = choose(index, &ids, near, false);
    match (&found, flags.first()) {
        (Lookup::Unique(def), Some(flag)) => {
            let at = ids
                .iter()
                .copied()
                .find(|&id| index.defs[id as usize] == *def);
            match self::flag(index, flag, at, near) {
                Lookup::Unique(flag) => Lookup::Unique(flag),
                // The command has no such flag: it isn't the one meant.
                Lookup::Missing => Lookup::Missing,
                Lookup::Ambiguous(_) => found,
            }
        }
        _ => found,
    }
}

/// The flag `flag`, of the subcommand `of` when it's known: a field of the
/// variant, or of the struct the variant holds; a cobra command's own.
fn flag(index: &Index, flag: &str, of: Option<u32>, near: &[String]) -> Lookup {
    let mut ids: Vec<u32> = (index.by_alias.get(flag).into_iter().flatten())
        .copied()
        .filter(|&id| matches!(index.defs[id as usize].kind, DefKind::Field | DefKind::Flag))
        .collect();
    if let Some(of) = of {
        let command = &index.meta[of as usize];
        let held: Vec<&str> = index.held_by(of).collect();
        ids.retain(|&id| {
            let meta = &index.meta[id as usize];
            match index.defs[id as usize].kind {
                DefKind::Flag => meta.of == Some(of),
                _ => {
                    owner(index, id) == &command.segments[..]
                        || container(index, id)
                            .is_some_and(|inner| held.iter().any(|ty| mentions(ty, inner)))
                }
            }
        });
    }
    choose(index, &ids, near, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(span: &str) -> Vec<Query> {
        let programs = HashSet::from(["lattice".to_string()]);
        let roots = HashSet::from(["build".to_string(), "doctor".to_string()]);
        queries(span, &programs, &roots)
    }

    fn name(segments: &[&str], hint: Option<Hint>) -> Query {
        Query::Name {
            segments: segments.iter().map(|segment| segment.to_string()).collect(),
            hint,
        }
    }

    fn key(keys: &[&str], table: bool) -> Query {
        Query::Key {
            keys: keys.iter().map(|key| key.to_string()).collect(),
            table,
        }
    }

    fn path(path: &str, dir: bool, lines: Option<(u32, u32)>) -> Query {
        Query::Path {
            path: path.into(),
            dir,
            lines,
        }
    }

    fn command(words: &[&str], flags: &[&str]) -> Query {
        Query::Command {
            words: words.iter().map(|word| word.to_string()).collect(),
            flags: flags.iter().map(|flag| flag.to_string()).collect(),
        }
    }

    #[test]
    fn a_name_is_read_out_of_the_ways_code_is_written_about() {
        assert_eq!(read("Session::stop"), [name(&["Session", "stop"], None)]);
        assert_eq!(
            read("Session::stop()"),
            [name(&["Session", "stop"], Some(Hint::Callable))]
        );
        assert_eq!(
            read("pub fn stop(&mut self) -> Result<()>"),
            [name(&["stop"], Some(Hint::Callable))]
        );
        assert_eq!(
            read("struct Session"),
            [name(&["Session"], Some(Hint::Kind(DefKind::Struct)))]
        );
        assert_eq!(
            read("out!"),
            [name(&["out"], Some(Hint::Macro)), name(&["out!"], None)]
        );
        assert_eq!(read("valid?")[1], name(&["valid?"], None));
        assert_eq!(
            read("crate::session::Session"),
            [name(&["session", "Session"], None)]
        );
        assert_eq!(read("&mut Session"), [name(&["Session"], None)]);
        assert_eq!(read("Vec<Session>"), [name(&["Vec"], None)]);
        assert!(read("self.sessions").contains(&name(&["sessions"], None)));
        assert_eq!(
            read("(*Server).Serve"),
            [name(&["Server", "Serve"], Some(Hint::Callable))]
        );
        assert_eq!(
            read("func (s *Server) Serve(w Writer)"),
            [name(&["Server", "Serve"], Some(Hint::Callable))]
        );
        assert_eq!(read("port: u16"), [name(&["port"], None)]);
        assert!(read("impl Display for Kind").is_empty());
        assert!(read("a + b").is_empty());
        assert!(read("").is_empty());
    }

    #[test]
    fn a_path_is_read_with_the_lines_after_it() {
        assert_eq!(read("src/db.rs"), [path("src/db.rs", false, None)]);
        assert_eq!(
            read("src/db.rs:12"),
            [path("src/db.rs", false, Some((12, 12)))]
        );
        assert_eq!(
            read("src/x.rs#L3-L9")[0],
            path("src/x.rs", false, Some((3, 9)))
        );
        assert_eq!(read("src/index/"), [path("src/index", true, None)]);
        assert_eq!(
            read("config.toml"),
            [
                path("config.toml", false, None),
                name(&["config", "toml"], None),
                key(&["config", "toml"], false),
                Query::Value("config.toml".into()),
            ]
        );
    }

    #[test]
    fn keys_flags_commands_and_values_are_read_as_they_re_typed() {
        assert_eq!(
            read("[sessions] stop_idle_after"),
            [key(&["sessions", "stop_idle_after"], false)]
        );
        assert_eq!(read("[[profile]]"), [key(&["profile"], true)]);
        assert_eq!(read("[keys.command]"), [key(&["keys", "command"], true)]);
        assert_eq!(
            read("[memory] embedder = \"gemini\""),
            [key(&["memory", "embedder"], false)]
        );
        assert_eq!(read("--wait"), [Query::Flag("--wait".into())]);
        assert_eq!(
            read("--test-threads=4"),
            [Query::Flag("--test-threads".into())]
        );
        assert_eq!(
            read("lattice doctor --model <name>"),
            [command(&["doctor"], &["--model"])]
        );
        assert_eq!(read("$ lattice build"), [command(&["build"], &[])]);
        // A top-level subcommand with no program before it.
        assert_eq!(
            read("doctor --model")[0],
            command(&["doctor"], &["--model"])
        );
        assert!(read("cargo test -- --test-threads=4").is_empty());
        assert_eq!(
            read("$LATTICE_NO_DOWNLOAD").last(),
            Some(&Query::Value("LATTICE_NO_DOWNLOAD".into()))
        );
        assert!(!read("sonnet").contains(&Query::Value("sonnet".into())));
        // A quoted word is a value, and a quoted path still a path.
        assert!(read("\"other\"").is_empty());
        assert_eq!(
            read("\"wiki.json\"").last(),
            Some(&Query::Value("wiki.json".into()))
        );
        assert_eq!(read("'src/db.rs'"), [path("src/db.rs", false, None)]);
    }

    #[test]
    fn a_word_is_mentioned_only_whole() {
        assert!(mentions("Option<SessionSettings>", "SessionSettings"));
        assert!(mentions("SessionSettings", "SessionSettings"));
        assert!(!mentions("Option<SessionSettingsX>", "SessionSettings"));
        assert!(!mentions("MySessionSettings", "SessionSettings"));
        let words: Vec<&str> = words("Option<Vec<KeyCommand>>").collect();
        assert_eq!(words, ["Option", "Vec", "KeyCommand"]);
    }

    #[test]
    fn a_type_named_like_its_file_is_the_file_s() {
        let def = |name: &str, kind, path: &str| Def {
            name: name.into(),
            qualified: name.into(),
            kind,
            path: path.into(),
            start: 1,
            end: 2,
            precise: false,
        };
        assert!(named_after_its_file(&def(
            "Viewer",
            DefKind::Struct,
            "src/viewer.rs"
        )));
        assert!(named_after_its_file(&def(
            "SplitTree",
            DefKind::Class,
            "web/split-tree.ts"
        )));
        assert!(named_after_its_file(&def(
            "SplitTree",
            DefKind::Enum,
            "src/split_tree.rs"
        )));
        assert!(!named_after_its_file(&def(
            "Viewer",
            DefKind::Struct,
            "src/session.rs"
        )));
        assert!(!named_after_its_file(&def(
            "viewer",
            DefKind::Function,
            "src/viewer.rs"
        )));
    }
}
