//! Fenced code blocks as CommonMark reads them, which the diagram policy,
//! the page checks and the web app's renderer all go by
//! (`web/src/lib/fences.ts` keeps the same rules), so that they agree on
//! what's a diagram: a ```` ```mermaid ```` fence quoted inside a longer
//! fence is the outer block's text, not a diagram to draw, check or take
//! out.
//!
//! A fence opens at the start of a line, after up to three spaces, with
//! three or more backticks or tildes, and closes on a line of the same
//! character, at least as many, with nothing after them.
//!
//! Adapted from deepwiki-by-cc's `src/lib/markdown-fences.ts` (MIT; see
//! THIRD_PARTY_NOTICES.md).

use std::ops::Range;

/// A fenced block of mermaid at the top level of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// From the start of its opening line to the end of its closing marker
    /// (or of the text, when it isn't closed), without the newline after.
    pub range: Range<usize>,
    /// What's between the fences, trimmed.
    pub code: String,
}

/// An opening fence: its character and how many of it.
struct Open {
    marker: char,
    len: usize,
    mermaid: bool,
    start: usize,
    lines: Vec<String>,
}

/// A line's fence marker and what follows it: three or more backticks or
/// tildes after up to three spaces.
fn marker(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = rest.chars().take_while(|&x| x == c).count();
    (len >= 3).then(|| (c, len, &rest[len..]))
}

/// The top-level mermaid fences in `md`, in order. Empty ones are left out.
pub fn mermaid(md: &str) -> Vec<Fence> {
    let mut found = Vec::new();
    let mut open: Option<Open> = None;
    let mut offset = 0;
    for line in md.split('\n') {
        match (marker(line), &mut open) {
            (Some((c, len, info)), None) => {
                // A backtick fence's info string has no backtick in it.
                if c == '`' && info.contains('`') {
                    offset += line.len() + 1;
                    continue;
                }
                let lang = info.split_whitespace().next().unwrap_or_default();
                open = Some(Open {
                    marker: c,
                    len,
                    mermaid: lang.eq_ignore_ascii_case("mermaid"),
                    start: offset,
                    lines: Vec::new(),
                });
            }
            (Some((c, len, rest)), Some(block))
                if c == block.marker && len >= block.len && rest.trim().is_empty() =>
            {
                if block.mermaid {
                    let code = block.lines.join("\n").trim().to_string();
                    if !code.is_empty() {
                        found.push(Fence {
                            range: block.start..offset + line.len(),
                            code,
                        });
                    }
                }
                open = None;
            }
            (_, Some(block)) => block.lines.push(line.to_string()),
            (None, None) => {}
        }
        offset += line.len() + 1;
    }
    if let Some(block) = open.filter(|block| block.mermaid) {
        let code = block.lines.join("\n").trim().to_string();
        if !code.is_empty() {
            found.push(Fence {
                range: block.start..md.len(),
                code,
            });
        }
    }
    found
}

/// `md`'s prose alone: fenced blocks, code spans and quoted lines taken
/// out, what a reader takes as the page's own claims.
pub fn prose(md: &str) -> String {
    let mut out = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for line in md.lines() {
        if let Some((c, len)) = open {
            if let Some((mc, mlen, rest)) = marker(line)
                && mc == c
                && mlen >= len
                && rest.trim().is_empty()
            {
                open = None;
            }
            continue;
        }
        if let Some((c, len, _)) = marker(line) {
            open = Some((c, len));
            continue;
        }
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3 && trimmed.starts_with('>') {
            continue;
        }
        out.push(without_code_spans(line));
    }
    out.join("\n")
}

/// `line` with each code span on it made a space.
fn without_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let Some(close) = rest[open + 1..].find('`') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push(' ');
        rest = &rest[open + 1 + close + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mermaid_fence_is_found_with_where_it_is() {
        let md = "Text.\n\n```mermaid\nflowchart TD\n  a --> b\n```\n\nMore.\n";
        let found = mermaid(md);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "flowchart TD\n  a --> b");
        assert_eq!(
            &md[found[0].range.clone()],
            "```mermaid\nflowchart TD\n  a --> b\n```"
        );
    }

    #[test]
    fn a_fence_quoted_in_a_longer_one_is_its_text() {
        let md = "````markdown\n```mermaid\nflowchart TD\n  a --> b\n```\n````\n\n~~~ Mermaid\nsequenceDiagram\n  A->>B: hi\n~~~\n";
        let found = mermaid(md);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].code.starts_with("sequenceDiagram"));
    }

    #[test]
    fn a_fence_closes_only_on_its_own_marker_and_an_open_one_runs_to_the_end() {
        let md = "```mermaid\nflowchart TD\n``` not a close\n  a --> b\n~~~\n```\n```mermaid\ngraph LR\n  x --> y";
        let found = mermaid(md);
        assert_eq!(found.len(), 2);
        assert!(found[0].code.contains("``` not a close"));
        assert_eq!(found[1].code, "graph LR\n  x --> y");
        assert_eq!(found[1].range.end, md.len());
        assert!(mermaid("```mermaid\n\n```\n").is_empty());
        assert!(mermaid("    ```mermaid\n    graph TD\n    ```").is_empty());
    }

    #[test]
    fn prose_leaves_out_code_and_quotes() {
        let md = "I can't read `the repo`.\n> I cannot access the repository\n```\nI cannot read the checkout\n```\nDone.";
        assert_eq!(prose(md), "I can't read  .\nDone.");
    }
}
