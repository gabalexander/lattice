//! Text lattice didn't write, made fit for the user's terminal: a
//! repository's name from its remote, what Claude says on its standard
//! error, an error that quotes either.
//!
//! A terminal takes a control character as an order rather than as text.
//! ESC starts a sequence that sets the window's title, writes the clipboard
//! (OSC 52), makes a link (OSC 8) or switches to the alternate screen, and
//! the C1 controls (U+0080 to U+009F) start the same sequences in a
//! terminal that reads them; a carriage return or a backspace writes over
//! what was shown before it. The explicit bidi controls, the embeddings,
//! overrides and isolates (U+202A to U+202E, U+2066 to U+2069), turn the
//! text after them around in a terminal that lays out right-to-left text,
//! so that a line reads as something it isn't. What's here takes all of
//! them out and leaves the text around them as it was.
//!
//! Adapted from crystal's `src/printable.rs` (MIT).

use std::borrow::Cow;

/// Whether a terminal could take `c` as an order rather than draw it.
pub fn is_unprintable(c: char) -> bool {
    c.is_control() || is_bidi_control(c)
}

/// The explicit bidi formatting characters: LRE, RLE, PDF, LRO and RLO,
/// then LRI, RLI, FSI and PDI.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// `text` on one line: a line break, a tab or another control that's
/// whitespace made a space, and the rest of what [`is_unprintable`] taken
/// out.
pub fn line(text: &str) -> Cow<'_, str> {
    keep(text, |c| c.is_whitespace().then_some(' '))
}

/// `text` with its lines and tabs: a `\r\n` made `\n`, and the rest of what
/// [`is_unprintable`] taken out, a lone carriage return too, which would
/// write the line after it over the one before.
pub fn text(text: &str) -> Cow<'_, str> {
    keep(text, |c| matches!(c, '\n' | '\t').then_some(c))
}

/// `text` without what [`is_unprintable`] says, but for what `instead`
/// puts in its place. Borrowed when there's nothing to take out, as there
/// mostly isn't.
fn keep(text: &str, instead: impl Fn(char) -> Option<char>) -> Cow<'_, str> {
    if !text.contains(is_unprintable) {
        return Cow::Borrowed(text);
    }
    let kept = text
        .chars()
        .filter_map(|c| match is_unprintable(c) {
            true => instead(c),
            false => Some(c),
        })
        .collect();
    Cow::Owned(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strings meant to do something to a terminal, each with some text
    /// around it.
    const HOSTILE: &[&str] = &[
        "title\x1b]0;pwned\x07end",
        "alt\x1b[?1049hend",
        "copy\x1b]52;c;cm0gLXJmIH4=\x07end",
        "link\x1b]8;;https://evil.example\x1b\\here\x1b]8;;\x1b\\end",
        "dcs\x1bP1$qm\x1b\\end",
        "over\rwritten",
        "back\x08\x08space",
        "c1\u{9b}31mred\u{9d}0;t\u{9c}end",
        "del\x7fend",
        "bidi \u{202e}txt.exe\u{202c} \u{2067}isolate\u{2069} end",
    ];

    #[test]
    fn plain_text_is_left_as_it_is_and_borrowed() {
        for plain in ["", "fix the login redirect", "naïve 中文 🦀 a\u{200d}b"] {
            assert!(matches!(line(plain), Cow::Borrowed(text) if text == plain));
            assert!(matches!(text(plain), Cow::Borrowed(text) if text == plain));
        }
    }

    #[test]
    fn whats_left_of_an_order_is_text_that_does_nothing() {
        assert_eq!(line("title\x1b]0;pwned\x07end"), "title]0;pwnedend");
        assert_eq!(line("alt\x1b[?1049hend"), "alt[?1049hend");
        assert_eq!(line("c1\u{9b}31mred"), "c131mred");
        assert_eq!(line("back\x08\x08space"), "backspace");
        assert_eq!(line("del\x7fend"), "delend");
        assert_eq!(line("a\u{202e}b\u{2066}c\u{2069}d\u{202a}e"), "abcde");
        for hostile in HOSTILE {
            assert!(!line(hostile).contains(is_unprintable), "{hostile:?}");
            assert!(!text(hostile).contains(is_unprintable), "{hostile:?}");
        }
    }

    #[test]
    fn a_line_has_spaces_where_its_breaks_and_tabs_were() {
        assert_eq!(
            line("one\ntwo\tthree\r\nfour\u{85}five"),
            "one two three  four five"
        );
        assert_eq!(line("over\rwritten"), "over written");
    }

    #[test]
    fn text_keeps_its_lines_and_tabs_but_no_carriage_return() {
        assert_eq!(text("one\r\n\ttwo\n"), "one\n\ttwo\n");
        assert_eq!(text("50%\r100%"), "50%100%");
        assert_eq!(text("a\x1b[2Jb\nc"), "a[2Jb\nc");
    }
}
