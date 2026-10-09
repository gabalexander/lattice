//! Whether what a writer answered is a text at all, before its links and
//! diagrams are checked, and the tidying every text gets.
//!
//! A writer that couldn't read the code says so in its answer's `failure`,
//! as it's told to. One that ignores that and writes the failure into the
//! page instead, as a refusal ("I can't access the repository") or as a
//! report of its tools failing, is caught by what its prose says: those
//! are fatal, and the writer is run again, once. What's only cosmetic, a
//! diagram that doesn't read, is the checks' to put right or drop.
//!
//! A text is tidied too: a first line repeating its title as a heading
//! goes, headings about how the page was written rather than about the code
//! ("Source of Truth") go, and a heading that is a code span alone loses
//! its backticks.
//!
//! Adapted from deepwiki-by-cc's `src/lib/server/ai/page-validation.ts` and
//! `src/lib/server/ai/generator.ts` (MIT; see THIRD_PARTY_NOTICES.md).

use super::fences;
use regex::Regex;
use std::sync::LazyLock;

/// Why a writer's answer is no text: what it said instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoText {
    /// It answered nothing.
    Empty,
    /// It said, in its answer's `failure`, that it couldn't do the work.
    Reported(String),
    /// Its text reports that its tools failed, or that the code couldn't
    /// be read.
    ToolsFailed(String),
    /// Its text is a refusal: it couldn't, or wouldn't, read the code.
    Refused(String),
}

impl std::fmt::Display for NoText {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            NoText::Empty => f.write_str("it answered with no text"),
            NoText::Reported(why) => write!(f, "it said it couldn't do the work: {why}"),
            NoText::ToolsFailed(said) => write!(f, "its text says its tools failed: {said}"),
            NoText::Refused(said) => write!(f, "its text is a refusal: {said}"),
        }
    }
}

/// What a text that reports its tools failing says.
static TOOLS_FAILED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    patterns(&[
        r"(?i)bwrap:\s*No permissions to create a new namespace",
        r"(?i)execution environment blocks all filesystem commands",
        r"(?is)(?:shell|command) (?:runner|wrapper|tool|commands?).{0,80}(?:fail|blocked|cannot|can't|could not).{0,80}(?:before execution|before executing|before they run)",
        r"(?is)(?:repository|repo|checkout|source) (?:files?|source|checkout)?.{0,80}(?:not readable|could not be read|cannot be read|blocked).{0,80}(?:available tools|current session|this session|this environment|execution environment|sandbox(?:ed)? environment)",
        r"(?is)(?:can't|cannot|could not).{0,80}(?:write|produce|complete).{0,80}(?:wiki page|requested page|subsection).{0,120}(?:checkout|repository|repo|source files).{0,80}(?:not readable|blocked|could not be read|cannot be read)",
    ])
});

/// What a refusal says.
static REFUSED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    patterns(&[
        r"(?i)\b(?:I|we) (?:can't|cannot|could not|am unable to|are unable to) (?:access|inspect|read|analyze) (?:the )?(?:repository|repo|source files|checkout)\b",
        r"(?i)\b(?:I|we) (?:don't|do not) have access to (?:the )?(?:repository|repo|source files|checkout|filesystem)\b",
        r"(?i)\bno repository files were provided\b",
    ])
});

/// What makes a match in code or a quote a failure after all: prose round
/// it saying the page couldn't be made.
static FAILURE_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)\b(?:i|we|this page|the page|requested page)\b.{0,100}\b(?:can't|cannot|could not|am unable to|are unable to|failed)\b|\b(?:unable|failed)\s+to\s+(?:access|inspect|read|analyze|write|produce|complete)\s+(?:this|the|requested)\s+(?:wiki\s+)?page\b",
    )
    .expect("the pattern reads")
});

fn patterns(sources: &[&str]) -> Vec<Regex> {
    sources
        .iter()
        .map(|source| Regex::new(source).expect("the pattern reads"))
        .collect()
}

/// Whether `text`, with `failure` as the answer's `failure`, is a text, or
/// why not.
pub fn text(text: &str, failure: Option<&str>) -> Result<(), NoText> {
    if let Some(why) = failure.map(str::trim).filter(|why| !why.is_empty()) {
        return Err(NoText::Reported(why.to_string()));
    }
    if text.trim().is_empty() {
        return Err(NoText::Empty);
    }
    if let Some(said) = matched(text, &TOOLS_FAILED) {
        return Err(NoText::ToolsFailed(said));
    }
    if let Some(said) = matched(text, &REFUSED) {
        return Err(NoText::Refused(said));
    }
    Ok(())
}

/// The start of `text` when one of `patterns` matches it as a failure: in
/// its prose, or in code or a quote with prose round it saying the page
/// couldn't be made, or with no prose besides. A page that quotes such a
/// message as an example, in a page that's otherwise a page, isn't one.
fn matched(text: &str, patterns: &[Regex]) -> Option<String> {
    if !patterns.iter().any(|pattern| pattern.is_match(text)) {
        return None;
    }
    let prose = fences::prose(text);
    let in_prose = patterns.iter().any(|pattern| pattern.is_match(&prose));
    let narrative: String = prose
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    if !in_prose && !narrative.trim().is_empty() && !FAILURE_CONTEXT.is_match(&prose) {
        return None;
    }
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(flat.chars().take(220).collect())
}

/// Headings that are about how the page was written, as a model sometimes
/// echoes its instructions, compared lower case without punctuation.
const POLICY_HEADINGS: &[&str] = &[
    "code first",
    "source of truth",
    "code vs docs",
    "docs vs code",
    "trust hierarchy",
    "source trust hierarchy",
];

/// `md` tidied: a first line that's `title` as a heading taken out, a
/// heading about how it was written taken out (with the blank line after
/// it), and a heading that's only a code span unwrapped. What's in a fence
/// is left as it is.
pub fn tidy(md: &str, title: &str) -> String {
    let md = without_title(md, title);
    let lines: Vec<&str> = md.split('\n').collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut fence: Option<char> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        if let Some(c) = marker
            && trimmed.starts_with(&c.to_string().repeat(3))
        {
            match fence {
                None => fence = Some(c),
                Some(open) if open == c => fence = None,
                Some(_) => {}
            }
            out.push(line.to_string());
            i += 1;
            continue;
        }
        if fence.is_some() {
            out.push(line.to_string());
            i += 1;
            continue;
        }
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let text = trimmed[hashes..].trim().trim_end_matches('#').trim();
            if POLICY_HEADINGS.contains(&plain(text).as_str()) {
                i += 1;
                if lines.get(i).is_some_and(|next| next.trim().is_empty()) {
                    i += 1;
                }
                continue;
            }
            if let Some(code) = text
                .strip_prefix('`')
                .and_then(|rest| rest.strip_suffix('`'))
                .filter(|code| !code.contains('`'))
            {
                out.push(format!("{} {code}", &trimmed[..hashes]));
                i += 1;
                continue;
            }
        }
        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

/// `md` without a first line that's `title` as a heading, and the blank
/// lines after it.
fn without_title(md: &str, title: &str) -> String {
    let start = md.len() - md.trim_start().len();
    let rest = &md[start..];
    let (first, after) = rest.split_once('\n').unwrap_or((rest, ""));
    let heading = first.trim_start_matches('#');
    if heading.len() < first.len() && heading.starts_with(' ') && plain(heading) == plain(title) {
        return after.trim_start_matches(['\n', '\r']).to_string();
    }
    md.to_string()
}

/// `text` lower case, its punctuation and code marks made spaces, its
/// spaces made one.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_reported_or_written_into_the_page_is_no_text() {
        assert_eq!(text("A page.", None), Ok(()));
        assert_eq!(text("  ", None), Err(NoText::Empty));
        assert_eq!(
            text("", Some("the files can't be read")),
            Err(NoText::Reported("the files can't be read".into()))
        );
        assert_eq!(text("A page.", Some("  ")), Ok(()));
        assert!(matches!(
            text(
                "I cannot access the repository with the tools I have, sorry.",
                None
            ),
            Err(NoText::Refused(_))
        ));
        assert!(matches!(
            text(
                "The repository files could not be read in this session, so the page is empty.",
                None
            ),
            Err(NoText::ToolsFailed(_))
        ));
    }

    #[test]
    fn a_page_that_quotes_such_a_message_is_still_a_page() {
        let page = "The preflight catches a sandbox that fails, which prints:\n\n```\nI cannot access the repository\n```\n\nand the build stops before it spends anything.";
        assert_eq!(text(page, None), Ok(()));
        let dump = "```\nbwrap: No permissions to create a new namespace\n```";
        assert!(matches!(text(dump, None), Err(NoText::ToolsFailed(_))));
        let said = "I could not complete this page:\n\n> I cannot access the repository";
        assert!(matches!(text(said, None), Err(NoText::Refused(_))));
    }

    #[test]
    fn a_text_is_tidied() {
        let md = "# The Job Queue\n\nIt queues.\n\n#### Source of Truth\n\nWhat the code does.\n\n#### `Retrier`\n\n```md\n## Code First\n```\n\n#### Code vs. Docs";
        assert_eq!(
            tidy(md, "The job queue"),
            "It queues.\n\nWhat the code does.\n\n#### Retrier\n\n```md\n## Code First\n```\n"
        );
        assert_eq!(
            tidy("# Other\n\nText.", "The Job Queue"),
            "# Other\n\nText."
        );
    }
}
