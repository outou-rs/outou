//! Rendering [`crate::Diagnostic`]s to the exact text format the fixtures
//! under `tests/fixtures/{incomplete,diagnostics}/` compare against:
//!
//! ```text
//! error: <message>
//!  --> <file>:<line>:<col>
//! ```
//!
//! **Column convention.** Line and column are both 1-based. A column is a
//! count of Unicode *characters* (`char`s, i.e. Unicode scalar values), not
//! UTF-8 bytes and not grapheme clusters — the same convention rustc uses
//! for human-facing diagnostics. This only matters for `.rsx` files
//! containing non-ASCII text before the diagnosed position; every fixture
//! in this repository is ASCII up to that point, so the distinction is not
//! exercised by the required fixtures, but is documented here because the
//! grammar leaves it open.

use crate::{Diagnostic, Parsed};

impl Parsed {
    /// Renders every diagnostic, in emission order, as
    /// `error: <message>\n --> <file>:<line>:<col>`, one after another
    /// separated by a newline. `file_name` is used verbatim, exactly as it
    /// appears in the `--> ` line of the fixture `.expected` files (a bare
    /// file name, not a full path).
    pub fn render_diagnostics(&self, file_name: &str) -> String {
        render_diagnostics(&self.diagnostics, &self.source, file_name)
    }
}

/// Renders `diagnostics` against `source`, whose byte offsets they were
/// computed from.
pub fn render_diagnostics(diagnostics: &[Diagnostic], source: &str, file_name: &str) -> String {
    let mut out = String::new();
    for (index, diag) in diagnostics.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let (line, col) = line_col(source, diag.span.start);
        out.push_str("error: ");
        out.push_str(&diag.message);
        out.push('\n');
        out.push_str(" --> ");
        out.push_str(file_name);
        out.push(':');
        out.push_str(&line.to_string());
        out.push(':');
        out.push_str(&col.to_string());
    }
    out
}

/// Converts a byte offset into `source` to a 1-based `(line, column)` pair,
/// counting columns in characters. `byte_offset` past the end of `source`
/// is clamped to the end, and one that does not land on a character
/// boundary (L1: recovery paths make no promise a span is that precise)
/// is walked down to the nearest boundary at or before it, using
/// `char_indices` rather than slicing so this can never panic.
fn line_col(source: &str, byte_offset: u32) -> (usize, usize) {
    let offset = (byte_offset as usize).min(source.len());
    let mut line = 1usize;
    let mut col = 1usize;
    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use outou_sourcemap::Span;

    #[test]
    fn first_line_first_column() {
        assert_eq!(line_col("abc", 0), (1, 1));
    }

    #[test]
    fn after_a_newline() {
        assert_eq!(line_col("ab\ncd", 3), (2, 1));
    }

    #[test]
    fn counts_characters_not_bytes() {
        // "café" — é is 2 UTF-8 bytes but 1 character.
        let source = "café\nx";
        let after_newline = source.find('x').unwrap() as u32;
        assert_eq!(line_col(source, after_newline), (2, 1));
    }

    /// Regression for L1: a span computed slightly off (recovery paths
    /// make no promise the byte offset lands exactly on a character
    /// boundary) must not panic slicing `source[..offset]`.
    #[test]
    fn non_boundary_offset_does_not_panic() {
        let source = "café";
        // 'é' starts at byte 3 and is 2 UTF-8 bytes long, so byte 4 is
        // inside its encoding — not a char boundary.
        assert!(!source.is_char_boundary(4));
        let _ = line_col(source, 4);
    }

    #[test]
    fn render_format_matches_fixtures() {
        let diagnostics = vec![Diagnostic {
            span: Span::new(6, 12),
            message: "closing tag `</span>` does not match opening tag `<div>`".to_string(),
            severity: crate::Severity::Error,
        }];
        let source = "12345\n</span>";
        let rendered = render_diagnostics(&diagnostics, source, "mismatched-closing-tag.rsx");
        assert_eq!(
            rendered,
            "error: closing tag `</span>` does not match opening tag `<div>`\n --> mismatched-closing-tag.rsx:2:1"
        );
    }
}
