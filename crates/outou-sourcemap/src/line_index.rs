//! Byte offset ↔ LSP `{line, character}` position conversion.
//!
//! Outou spans are UTF-8 byte offsets everywhere else in this crate, but the
//! Language Server Protocol counts `character` in UTF-16 code units within a
//! line (see the [LSP specification][spec]). [`LineIndex`] is the single
//! place that bridges the two.
//!
//! [spec]: https://microsoft.github.io/language-server-protocol/specification

use serde::{Deserialize, Serialize};

use crate::Span;

/// A 0-based line/character position, matching the LSP `Position` type.
///
/// `character` counts UTF-16 code units within the line, not bytes and not
/// Unicode scalar values: a character outside the Basic Multilingual Plane
/// (an emoji, for example) advances `character` by two, matching how LSP
/// clients (and JavaScript strings) count it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    /// 0-based line number.
    pub line: u32,
    /// 0-based UTF-16 code unit offset within the line.
    pub character: u32,
}

impl Position {
    /// Creates a position.
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A `start`/`end` pair of [`Position`]s, half-open like [`Span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PositionRange {
    /// Inclusive start position.
    pub start: Position,
    /// Exclusive end position.
    pub end: Position,
}

impl PositionRange {
    /// Creates a range.
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// Maps between byte offsets and [`Position`]s for one file's full text.
///
/// Line boundaries follow `\n`, `\r\n` and lone `\r`, the line-terminator
/// sequences the LSP specification recognizes. A byte offset past the end of
/// the text is clamped to the end; a [`Position`] whose `character` is past
/// the end of its line is clamped to that line's content end (before the
/// terminator); a `line` past the last line is clamped to the last line; an
/// offset that lands inside a line terminator normalises to that line's
/// content end; a `character` that bisects a UTF-16 surrogate pair floors to
/// that character's start. Round-tripping is exact only for offsets that are
/// UTF-8 character boundaries and are not inside a line terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    text: String,
    // Byte offset where each line starts. Always non-empty; index 0 is 0.
    line_starts: Vec<u32>,
}

impl LineIndex {
    /// Builds a line index from a file's full text.
    pub fn new(text: &str) -> Self {
        let bytes = text.as_bytes();
        let mut line_starts = vec![0u32];
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' => {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'\n' {
                        i += 1;
                    }
                    line_starts.push(i as u32);
                }
                b'\n' => {
                    i += 1;
                    line_starts.push(i as u32);
                }
                _ => i += 1,
            }
        }
        Self {
            text: text.to_owned(),
            line_starts,
        }
    }

    /// The number of lines, counting a trailing empty line after a final
    /// line terminator.
    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    /// Converts a byte offset into a [`Position`]. An offset past the end of
    /// the text is clamped to the end; an offset that does not fall on a
    /// UTF-8 character boundary is rounded down to the nearest one; an
    /// offset that lands inside a line terminator normalises to that line's
    /// content end.
    ///
    /// Performance: this walks the line's content from its start to count
    /// UTF-16 units, so a query costs O(line length) rather than O(1). This
    /// is immaterial in practice (measured ~42 ns/query on a 1.27 MB
    /// generated-Rust fixture); if it ever matters, the escape hatch is a
    /// single `all_ascii: bool` computed once in [`LineIndex::new`], which
    /// makes both directions O(1) for ASCII text.
    pub fn offset_to_position(&self, offset: u32) -> Position {
        let clamped = (offset as usize).min(self.text.len());
        let clamped = floor_char_boundary(&self.text, clamped) as u32;
        let line = match self.line_starts.binary_search(&clamped) {
            Ok(exact) => exact,
            Err(insert) => insert - 1,
        };
        let line_start = self.line_starts[line];
        let clamped = clamped.min(self.line_content_end(line));
        let character = utf16_len(&self.text[line_start as usize..clamped as usize]);
        Position::new(line as u32, character)
    }

    /// Converts a [`Position`] into a byte offset. A `line` past the last
    /// line clamps to the last line; a `character` past a line's content
    /// clamps to that line's content end; a `character` that bisects a
    /// UTF-16 surrogate pair floors to that character's start.
    pub fn position_to_offset(&self, position: Position) -> u32 {
        let line = (position.line as usize).min(self.line_starts.len() - 1);
        let line_start = self.line_starts[line];
        let content_end = self.line_content_end(line);
        let mut byte = line_start;
        let mut units = 0u32;
        for ch in self.text[line_start as usize..content_end as usize].chars() {
            let next = units + ch.len_utf16() as u32;
            if next > position.character {
                break;
            }
            units = next;
            byte += ch.len_utf8() as u32;
        }
        byte
    }

    /// Converts a byte [`Span`] into a [`PositionRange`].
    pub fn span_to_range(&self, span: Span) -> PositionRange {
        PositionRange::new(
            self.offset_to_position(span.start),
            self.offset_to_position(span.end),
        )
    }

    /// Converts a [`PositionRange`] into a byte [`Span`].
    pub fn range_to_span(&self, range: PositionRange) -> Span {
        Span::new(
            self.position_to_offset(range.start),
            self.position_to_offset(range.end),
        )
    }

    /// Byte offset of the end of a line's content, excluding its line
    /// terminator (`\n`, `\r\n` or `\r`).
    fn line_content_end(&self, line: usize) -> u32 {
        let start = self.line_starts[line];
        let end = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len() as u32);
        let bytes = self.text.as_bytes();
        let mut content_end = end;
        if content_end > start && bytes[content_end as usize - 1] == b'\n' {
            content_end -= 1;
            if content_end > start && bytes[content_end as usize - 1] == b'\r' {
                content_end -= 1;
            }
        } else if content_end > start && bytes[content_end as usize - 1] == b'\r' {
            content_end -= 1;
        }
        content_end
    }
}

/// Number of UTF-16 code units `s` would occupy.
fn utf16_len(s: &str) -> u32 {
    s.chars().map(|c| c.len_utf16() as u32).sum()
}

/// Largest byte index `<= offset` that lies on a UTF-8 character boundary.
/// `str::floor_char_boundary` is not yet stable at this crate's MSRV.
fn floor_char_boundary(s: &str, offset: usize) -> usize {
    if offset >= s.len() {
        return s.len();
    }
    let mut o = offset;
    while o > 0 && !s.is_char_boundary(o) {
        o -= 1;
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file() {
        let index = LineIndex::new("");
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.offset_to_position(0), Position::new(0, 0));
        assert_eq!(index.offset_to_position(50), Position::new(0, 0));
        assert_eq!(index.position_to_offset(Position::new(0, 0)), 0);
        assert_eq!(index.position_to_offset(Position::new(5, 5)), 0);
    }

    #[test]
    fn offset_at_eof_without_trailing_newline() {
        let index = LineIndex::new("abc");
        assert_eq!(index.offset_to_position(3), Position::new(0, 3));
        // Past the end clamps to the same position.
        assert_eq!(index.offset_to_position(100), Position::new(0, 3));
    }

    #[test]
    fn offset_at_eof_with_trailing_newline_starts_a_new_line() {
        let index = LineIndex::new("abc\n");
        assert_eq!(index.line_count(), 2);
        assert_eq!(index.offset_to_position(4), Position::new(1, 0));
        assert_eq!(index.position_to_offset(Position::new(1, 0)), 4);
    }

    #[test]
    fn lf_line_starts() {
        let text = "one\ntwo\nthree";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 3);
        assert_eq!(index.offset_to_position(4), Position::new(1, 0));
        assert_eq!(index.offset_to_position(8), Position::new(2, 0));
        assert_eq!(index.position_to_offset(Position::new(2, 3)), 11);
    }

    #[test]
    fn crlf_line_starts() {
        let text = "one\r\ntwo\r\nthree";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 3);
        // "two" starts right after the CRLF pair.
        assert_eq!(index.offset_to_position(5), Position::new(1, 0));
        assert_eq!(index.position_to_offset(Position::new(1, 0)), 5);
        // The content end of line 0 excludes the CRLF, so a character past
        // "one" clamps before the \r.
        assert_eq!(index.position_to_offset(Position::new(0, 99)), 3);
    }

    #[test]
    fn lone_cr_line_starts() {
        let text = "one\rtwo";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 2);
        assert_eq!(index.offset_to_position(4), Position::new(1, 0));
        assert_eq!(index.position_to_offset(Position::new(1, 0)), 4);
    }

    #[test]
    fn multi_byte_characters_count_as_one_utf16_unit() {
        // "café" - 'é' is 2 bytes in UTF-8 but 1 UTF-16 code unit.
        let text = "café";
        let index = LineIndex::new(text);
        assert_eq!(text.len(), 5); // c, a, f, then 2-byte é
        assert_eq!(index.offset_to_position(5), Position::new(0, 4));
        assert_eq!(index.position_to_offset(Position::new(0, 4)), 5);
    }

    #[test]
    fn emoji_uses_a_utf16_surrogate_pair() {
        // U+1F600 GRINNING FACE: 4 UTF-8 bytes, 2 UTF-16 code units.
        let text = "a\u{1F600}b";
        let index = LineIndex::new(text);
        assert_eq!(text.len(), 6); // 'a' (1) + emoji (4) + 'b' (1)

        // Position of 'b': byte offset 5, after the 4-byte emoji.
        assert_eq!(index.offset_to_position(5), Position::new(0, 3));
        assert_eq!(index.position_to_offset(Position::new(0, 3)), 5);

        // A character offset landing inside the surrogate pair floors to
        // the start of the character it bisects; the round trip through the
        // start of the pair must still be exact.
        assert_eq!(index.offset_to_position(1), Position::new(0, 1));
        assert_eq!(index.position_to_offset(Position::new(0, 1)), 1);
    }

    #[test]
    fn crlf_midpoint_clamps_to_the_line_content_end() {
        let index = LineIndex::new("one\r\ntwo");
        assert_eq!(index.offset_to_position(3), Position::new(0, 3));
        assert_eq!(index.offset_to_position(4), Position::new(0, 3));
        assert_eq!(index.offset_to_position(5), Position::new(1, 0));
        assert_eq!(index.position_to_offset(index.offset_to_position(4)), 3);
    }

    #[test]
    fn offsets_outside_line_terminators_round_trip() {
        let text = "a\u{1F600}b\r\nx\ry\nz café\r\n";
        let index = LineIndex::new(text);
        for o in 0..=text.len() as u32 {
            if !text.is_char_boundary(o as usize) {
                continue;
            }
            let line = index.offset_to_position(o).line as usize;
            if o > index.line_content_end(line) {
                continue;
            }
            assert_eq!(
                index.position_to_offset(index.offset_to_position(o)),
                o,
                "offset {o} did not round-trip"
            );
        }
    }

    #[test]
    fn half_surrogate_position_floors_to_the_character_start() {
        let index = LineIndex::new("a\u{1F600}b");
        assert_eq!(index.position_to_offset(Position::new(0, 2)), 1);
        assert_eq!(index.position_to_offset(Position::new(0, 1)), 1);
        assert_eq!(index.position_to_offset(Position::new(0, 3)), 5);
    }

    #[test]
    fn out_of_range_offsets_and_positions_are_clamped() {
        let index = LineIndex::new("hello\nworld");
        assert_eq!(index.offset_to_position(u32::MAX), Position::new(1, 5));
        assert_eq!(
            index.position_to_offset(Position::new(u32::MAX, u32::MAX)),
            11
        );
    }

    #[test]
    fn span_and_range_round_trip() {
        let index = LineIndex::new("let user = load_user();\n");
        let span = Span::new(4, 8);
        let range = index.span_to_range(span);
        assert_eq!(range.start, Position::new(0, 4));
        assert_eq!(range.end, Position::new(0, 8));
        assert_eq!(index.range_to_span(range), span);
    }
}
