//! Globs as `.gitignore` writes them, which the settings' `exclude` and
//! `[index] paths_only` take: `vendor/`, `*.pb.go`, `docs/**/*.svg`.
//!
//! Adapted from crystal's `src/wiki/files.rs` (MIT).

/// Whether one of `globs` matches `path`.
pub fn any(globs: &[String], path: &str) -> bool {
    globs.iter().any(|glob| matches(glob, path))
}

/// Whether the glob `glob` matches `path`, a file's from the top of the
/// repository, the way `.gitignore` reads one: `*` within a name, `**`
/// across directories, `?` one character; one with no `/` but at its end
/// matches a name at any depth; one ending with `/` matches a directory
/// and everything in it; one starting with `/` matches from the top only.
pub fn matches(glob: &str, path: &str) -> bool {
    let glob = glob.trim().trim_start_matches("./");
    if glob.is_empty() {
        return false;
    }
    let (glob, dir_only) = match glob.strip_suffix('/') {
        Some(dir) => (dir, true),
        None => (glob, false),
    };
    let anchored = glob.starts_with('/') || glob.contains('/');
    let glob = glob.trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    // A directory's glob matches the directories a file is in; a file's,
    // the file or any directory it's in, as `.gitignore` leaves out what's
    // under a directory it leaves out.
    let last = if dir_only {
        parts.len() - 1
    } else {
        parts.len()
    };
    if !anchored {
        return parts[..last].iter().any(|part| match_name(glob, part));
    }
    let glob: Vec<&str> = glob.split('/').collect();
    (1..=last).any(|end| match_parts(&glob, &parts[..end]))
}

/// Whether the glob's parts match the path's, `**` standing for any number
/// of directories.
fn match_parts(glob: &[&str], parts: &[&str]) -> bool {
    match glob.split_first() {
        None => parts.is_empty(),
        Some((&"**", rest)) => (0..=parts.len()).any(|skip| match_parts(rest, &parts[skip..])),
        Some((first, rest)) => match parts.split_first() {
            Some((part, others)) => match_name(first, part) && match_parts(rest, others),
            None => false,
        },
    }
}

/// Whether `glob`, `*` and `?` in it, matches the name `name`.
fn match_name(glob: &str, name: &str) -> bool {
    let glob: Vec<char> = glob.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut g, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if g < glob.len() && (glob[g] == '?' || glob[g] == name[n]) {
            g += 1;
            n += 1;
        } else if g < glob.len() && glob[g] == '*' {
            star = Some((g, n));
            g += 1;
        } else if let Some((at, matched)) = star {
            g = at + 1;
            n = matched + 1;
            star = Some((at, matched + 1));
        } else {
            return false;
        }
    }
    while g < glob.len() && glob[g] == '*' {
        g += 1;
    }
    g == glob.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_as_gitignore_reads_them() {
        assert!(matches("*.lock", "Cargo.lock"));
        assert!(matches("*.lock", "sub/yarn.lock"));
        assert!(!matches("*.lock", "src/lock.rs"));
        assert!(matches("vendor/", "vendor/a/b.go"));
        assert!(matches("vendor/", "x/vendor/a.go"));
        assert!(!matches("vendor/", "vendor"));
        assert!(!matches("vendor/", "src/vendored.rs"));
        assert!(matches("tests/mermaid", "tests/mermaid/a.txt"));
        assert!(matches("docs/*.md", "docs/a.md"));
        assert!(!matches("docs/*.md", "docs/sub/a.md"));
        assert!(matches("docs/**/*.md", "docs/sub/deeper/a.md"));
        assert!(matches("docs/**/*.md", "docs/a.md"));
        assert!(matches("**/fixtures/**", "a/fixtures/x.json"));
        assert!(matches("src/gen?.rs", "src/gen1.rs"));
        assert!(matches("/build", "build/out.c"));
        assert!(!matches("/build", "src/build/out.c"));
        assert!(!matches("", "a"));
        assert!(any(&["*.md".into(), "vendor/".into()], "vendor/x.go"));
        assert!(!any(&[], "a"));
    }
}
