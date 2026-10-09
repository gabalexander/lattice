//! The index's rules on repositories held in memory, read as a build reads
//! them but for git; `tests/cli.rs` builds them from git, a fixture
//! repository a language.

use super::*;

/// The index of `sources`, each a file's path and what's in it, and what
/// an indexer said of them.
fn index_with(sources: &[(&str, &str)], precise: &precise::Run) -> Index {
    let entries: Vec<Entry> = (sources.iter().enumerate())
        .map(|(at, (path, text))| Entry {
            path: path.to_string(),
            blob: format!("{at:040}"),
            size: text.len() as u64,
            lang: Lang::of(path),
        })
        .collect();
    let read: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.lang.is_some())
        .collect();
    let mut parser = tree_sitter::Parser::new();
    let outlines: HashMap<String, Outline> = (sources.iter())
        .filter_map(|(path, text)| {
            let outline = cache::read(Lang::of(path)?, text.as_bytes(), &mut parser)?;
            Some((path.to_string(), outline))
        })
        .collect();
    let mut index = Index::assemble(&entries, &read, &outlines, precise);
    for (path, text) in sources {
        index.programs.extend(files::programs_in(path, text));
    }
    index.command_tree();
    index
}

fn index_of(sources: &[(&str, &str)]) -> Index {
    index_with(sources, &precise::Run::default())
}

/// Where `span` links, near `near`: its target, `N things` when it could
/// be several, or `nothing`.
fn look(index: &Index, span: &str, near: &[&str]) -> String {
    let near: Vec<String> = near.iter().map(|path| path.to_string()).collect();
    match index.lookup(span, &near) {
        Lookup::Unique(def) => def.target(),
        Lookup::Ambiguous(defs) => format!("{} things", defs.len()),
        Lookup::Missing => "nothing".to_string(),
    }
}

const SESSION: &str = "\
pub struct Session {
    name: String,
}

impl Session {
    pub fn name(&self) -> &str {
        &self.name
    }
}
";

#[test]
fn a_capitalized_name_is_a_type_s_but_never_a_test_s() {
    let kind = "pub enum Kind {\n    Session,\n    Task,\n}\n";
    let tests = "#[cfg(test)]\nmod tests {\n    struct Task;\n}\n";
    let index = index_of(&[
        ("src/session.rs", SESSION),
        ("src/kind.rs", kind),
        ("src/check.rs", tests),
    ]);
    assert_eq!(look(&index, "Session", &[]), "src/session.rs#L1-L3");
    assert_eq!(look(&index, "Kind::Session", &[]), "src/kind.rs#L2");
    // The only type called `Task` is a test's, so it's the variant.
    assert_eq!(look(&index, "Task", &[]), "src/kind.rs#L3");
    // A test's own, alone: its function by its name, not its stand-ins.
    let tests = "#[cfg(test)]\nmod tests {\n    struct Fake;\n    const OPTIONS: u8 = 1;\n    fn rings_twice() {}\n}\n";
    let alone = index_of(&[("src/check.rs", tests)]);
    assert_eq!(look(&alone, "rings_twice", &[]), "src/check.rs#L5");
    assert_eq!(look(&alone, "Fake", &[]), "1 things");
    assert_eq!(look(&alone, "OPTIONS", &[]), "1 things");
    assert_eq!(
        look(&alone, "OPTIONS", &["src/check.rs"]),
        "src/check.rs#L4"
    );
    // A getter is meant over the field it gets.
    assert_eq!(look(&index, "Session::name", &[]), "src/session.rs#L6-L8");
    assert_eq!(look(&index, "Session.name", &[]), "src/session.rs#L6-L8");
    // Rust writes a module's items with `::`: this is an event's name.
    assert_eq!(look(&index, "session.Session", &[]), "nothing");
}

#[test]
fn of_several_the_one_in_the_files_near_or_the_file_named_after_it_is_meant() {
    let viewer = "pub struct Viewer {\n    rows: u16,\n}\n";
    let other = "struct Viewer;\n\npub fn new() {}\n";
    let config = "pub fn new() {}\n";
    let index = index_of(&[
        ("src/viewer.rs", viewer),
        ("src/session.rs", other),
        ("src/config.rs", config),
    ]);
    assert_eq!(look(&index, "Viewer", &[]), "src/viewer.rs#L1-L3");
    assert_eq!(
        look(&index, "Viewer", &["src/session.rs"]),
        "src/session.rs#L1"
    );
    assert_eq!(look(&index, "new", &[]), "2 things");
    assert_eq!(look(&index, "new", &["src/config.rs"]), "src/config.rs#L1");
    assert_eq!(
        look(&index, "new()", &["src/config.rs"]),
        "src/config.rs#L1"
    );
    assert_eq!(look(&index, "struct new", &[]), "nothing");
}

#[test]
fn what_an_indexer_says_a_file_refers_to_tells_several_apart() {
    let main = "fn main() {\n    let _ = config::new();\n}\n";
    let sources = [
        ("src/main.rs", main),
        ("src/session.rs", "pub fn new() {}\n"),
        ("src/config.rs", "pub fn new() {}\n"),
    ];
    let def = |id, segments: &[&str]| precise::Def {
        id,
        name: segments.last().unwrap().to_string(),
        segments: segments.iter().map(|segment| segment.to_string()).collect(),
        kind: DefKind::Function,
        line: 1,
        start: 1,
        end: 1,
        test: false,
    };
    let doc = |defs, refs| precise::Doc { defs, refs };
    let run = precise::Run {
        docs: HashMap::from([
            ("src/main.rs".to_string(), doc(vec![], vec![1])),
            (
                "src/session.rs".to_string(),
                doc(vec![def(0, &["session", "new"])], vec![]),
            ),
            (
                "src/config.rs".to_string(),
                doc(vec![def(1, &["config", "new"])], vec![]),
            ),
        ]),
        notes: HashMap::new(),
    };
    let grammar = index_of(&sources);
    assert_eq!(look(&grammar, "new", &["src/main.rs"]), "2 things");
    let precise = index_with(&sources, &run);
    assert_eq!(look(&precise, "new", &["src/main.rs"]), "src/config.rs#L1");
    let Lookup::Unique(new) = precise.lookup("config::new", &[]) else {
        panic!("config::new is there");
    };
    assert!(new.precise, "{new:?}");
}

#[test]
fn a_config_key_is_a_serde_field_s_followed_through_its_tables() {
    let config = "\
#[derive(Deserialize)]
pub struct Config {
    pub sessions: SessionSettings,
    pub colors: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct SessionSettings {
    pub stop_idle_after: u64,
}
";
    let runtime =
        "pub struct Running {\n    pub stop_idle_after: u64,\n    pub sessions: u32,\n}\n";
    let index = index_of(&[("src/config.rs", config), ("src/running.rs", runtime)]);
    assert_eq!(
        look(&index, "[sessions] stop_idle_after", &[]),
        "src/config.rs#L9"
    );
    assert_eq!(look(&index, "stop_idle_after", &[]), "2 things");
    assert_eq!(look(&index, "[sessions]", &[]), "src/config.rs#L3");
    assert_eq!(look(&index, "[colors]", &[]), "src/config.rs#L4");
    assert_eq!(look(&index, "[colors] stop_idle_after", &[]), "nothing");
    assert_eq!(
        look(&index, "sessions.stop_idle_after", &[]),
        "src/config.rs#L9"
    );
}

#[test]
fn a_subcommand_is_a_top_level_one_unless_the_words_before_say_otherwise() {
    let cargo = "[package]\nname = \"bell\"\n";
    let main = "\
#[derive(Subcommand)]
enum Command {
    Stop {
        #[arg(long)]
        now: bool,
    },
    Backlog(BacklogArgs),
}

#[derive(Args)]
struct BacklogArgs {
    #[command(subcommand)]
    command: BacklogCommand,
}

#[derive(Subcommand)]
enum BacklogCommand {
    Stop {
        #[arg(long)]
        all: bool,
    },
}
";
    let index = index_of(&[("Cargo.toml", cargo), ("src/main.rs", main)]);
    assert_eq!(look(&index, "bell stop", &[]), "src/main.rs#L3-L6");
    assert_eq!(look(&index, "stop --now", &[]), "src/main.rs#L5");
    assert_eq!(
        look(&index, "bell backlog stop", &[]),
        "src/main.rs#L18-L21"
    );
    assert_eq!(
        look(&index, "bell backlog stop --all", &[]),
        "src/main.rs#L20"
    );
    // A flag its command doesn't have says the command isn't the one.
    assert_eq!(look(&index, "bell stop --all", &[]), "nothing");
    assert_eq!(look(&index, "--all", &[]), "src/main.rs#L20");
}

#[test]
fn a_bare_word_is_a_field_s_only_as_its_key_and_in_go_never_as_a_word() {
    let config = "\
#[derive(Deserialize)]
pub struct Settings {
    pub restart_spacing_ms: u64,
    pub capture: bool,
    pub internal: bool,
}

pub struct Running {
    pub workers: u32,
}
";
    let volume = "\
package libpod

type PruneOptions struct {
\tExternal bool `json:\"external\"`
\tBuildCache bool `json:\"buildCache\"`
}

func (v *Volume) gid() int { return 0 }

func (v *Volume) mountPoint() string { return \"\" }
";
    let index = index_of(&[("src/config.rs", config), ("libpod/volume.go", volume)]);
    // A serde type's fields go by their keys; another's only near.
    assert_eq!(look(&index, "restart_spacing_ms", &[]), "src/config.rs#L3");
    assert_eq!(look(&index, "capture", &[]), "src/config.rs#L4");
    assert_eq!(look(&index, "workers", &[]), "nothing");
    assert_eq!(
        look(&index, "workers", &["src/config.rs"]),
        "src/config.rs#L9"
    );
    assert_eq!(look(&index, "Settings::internal", &[]), "src/config.rs#L5");
    // A Go tag or a package's own name that's a word is the word.
    assert_eq!(look(&index, "external", &[]), "nothing");
    assert_eq!(look(&index, "buildCache", &[]), "libpod/volume.go#L5");
    assert_eq!(look(&index, "gid", &[]), "nothing");
    assert_eq!(
        look(&index, "gid", &["libpod/volume.go"]),
        "libpod/volume.go#L8"
    );
    assert_eq!(look(&index, "Volume.gid", &[]), "libpod/volume.go#L8");
    assert_eq!(look(&index, "mountPoint", &[]), "libpod/volume.go#L10");
}

#[test]
fn a_writer_s_links_are_checked_and_its_prompt_outlined() {
    let tests = "#[cfg(test)]\nmod tests {\n    fn helper() {}\n}\n";
    let index = index_of(&[("src/session.rs", SESSION), ("src/x/check.rs", tests)]);
    let at: Vec<&str> = (index.defined_at("src/session.rs", 7).into_iter())
        .map(|def| def.qualified.as_str())
        .collect();
    assert_eq!(at, ["session::Session::name"]);
    assert!(index.defined_at("src/session.rs", 4).is_empty());
    let outline = |files: &[&str]| -> Vec<(&str, DefKind)> {
        let files: Vec<String> = files.iter().map(|file| file.to_string()).collect();
        (index.outline(&files).into_iter())
            .map(|def| (def.name.as_str(), def.kind))
            .collect()
    };
    let session = [
        ("session", DefKind::Module),
        ("Session", DefKind::Struct),
        ("name", DefKind::Field),
        ("name", DefKind::Method),
    ];
    assert_eq!(outline(&["src/session.rs"]), session);
    // A directory's files, but the tests inside them.
    let mut all = session.to_vec();
    all.extend([("check", DefKind::Module)]);
    assert_eq!(outline(&["src/"]), all);
    assert_eq!(outline(&["/"]), all);
    assert!(outline(&["src/session"]).is_empty());
}

#[test]
fn a_commit_that_reads_as_an_option_is_refused() {
    let cache = tempfile::tempdir().unwrap();
    let settings = IndexSettings::default();
    for commit in ["--output=/tmp/x", ""] {
        let err = Index::build(Path::new("/nowhere"), commit, cache.path(), &settings);
        assert!(format!("{:#}", err.unwrap_err()).contains("isn't a commit"));
    }
}

#[test]
fn a_link_s_target_has_the_lines_it_spans() {
    let def = |start, end| Def {
        name: "x".into(),
        qualified: "x".into(),
        kind: DefKind::Function,
        path: "src/x.rs".into(),
        start,
        end,
        precise: false,
    };
    assert_eq!(def(0, 0).target(), "src/x.rs");
    assert_eq!(def(4, 4).target(), "src/x.rs#L4");
    assert_eq!(def(4, 9).target(), "src/x.rs#L4-L9");
}
