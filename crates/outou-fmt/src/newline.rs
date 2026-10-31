//! Line endings. The formatter works on `\n` text internally; a `\r\n` file
//! is normalised on the way in and restored on the way out, so a CRLF file
//! never ends up with mixed endings.
//!
//! The rule follows `rustfmt`'s `newline_style` (read from the discovered
//! `rustfmt.toml`): `Unix` and `Native` give `\n`, `Windows` gives `\r\n`,
//! and `Auto` (rustfmt's default) keeps the style of the first line ending
//! in the input, as `rustfmt` itself does. A file with mixed endings is thus
//! made uniform. A lone `\r` is not a line ending and is left as it is.
//!
//! Rust's lexer reads `\r\n` inside a string literal as `\n`, so a
//! multi-line literal means the same before and after normalisation.

use std::borrow::Cow;

use crate::ctx::NewlineStyle;

/// The line ending the output will use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineEnding {
    Lf,
    CrLf,
}

/// The ending to write for a file with `source` as input under `configured`.
pub(crate) fn style_for_output(configured: NewlineStyle, source: &str) -> LineEnding {
    match configured {
        NewlineStyle::Unix => LineEnding::Lf,
        NewlineStyle::Windows => LineEnding::CrLf,
        NewlineStyle::Auto => match source.find('\n') {
            Some(at) if source[..at].ends_with('\r') => LineEnding::CrLf,
            _ => LineEnding::Lf,
        },
    }
}

/// `source` with every `\r\n` replaced by `\n`.
pub(crate) fn normalize(source: &str) -> Cow<'_, str> {
    if source.contains("\r\n") {
        Cow::Owned(source.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(source)
    }
}

/// `formatted` (which has `\n` endings only) in the requested style.
pub(crate) fn apply(formatted: String, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => formatted,
        LineEnding::CrLf => formatted.replace('\n', "\r\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_first_line_ending() {
        assert_eq!(
            style_for_output(NewlineStyle::Auto, "a\r\nb\n"),
            LineEnding::CrLf
        );
        assert_eq!(
            style_for_output(NewlineStyle::Auto, "a\nb\r\n"),
            LineEnding::Lf
        );
        assert_eq!(style_for_output(NewlineStyle::Auto, "a"), LineEnding::Lf);
    }

    #[test]
    fn an_explicit_style_ignores_the_input() {
        assert_eq!(
            style_for_output(NewlineStyle::Unix, "a\r\n"),
            LineEnding::Lf
        );
        assert_eq!(
            style_for_output(NewlineStyle::Windows, "a\n"),
            LineEnding::CrLf
        );
    }

    #[test]
    fn normalises_and_restores_crlf_but_leaves_a_lone_cr() {
        assert_eq!(normalize("a\r\nb\rc\n"), "a\nb\rc\n");
        assert_eq!(apply("a\nb\n".to_string(), LineEnding::CrLf), "a\r\nb\r\n");
    }
}
