//! Opening a file of a repository in the user's editor at a line, as a
//! click on a `code:` link in the wiki asks: `$VISUAL`, or else `$EDITOR`,
//! each of which may carry its own arguments, like `code --reuse-window`.
//!
//! The server has no terminal to give an editor that runs in one, like vi
//! or nano, so only an editor with a window of its own is started; for
//! another, the page is told to set `VISUAL` to one.
//!
//! Adapted from crystal's `src/wiki_server.rs` and `src/tui/mod.rs` (MIT).

use crate::shell;
use anyhow::{Context, Result, bail};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

/// The editors with a window of their own, which a click in the browser
/// can start as they are.
const WINDOWED: &[&str] = &[
    "code",
    "code-insiders",
    "codium",
    "cursor",
    "windsurf",
    "zed",
    "subl",
    "gvim",
    "mvim",
];

/// The file at `path`, from `root`, once it's a file in it: not above it,
/// nor out of it through a link.
pub fn file_in(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    let plain = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
    if path.is_empty() || !plain {
        return None;
    }
    let root = std::fs::canonicalize(root).ok()?;
    let file = std::fs::canonicalize(root.join(relative)).ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

/// Opens `file`, of the repository at `root`, in the user's editor at
/// `line`.
pub fn open(root: &Path, file: &Path, line: Option<usize>) -> Result<()> {
    let command = at(editor()?, file.display().to_string(), line);
    if !windowed(&command[0]) {
        bail!(
            "{} runs in a terminal, which a page can't give it: set VISUAL to an editor with \
             a window of its own, like code or zed, where lattice runs",
            command[0]
        );
    }
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("couldn't start {}", command[0]))?;
    // Waited for off the page's thread, so it doesn't linger.
    thread::spawn(move || child.wait());
    Ok(())
}

/// The user's editor, as a command line to put a file's path after:
/// `$VISUAL`, or else `$EDITOR`.
fn editor() -> Result<Vec<String>> {
    for name in ["VISUAL", "EDITOR"] {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        let command = shell::split(&value).map_err(|err| anyhow::anyhow!("${name}: {err}"))?;
        if !command.is_empty() {
            return Ok(command);
        }
    }
    bail!("neither VISUAL nor EDITOR says which editor to open it in, where lattice runs")
}

/// Whether `program` is an editor with a window of its own.
fn windowed(program: &str) -> bool {
    let name = Path::new(program).file_name().unwrap_or_default();
    WINDOWED.iter().any(|editor| name == *editor)
}

/// The command that opens `path` in `editor` at `line`: `+12 path`, the way
/// vi and most others take it, but for the editors that take `path:12`,
/// and VS Code and those made from it, which want `--goto` too.
fn at(mut editor: Vec<String>, path: String, line: Option<usize>) -> Vec<String> {
    let Some(line) = line else {
        editor.push(path);
        return editor;
    };
    let program = editor
        .first()
        .and_then(|program| Path::new(program).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    match program.as_str() {
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => {
            editor.extend(["--goto".to_string(), format!("{path}:{line}")]);
        }
        "zed" | "subl" => editor.push(format!("{path}:{line}")),
        _ => editor.extend([format!("+{line}"), path]),
    }
    editor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_to_open_must_be_in_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("secret"), "no\n").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret"), root.join("link")).unwrap();
        let real = std::fs::canonicalize(root.join("src/main.rs")).unwrap();
        assert_eq!(file_in(&root, "src/main.rs"), Some(real.clone()));
        assert_eq!(file_in(&root, "./src/main.rs"), Some(real));
        assert_eq!(file_in(&root, "../secret"), None);
        assert_eq!(file_in(&root, "src/../../secret"), None);
        let absolute = dir.path().join("secret").display().to_string();
        assert_eq!(file_in(&root, &absolute), None);
        assert_eq!(file_in(&root, "link"), None, "out through a link");
        assert_eq!(file_in(&root, "src"), None, "a directory");
        assert_eq!(file_in(&root, ""), None);
        assert_eq!(file_in(&root, "gone.rs"), None);
    }

    #[test]
    fn each_editor_is_given_the_line_the_way_it_takes_it() {
        let open = |editor: &[&str], line| {
            let editor = editor.iter().map(|word| word.to_string()).collect();
            at(editor, "src/x.rs".into(), line)
        };
        assert_eq!(
            open(&["code", "--reuse-window"], Some(12)),
            ["code", "--reuse-window", "--goto", "src/x.rs:12"]
        );
        assert_eq!(
            open(&["/usr/local/bin/zed"], Some(3)),
            ["/usr/local/bin/zed", "src/x.rs:3"]
        );
        assert_eq!(open(&["gvim"], Some(7)), ["gvim", "+7", "src/x.rs"]);
        assert_eq!(open(&["subl"], None), ["subl", "src/x.rs"]);
    }

    #[test]
    fn only_an_editor_with_a_window_starts_as_it_is() {
        assert!(windowed("code"));
        assert!(windowed("/usr/local/bin/zed"));
        assert!(!windowed("nvim"));
        assert!(!windowed("vi"));
    }
}
