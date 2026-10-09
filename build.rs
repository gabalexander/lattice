//! Carries the web app into the binary: every file under `web/build/`,
//! which `npm --prefix web run build` writes, as `web::FILES`, by its path
//! there. Without it, the list is empty and the server shows a page saying
//! how to build it, so lattice builds without node.

// Cargo reads a build script's directives from its standard output, which
// it reads to the end: `println!` is how they're written.
#![allow(clippy::disallowed_macros)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let build = root.join("web/build");
    // Cargo looks through a directory it's told to watch. Until the app is
    // built, `web/` is watched, for its build to appear in. A path watched
    // that isn't there would have Cargo build lattice again every time.
    if build.is_dir() {
        println!("cargo:rerun-if-changed=web/build");
    } else if root.join("web").is_dir() {
        println!("cargo:rerun-if-changed=web");
    }
    println!("cargo:rerun-if-changed=build.rs");
    let mut files = Vec::new();
    if build.is_dir() {
        gather(&build, &build, &mut files);
    }
    files.sort();
    let mut code = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (name, path) in files {
        writeln!(code, "    ({name:?}, include_bytes!({path:?})),").unwrap();
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("web_files.rs");
    std::fs::write(out, code).unwrap();
}

/// Every file under `dir`, by its path from `root` with `/` between its
/// parts, and where it is.
fn gather(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            gather(root, &path, files);
        } else if let Ok(below) = path.strip_prefix(root) {
            let name: Vec<String> = below
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            files.push((name.join("/"), path.display().to_string()));
        }
    }
}
