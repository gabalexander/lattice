//! Writing commands and paths the way a shell reads them.
//!
//! Adapted from crystal's `src/shell.rs` (MIT).

use std::path::{Path, PathBuf};

/// `path` with the home directory written as `~`, as you'd type it: the
/// home directory as `HOME` says it, or as it really is, when that's
/// through a link, as a path made real is.
pub fn home_relative(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()) else {
        return path.display().to_string();
    };
    let home = PathBuf::from(home);
    let real = home.canonicalize().ok().filter(|real| *real != home);
    match real {
        Some(real) if path.starts_with(&real) => relative_to(path, &real),
        _ => relative_to(path, &home),
    }
}

/// `path` with a leading `~` made the home directory, as a shell would.
pub fn expand_home(path: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(rest)
        }
        Err(_) => path.to_path_buf(),
    }
}

/// `path` with `home` written as `~`. Paths compare whole directory names,
/// so `/home/ann` isn't under `/home/an`.
fn relative_to(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// `arg` as you'd type it into a shell: as it is when that's safe, or else
/// in single quotes.
pub fn quote(arg: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c);
    if !arg.is_empty() && arg.chars().all(plain) {
        return arg.to_string();
    }
    // Inside single quotes nothing is special but the quote itself, which
    // has to close the quotes, be escaped, and open them again.
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// Splits `line` into words the way a shell does: at spaces, except inside
/// single or double quotes. Outside single quotes, a backslash takes the
/// character after it as it is. What `$EDITOR` holds is read this way.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    // Whether a word has begun, which `""` does even though it adds nothing.
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('"'), '\\') | (None, '\\') => {
                let escaped = chars.next().ok_or("the line ends in a backslash")?;
                word.push(escaped);
                in_word = true;
            }
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("a quote isn't closed".to_string());
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_directory_is_written_as_a_tilde() {
        let home = Path::new("/home/ann");
        assert_eq!(
            relative_to(Path::new("/home/ann/code/app"), home),
            "~/code/app"
        );
        assert_eq!(relative_to(Path::new("/home/ann"), home), "~");
        assert_eq!(relative_to(Path::new("/home/anna"), home), "/home/anna");
        assert_eq!(relative_to(Path::new("/tmp"), home), "/tmp");
    }

    #[test]
    fn a_line_is_split_into_words_as_a_shell_splits_it() {
        let split = |line: &str| split(line).unwrap();
        assert_eq!(split("code --wait"), ["code", "--wait"]);
        assert_eq!(split("  'my editor'  -n "), ["my editor", "-n"]);
        assert_eq!(split(r#"a\ b "c \"d\"" ''"#), ["a b", "c \"d\"", ""]);
        assert!(super::split("'open").is_err());
        assert!(super::split("end\\").is_err());
    }

    #[test]
    fn plain_words_are_left_alone() {
        assert_eq!(quote("--model=opus"), "--model=opus");
    }

    #[test]
    fn what_a_shell_would_split_is_quoted() {
        assert_eq!(quote("exit 3"), "'exit 3'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote(""), "''");
    }
}
