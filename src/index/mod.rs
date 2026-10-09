//! The symbol index: where each thing a wiki's prose names is defined, so
//! that a code span like `Session::stop`, `[sessions] stop_idle_after`,
//! `lattice build` or `src/db.rs` links to its file and its lines at the
//! wiki's commit. Code Wiki gets this from Kythe, which hooks into a
//! project's build; lattice gets it from three tiers, each reaching where
//! the one above it can't:
//!
//! 1. Precise ([`precise`]): a SCIP indexer, the compiler's own view of
//!    every definition and every reference, run for each language the
//!    repository has whose indexer is installed: `rust-analyzer scip`,
//!    scip-go, scip-typescript, scip-python, scip-java and scip-clang. Each
//!    is held to a time and a memory, and one that isn't there, fails or
//!    goes past them leaves its language to the tier below, which
//!    [`Index::report`] says.
//! 2. Syntactic ([`grammar`]): tree-sitter's grammars, compiled in, read
//!    every file for its definitions, each with its kind, its exact lines
//!    and what it's in, so `Session::stop` is a name too, and a config
//!    struct's field its key, a clap variant its subcommand. A language
//!    with no grammar compiled in is read by its keywords ([`keywords`]):
//!    first lines exact, last lines by indentation.
//! 3. Paths: every file git tracks at the commit, and their directories.
//!
//! [`Index::lookup`] resolves a span to the one definition it names, or
//! says it can't tell rather than guess ([`lookup`]): a wrong link is worse
//! than none.
//!
//! Everything is read from git at the commit, not from the working tree,
//! and what's read is kept by its blob in the cache directory the index is
//! given ([`cache`]), so building it again reads only what changed, and an
//! indexer runs again only once a file of its languages has.
//!
//! Adapted from crystal's `src/wiki/index/` (MIT), where it began.

mod cache;
mod files;
mod grammar;
mod keywords;
mod lookup;
mod precise;
mod scip;

use crate::cancel::Cancel;
use crate::glob;
use anyhow::{Result, bail};
use files::{Entry, Lang};
use grammar::{Found, Outline};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

/// Where something is defined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Def {
    /// Its own name: `stop`, `--wait`, or a file's or a directory's.
    pub name: String,
    /// Its name with what it's in, as its language writes it:
    /// `session::Session::stop`, `engine.Server.Serve`; a file's or a
    /// directory's path.
    pub qualified: String,
    pub kind: DefKind,
    /// The file it's in, from the top of the repository.
    pub path: String,
    /// Its first and last lines, from 1; both 0 for what's a whole file,
    /// a file's module, or a directory.
    pub start: u32,
    pub end: u32,
    /// Whether it's certain: a SCIP indexer said it's defined here, or it's
    /// a path. A grammar's definition is right about its lines, but not
    /// about what a name in another file refers to.
    pub precise: bool,
}

impl Def {
    /// Where a `code:` link to it goes: `src/x.rs#L10-L20`, `src/x.rs#L10`
    /// for one line, or `src/x.rs` for a whole file or a directory.
    pub fn target(&self) -> String {
        match (self.start, self.end) {
            (0, _) => self.path.clone(),
            (start, end) if end <= start => format!("{}#L{start}", self.path),
            (start, end) => format!("{}#L{start}-L{end}", self.path),
        }
    }
}

/// What a definition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefKind {
    /// A module, a namespace or a package.
    Module,
    Struct,
    Class,
    Enum,
    Union,
    Trait,
    Interface,
    /// A type alias, or an associated type.
    Type,
    Function,
    Method,
    /// A field of a struct or a class, or a property.
    Field,
    /// An enum's variant or member.
    Variant,
    Const,
    Static,
    Variable,
    Macro,
    /// A subcommand of a command line, where it isn't a variant.
    Command,
    /// A flag of a command line, where it isn't a field.
    Flag,
    File,
    Directory,
}

impl DefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DefKind::Module => "module",
            DefKind::Struct => "struct",
            DefKind::Class => "class",
            DefKind::Enum => "enum",
            DefKind::Union => "union",
            DefKind::Trait => "trait",
            DefKind::Interface => "interface",
            DefKind::Type => "type",
            DefKind::Function => "function",
            DefKind::Method => "method",
            DefKind::Field => "field",
            DefKind::Variant => "variant",
            DefKind::Const => "const",
            DefKind::Static => "static",
            DefKind::Variable => "variable",
            DefKind::Macro => "macro",
            DefKind::Command => "command",
            DefKind::Flag => "flag",
            DefKind::File => "file",
            DefKind::Directory => "directory",
        }
    }

    /// Whether it's a type, which a capitalized name in prose most often
    /// means.
    pub fn is_type(self) -> bool {
        matches!(
            self,
            DefKind::Struct
                | DefKind::Class
                | DefKind::Enum
                | DefKind::Union
                | DefKind::Trait
                | DefKind::Interface
                | DefKind::Type
        )
    }

    /// Whether it can be called, which a span ending in `()` names.
    fn is_callable(self) -> bool {
        matches!(
            self,
            DefKind::Function | DefKind::Method | DefKind::Macro | DefKind::Class | DefKind::Struct
        )
    }
}

impl fmt::Display for DefKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a span names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// One definition, the one to link to.
    Unique(Def),
    /// Several it could be, none of them surely: left unlinked.
    Ambiguous(Vec<Def>),
    /// Nothing in the repository: left unlinked.
    Missing,
}

/// How the index is built: `[index]` in the settings (see
/// [`crate::config`]), and the files every wiki leaves out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndexSettings {
    /// Whether the SCIP indexers installed here run, for precise
    /// definitions and references. Without them, every language is read by
    /// its grammar.
    pub precise: bool,
    /// The most an indexer may run, in seconds, before it's stopped and its
    /// languages left to their grammars.
    pub indexer_timeout_secs: u64,
    /// The most memory an indexer, with every process under it, may take,
    /// in megabytes, before it's stopped.
    pub indexer_memory_mb: u64,
    /// Files bigger than this, in kilobytes, aren't read for definitions,
    /// most often generated or minified; their paths still link.
    pub max_file_kb: u64,
    /// Globs, as `.gitignore` writes them, of the files that link by their
    /// paths only, none of their definitions read: code a project carries
    /// but didn't write.
    pub paths_only: Vec<String>,
    /// Globs of the files left out altogether, which nothing links to: the
    /// settings' `exclude`, which every wiki leaves out.
    #[serde(skip)]
    pub exclude: Vec<String>,
}

impl Default for IndexSettings {
    fn default() -> IndexSettings {
        IndexSettings {
            precise: true,
            indexer_timeout_secs: 900,
            indexer_memory_mb: 8192,
            max_file_kb: 1024,
            paths_only: ["vendor/", "third_party/", "node_modules/", "testdata/"]
                .map(String::from)
                .to_vec(),
            exclude: Vec::new(),
        }
    }
}

/// How a language's definitions were found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// By a SCIP indexer.
    Precise,
    /// By its tree-sitter grammar.
    Syntactic,
    /// By its keywords, with no grammar compiled in.
    Keywords,
    /// Not at all: only its files' paths link.
    Paths,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Precise => "precise",
            Tier::Syntactic => "syntactic",
            Tier::Keywords => "keywords",
            Tier::Paths => "paths",
        }
    }
}

/// How one language of the repository was indexed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanguageReport {
    pub language: String,
    /// Its files that were read for definitions.
    pub files: usize,
    /// The best tier any of them got.
    pub tier: Tier,
    /// How many of them a SCIP indexer covered; the rest went by their
    /// grammar.
    pub precise_files: usize,
    pub definitions: usize,
    /// Why it got no better, or which indexer gave it its tier and how long
    /// it took.
    pub note: Option<String>,
}

/// What a definition carries beside its [`Def`], for looking it up.
#[derive(Debug, Clone, Default)]
struct Meta {
    /// What it's in, then its name, as [`Found::segments`], under its
    /// file's module or package.
    segments: Vec<String>,
    /// The other names it goes by: [`Found::names`].
    names: Vec<String>,
    ty: Option<String>,
    /// The command a cobra command is added to, or a flag given to.
    of: Option<u32>,
    test: bool,
    decl: bool,
}

/// Where every definition of a repository at a commit is, and its files.
#[derive(Debug, Default)]
pub struct Index {
    defs: Vec<Def>,
    meta: Vec<Meta>,
    /// Definitions by their own names.
    by_name: HashMap<String, Vec<u32>>,
    /// Definitions by the other names they go by (keys, flags, words).
    by_alias: HashMap<String, Vec<u32>>,
    /// Each file's definitions, by where they start.
    by_path: HashMap<String, Vec<u32>>,
    /// The fields and the variants of each type, by its name.
    members: HashMap<String, Vec<u32>>,
    /// Every file git tracks, and how many lines a file read has.
    files: BTreeSet<String>,
    lines: HashMap<String, u32>,
    dirs: BTreeSet<String>,
    /// Each file's references, by a SCIP indexer, to definitions here.
    refs: HashMap<String, HashSet<u32>>,
    /// What the repository's command lines are called: `lattice`.
    programs: HashSet<String>,
    /// The types a subcommand holds, its own subcommands' enums among them.
    held: HashSet<String>,
    /// The words of the top-level subcommands: `build`, `doctor`.
    roots: HashSet<String>,
    report: Vec<LanguageReport>,
    took: Duration,
}

impl Index {
    /// Indexes the repository at `root`, its files at `commit`, with what
    /// `cache` keeps from before, and keeps there what it read.
    pub fn build(
        root: &Path,
        commit: &str,
        cache: &Path,
        settings: &IndexSettings,
    ) -> Result<Index> {
        Index::build_cancellable(root, commit, cache, settings, &Cancel::new())
    }

    /// [`Index::build`], for a job that can be cancelled: once `cancel` is,
    /// an indexer that's running is stopped and the build fails.
    pub fn build_cancellable(
        root: &Path,
        commit: &str,
        cache: &Path,
        settings: &IndexSettings,
        cancel: &Cancel,
    ) -> Result<Index> {
        let started = Instant::now();
        // It's given to git after its options, so it mustn't read as one.
        if commit.is_empty() || commit.starts_with('-') {
            bail!("{commit:?} isn't a commit");
        }
        let mut entries = files::list(root, commit)?;
        entries.retain(|entry| !glob::any(&settings.exclude, &entry.path));
        std::fs::create_dir_all(cache)?;
        let read: Vec<&Entry> = entries
            .iter()
            .filter(|entry| entry.lang.is_some() && files::readable(entry, settings))
            .collect();
        let (outlines, precise) = thread::scope(|scope| {
            let precise = scope.spawn(|| match settings.precise {
                true => precise::run(root, commit, cache, settings, &entries, &read, cancel),
                false => precise::Run::default(),
            });
            let outlines = cache::outlines(root, cache, &read, cancel);
            (outlines, precise.join().unwrap_or_default())
        });
        let outlines = outlines?;
        if cancel.is_cancelled() {
            bail!("cancelled");
        }
        let mut index = Index::assemble(&entries, &read, &outlines, &precise);
        index.programs = programs(root, &entries);
        index.command_tree();
        index.took = started.elapsed();
        Ok(index)
    }

    /// The index of `read`, the files read for definitions, and what each
    /// says, with what the indexers said of them.
    fn assemble(
        entries: &[Entry],
        read: &[&Entry],
        outlines: &HashMap<String, Outline>,
        precise: &precise::Run,
    ) -> Index {
        let mut index = Index::default();
        for entry in entries {
            index.files.insert(entry.path.clone());
            let mut dir = entry.path.as_str();
            while let Some((parent, _)) = dir.rsplit_once('/') {
                if !index.dirs.insert(parent.to_string()) {
                    break;
                }
                dir = parent;
            }
        }
        // Each SCIP definition's place here, by its id in the run.
        let mut precise_ids: HashMap<u32, u32> = HashMap::new();
        // Go: each cobra command by the variable it's kept in, and what's
        // added or given to one, by the variable it names.
        let mut commands: HashMap<(String, String), u32> = HashMap::new();
        let mut given: Vec<(u32, (String, String))> = Vec::new();
        let mut counts: HashMap<Lang, (usize, usize, usize)> = HashMap::new();
        for entry in read {
            let Some(lang) = entry.lang else { continue };
            let Some(outline) = outlines.get(&entry.path) else {
                continue;
            };
            index.lines.insert(entry.path.clone(), outline.lines);
            let count = counts.entry(lang).or_default();
            count.0 += 1;
            let module = files::module(lang, &entry.path, &outline.package);
            let doc = precise.docs.get(&entry.path);
            count.1 += usize::from(doc.is_some());
            let test_file = files::is_test(&entry.path);
            let mut matched = HashSet::new();
            for found in file_module(lang, &module).iter().chain(&outline.defs) {
                let id = doc.and_then(|doc| {
                    let name = found.segments.last()?;
                    let at = doc.defs.iter().position(|def| {
                        def.line == found.line && &def.name == name && !matched.contains(&def.id)
                    })?;
                    matched.insert(doc.defs[at].id);
                    Some(doc.defs[at].id)
                });
                let module = match found.start {
                    0 => &module[..module.len() - 1],
                    _ => &module[..],
                };
                let at = index.add(lang, &entry.path, module, found, id.is_some(), test_file);
                if let Some(id) = id {
                    precise_ids.insert(id, at);
                }
                if let Some(var) = &found.var {
                    commands.insert(variable(&entry.path, var), at);
                }
                if let Some(of) = &found.of {
                    given.push((at, variable(&entry.path, of)));
                }
                count.2 += 1;
            }
            // What the indexer found that the grammar didn't, but its
            // packages and namespaces: scip-go has each file of a package
            // define it, and none of them is the one place it is.
            for def in doc.iter().flat_map(|doc| &doc.defs) {
                if matched.contains(&def.id) || def.kind == DefKind::Module {
                    continue;
                }
                let found = Found {
                    segments: def.segments.clone(),
                    kind: def.kind,
                    start: def.start,
                    end: def.end,
                    line: def.line,
                    names: Vec::new(),
                    ty: None,
                    test: def.test,
                    decl: false,
                    var: None,
                    of: None,
                };
                let at = index.add(lang, &entry.path, &[], &found, true, test_file);
                precise_ids.insert(def.id, at);
                count.2 += 1;
            }
        }
        for (at, of) in given {
            index.meta[at as usize].of = commands.get(&of).copied();
        }
        for (path, doc) in &precise.docs {
            let refs = doc
                .refs
                .iter()
                .filter_map(|id| precise_ids.get(id).copied());
            index.refs.insert(path.clone(), refs.collect());
        }
        for ids in index.by_path.values_mut() {
            ids.sort_by_key(|&id| (index.defs[id as usize].start, id));
        }
        index.report = report(entries, &counts, precise);
        index
    }

    /// Which subcommands are under others, from what each holds, and the
    /// words of those that aren't: after [`Index::programs`], since a
    /// command added to the program's own is a top-level one.
    fn command_tree(&mut self) {
        let subcommands: Vec<u32> = (0..self.defs.len() as u32)
            .filter(|&id| self.is_subcommand(id))
            .collect();
        let mut held = HashSet::new();
        for &id in &subcommands {
            for ty in self.held_by(id) {
                held.extend(lookup::words(ty).map(String::from));
            }
        }
        self.held = held;
        let roots: Vec<String> = (subcommands.iter())
            .filter(|&&id| !self.is_nested(id))
            .flat_map(|&id| self.meta[id as usize].names.iter().cloned())
            .collect();
        self.roots = roots.into_iter().collect();
    }

    /// Whether `id` is a subcommand: a clap variant, which goes by its
    /// words, or a command a call defines.
    fn is_subcommand(&self, id: u32) -> bool {
        match self.defs[id as usize].kind {
            DefKind::Variant => !self.meta[id as usize].names.is_empty(),
            DefKind::Command => true,
            _ => false,
        }
    }

    /// The types the subcommand `id` holds, as written: its own, its
    /// fields', and the fields' of the struct it holds.
    fn held_by(&self, id: u32) -> impl Iterator<Item = &str> {
        let ty = self.meta[id as usize].ty.as_deref();
        let own = self.members.get(&self.defs[id as usize].name);
        let structs =
            (ty.into_iter().flat_map(lookup::words)).flat_map(|word| self.members.get(word));
        let fields = own.into_iter().chain(structs).flatten();
        let fields = fields.filter(|&&field| self.defs[field as usize].kind == DefKind::Field);
        ty.into_iter()
            .chain(fields.filter_map(|&field| self.meta[field as usize].ty.as_deref()))
    }

    /// Whether the subcommand `id` is under another: a clap enum's
    /// variant another subcommand holds, or a command added to another but
    /// the program's own.
    fn is_nested(&self, id: u32) -> bool {
        match self.defs[id as usize].kind {
            DefKind::Command => (self.meta[id as usize].of)
                .is_some_and(|parent| !self.programs.contains(&self.defs[parent as usize].name)),
            _ => {
                let segments = &self.meta[id as usize].segments;
                let enum_name = segments.len().checked_sub(2).map(|at| &segments[at]);
                enum_name.is_some_and(|name| self.held.contains(name))
            }
        }
    }

    /// Adds what `found` says is defined in the file at `path`, under its
    /// `module`, and gives back where it went.
    fn add(
        &mut self,
        lang: Lang,
        path: &str,
        module: &[String],
        found: &Found,
        precise: bool,
        test_file: bool,
    ) -> u32 {
        let at = u32::try_from(self.defs.len()).unwrap_or(u32::MAX);
        let mut segments = module.to_vec();
        segments.extend(found.segments.iter().cloned());
        let name = found.segments.last().cloned().unwrap_or_default();
        let qualified = segments.join(lang.separator());
        if matches!(found.kind, DefKind::Field | DefKind::Variant)
            && let [.., container, _] = found.segments.as_slice()
        {
            self.members.entry(container.clone()).or_default().push(at);
        }
        self.defs.push(Def {
            name: name.clone(),
            qualified,
            kind: found.kind,
            path: path.to_string(),
            start: found.start,
            end: found.end,
            precise,
        });
        self.meta.push(Meta {
            segments,
            names: found.names.clone(),
            ty: found.ty.clone(),
            of: None,
            test: found.test || test_file,
            decl: found.decl,
        });
        self.by_name.entry(name).or_default().push(at);
        for alias in &found.names {
            self.by_alias.entry(alias.clone()).or_default().push(at);
        }
        self.by_path.entry(path.to_string()).or_default().push(at);
        at
    }

    /// What a code span names: `Session`, `Session::stop`,
    /// `stop_idle_after`, `[sessions] stop_idle_after`, `lattice build`,
    /// `src/db.rs`, `--model`. `near` is the files the prose is about,
    /// whose definitions are preferred among several.
    pub fn lookup(&self, span: &str, near: &[String]) -> Lookup {
        lookup::lookup(self, span, near)
    }

    /// The definitions whose lines include `line` in the file at `path`,
    /// the innermost first: for checking a link a writer made.
    pub fn defined_at(&self, path: &str, line: u32) -> Vec<&Def> {
        let mut defs: Vec<&Def> = (self.by_path.get(path).into_iter().flatten())
            .map(|&id| &self.defs[id as usize])
            .filter(|def| def.start > 0 && def.start <= line && line <= def.end)
            .collect();
        defs.sort_by_key(|def| (def.end - def.start, std::cmp::Reverse(def.start)));
        defs
    }

    /// What's defined in `files`, each a file or a directory written with a
    /// `/` at its end, file by file and each file's in the order they come
    /// in it: for a writer's prompt. Tests inside a file that isn't one are
    /// left out.
    pub fn outline(&self, files: &[String]) -> Vec<&Def> {
        let mut paths: Vec<&str> = (self.by_path.keys())
            .map(String::as_str)
            .filter(|path| covers(files, path))
            .collect();
        paths.sort_unstable();
        let mut defs = Vec::new();
        for path in paths {
            let test_file = files::is_test(path);
            for &id in &self.by_path[path] {
                if test_file || !self.meta[id as usize].test {
                    defs.push(&self.defs[id as usize]);
                }
            }
        }
        defs
    }

    /// How each of the repository's languages was indexed, the most files
    /// first.
    pub fn report(&self) -> &[LanguageReport] {
        &self.report
    }

    /// How many files git tracks at the commit, but those left out.
    pub fn files(&self) -> usize {
        self.files.len()
    }

    /// How many definitions the index has.
    pub fn definitions(&self) -> usize {
        self.defs.len()
    }

    /// How long building it took.
    pub fn took(&self) -> Duration {
        self.took
    }

    /// What the index has, in a few lines for a build's log: how long it
    /// took, and each language's tier with why.
    pub fn summary(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "indexed {} files, {} definitions, in {:.1}s",
            self.files.len(),
            self.defs.len(),
            self.took.as_secs_f64()
        )];
        for language in &self.report {
            let mut line = format!(
                "{}: {} ({} files, {} definitions",
                language.language,
                language.tier.as_str(),
                language.files,
                language.definitions
            );
            if language.precise_files > 0 && language.precise_files < language.files {
                line.push_str(&format!(", {} of them precise", language.precise_files));
            }
            line.push(')');
            if let Some(note) = &language.note {
                line.push_str(&format!(": {note}"));
            }
            lines.push(line);
        }
        lines
    }
}

/// Where a Go variable a file at `path` names is, and its name: a
/// package's, `name`, in the file's directory, as every file of the package
/// sees it; a function's, `name@<where>`, in the file.
fn variable(path: &str, var: &str) -> (String, String) {
    let place = match var.contains('@') {
        true => path,
        false => path.rsplit_once('/').map_or("", |(dir, _)| dir),
    };
    (place.to_string(), var.to_string())
}

/// Whether one of `files`, files and directories written with a `/` at
/// their end (`/` alone is the whole repository), covers `path`.
fn covers(files: &[String], path: &str) -> bool {
    files.iter().any(|file| match file.strip_suffix('/') {
        Some("") => true,
        Some(dir) => path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/')),
        None => file == path,
    })
}

/// The definition of the module a file is, named by the last part of its
/// `module`, as a whole (its lines 0): Rust's and Python's files are their
/// modules, which a `mod x;` only declares.
fn file_module(lang: Lang, module: &[String]) -> Option<Found> {
    if !matches!(lang, Lang::Rust | Lang::Python) {
        return None;
    }
    let name = module.last()?;
    Some(Found {
        segments: vec![name.clone()],
        kind: DefKind::Module,
        start: 0,
        end: 0,
        line: 1,
        names: Vec::new(),
        ty: None,
        test: false,
        decl: false,
        var: None,
        of: None,
    })
}

/// Each language's report, from how many of its files were read, how many
/// an indexer covered and how many definitions they had, and what the
/// indexers said.
fn report(
    entries: &[Entry],
    counts: &HashMap<Lang, (usize, usize, usize)>,
    precise: &precise::Run,
) -> Vec<LanguageReport> {
    let mut present: HashMap<Lang, usize> = HashMap::new();
    for entry in entries {
        if let Some(lang) = entry.lang {
            *present.entry(lang).or_default() += 1;
        }
    }
    let mut report: Vec<LanguageReport> = present
        .into_iter()
        .map(|(lang, all)| {
            let (files, precise_files, definitions) =
                counts.get(&lang).copied().unwrap_or_default();
            let tier = if precise_files > 0 {
                Tier::Precise
            } else if files == 0 {
                Tier::Paths
            } else if grammar::compiled(lang).is_some() {
                Tier::Syntactic
            } else if keywords::reads(lang) {
                Tier::Keywords
            } else {
                Tier::Paths
            };
            let note = precise.notes.get(&lang).cloned().or_else(|| match tier {
                Tier::Paths if files == 0 && all > 0 => {
                    Some("its files are all paths only or too big to read".to_string())
                }
                Tier::Paths => Some("lattice reads no definitions in it".to_string()),
                Tier::Keywords => Some("no grammar for it is compiled in".to_string()),
                _ => None,
            });
            LanguageReport {
                language: lang.name().to_string(),
                files,
                tier,
                precise_files,
                definitions,
                note,
            }
        })
        .collect();
    report.sort_by(|a, b| {
        b.files
            .cmp(&a.files)
            .then_with(|| a.language.cmp(&b.language))
    });
    report
}

/// What the repository's command lines are called, for a span like
/// `lattice build`: its Cargo packages and their binaries, its Go `cmd/`
/// directories, its npm packages' `bin`, its Python scripts.
fn programs(root: &Path, entries: &[Entry]) -> HashSet<String> {
    let mut programs = HashSet::new();
    let manifests: Vec<&Entry> = entries
        .iter()
        .filter(|entry| {
            let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
            matches!(name, "Cargo.toml" | "package.json" | "pyproject.toml")
                && !entry
                    .path
                    .split('/')
                    .any(|part| part == "vendor" || part == "node_modules")
        })
        .take(64)
        .collect();
    let blobs: Vec<String> = manifests.iter().map(|entry| entry.blob.clone()).collect();
    // Without them, a span starting with the program's name isn't read
    // as a command line.
    let texts = files::read_blobs(root, &blobs).unwrap_or_default();
    for (entry, text) in manifests.iter().zip(texts) {
        let text = String::from_utf8_lossy(&text);
        programs.extend(files::programs_in(&entry.path, &text));
    }
    for entry in entries {
        if let Some(rest) = entry.path.strip_prefix("cmd/")
            && let Some((name, _)) = rest.split_once('/')
        {
            programs.insert(name.to_string());
        }
    }
    programs
}

#[cfg(test)]
mod tests;
