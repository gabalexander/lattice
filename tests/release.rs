//! The release's archives are named the same everywhere they're named:
//! the workflow that builds them, install.sh that downloads them, and the
//! Homebrew formula and its script. Change one, change them all.

use std::path::Path;

const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
];

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

#[test]
fn every_archive_is_named_alike_where_it_s_built_downloaded_and_brewed() {
    let workflow = read(".github/workflows/release.yml");
    assert!(
        workflow
            .contains("ARCHIVE: lattice-${{ needs.version.outputs.version }}-${{ matrix.target }}")
    );
    assert!(workflow.contains("release/lattice\" README.md LICENSE \"$ARCHIVE/\""));
    let install = read("install.sh");
    assert!(install.contains("archive=\"lattice-$version-$target\""));
    assert!(install.contains("url=\"$releases/download/v$version/$archive.tar.gz\""));
    assert!(install.contains("cp \"$tmp/$archive/lattice\""));
    let formula = read("packaging/homebrew/lattice.rb");
    let script = read("packaging/homebrew/formula.sh");
    for target in TARGETS {
        assert!(workflow.contains(&format!("target: {target}")), "{target}");
        assert!(
            formula.contains(&format!(
                "@RELEASES@/download/v@VERSION@/lattice-@VERSION@-{target}.tar.gz"
            )),
            "{target}"
        );
        assert!(formula.contains(&format!("@SHA256_{target}@")), "{target}");
        assert!(script.contains(target), "{target}");
    }
    for os in ["apple-darwin", "unknown-linux-musl"] {
        assert!(install.contains(&format!("os=\"{os}\"")), "{os}");
    }
}
