//! JSX text whitespace normalization (grammar §8), Babel-exact.
//!
//! Each maximal run of text between two non-text boundaries (a tag, an
//! island, or the edge of an element) is normalized independently by
//! [`normalize_jsx_text`], following the same algorithm as Babel's
//! `cleanJSXElementLiteralChild` (used by `@babel/plugin-transform-react-jsx`,
//! and so by React's own JSX transform):
//!
//! 1. Split the run on line breaks (`\n`, `\r\n`, `\r`) into lines, keeping
//!    empty ones; the line breaks themselves are discarded.
//! 2. In every line, replace each tab with one space. This is a
//!    substitution over the whole line, not a trim.
//! 3. For each line: unless it is the first line of the run, remove
//!    leading U+0020 characters; unless it is the last line of the run,
//!    remove trailing U+0020 characters. Only U+0020 is removed — no other
//!    Unicode whitespace (U+00A0, U+000B, U+000C, …) is touched.
//! 4. Drop every line that is empty after step 3.
//! 5. Join the remaining lines with a single U+0020. If no line remains,
//!    the run produces no text node at all.

/// Normalizes one run of raw JSX text per grammar §8. Returns `None` when
/// the run normalizes away to nothing (grammar §8 step 5): the caller must
/// not create a text child in that case.
pub fn normalize_jsx_text(raw: &str) -> Option<String> {
    let line_count = count_lines(raw);
    let mut kept_lines: Vec<String> = Vec::with_capacity(line_count);

    for (index, line) in split_lines(raw).enumerate() {
        let is_first = index == 0;
        let is_last = index + 1 == line_count;
        let tabs_replaced = replace_tabs(line);
        let trimmed = trim_line(&tabs_replaced, is_first, is_last);
        if !trimmed.is_empty() {
            kept_lines.push(trimmed);
        }
    }

    if kept_lines.is_empty() {
        None
    } else {
        Some(kept_lines.join(" "))
    }
}

/// Counts the lines a run splits into, without allocating them.
fn count_lines(raw: &str) -> usize {
    split_lines(raw).count()
}

/// Splits `raw` on `\n`, `\r\n`, and lone `\r`, keeping empty lines and
/// discarding the line-break characters themselves.
fn split_lines(raw: &str) -> impl Iterator<Item = &str> {
    LineSplitter {
        rest: raw,
        done: false,
    }
}

struct LineSplitter<'a> {
    rest: &'a str,
    done: bool,
}

impl<'a> Iterator for LineSplitter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.done {
            return None;
        }
        match self.rest.find(['\n', '\r']) {
            Some(idx) => {
                let line = &self.rest[..idx];
                let after_break_char = &self.rest[idx..];
                let consumed = if after_break_char.starts_with("\r\n") {
                    2
                } else {
                    1
                };
                self.rest = &self.rest[idx + consumed..];
                Some(line)
            }
            None => {
                self.done = true;
                Some(self.rest)
            }
        }
    }
}

/// Replaces every U+0009 tab with a single U+0020 space. Not a trim: an
/// interior tab (`a\tb`) becomes `a b`.
fn replace_tabs(line: &str) -> String {
    line.replace('\t', " ")
}

/// Removes leading spaces unless `is_first`, and trailing spaces unless
/// `is_last`. Only U+0020 is ever removed.
fn trim_line(line: &str, is_first: bool, is_last: bool) -> String {
    let mut s = line;
    if !is_first {
        s = s.trim_start_matches(' ');
    }
    if !is_last {
        s = s.trim_end_matches(' ');
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_keeps_both_edge_spaces() {
        assert_eq!(normalize_jsx_text(" a ").as_deref(), Some(" a "));
    }

    #[test]
    fn single_line_whitespace_only_survives() {
        assert_eq!(normalize_jsx_text(" ").as_deref(), Some(" "));
    }

    #[test]
    fn multiline_collapses_to_one_line() {
        assert_eq!(
            normalize_jsx_text("\n        Hello\n        world\n    ").as_deref(),
            Some("Hello world")
        );
    }

    #[test]
    fn multiline_whitespace_only_yields_nothing() {
        assert_eq!(normalize_jsx_text("\n").as_deref(), None);
    }

    #[test]
    fn blank_line_in_the_middle_is_dropped() {
        assert_eq!(
            normalize_jsx_text("\n        Hello\n\n        world\n    ").as_deref(),
            Some("Hello world")
        );
    }

    #[test]
    fn tab_inside_single_line_becomes_space_not_trim() {
        assert_eq!(normalize_jsx_text("a\tb").as_deref(), Some("a b"));
    }

    #[test]
    fn nbsp_is_not_trimmed_only_ascii_space_is() {
        assert_eq!(
            normalize_jsx_text("\n         \u{a0}\n    ").as_deref(),
            Some("\u{a0}")
        );
    }

    #[test]
    fn crlf_and_lone_cr_are_both_line_breaks() {
        assert_eq!(
            normalize_jsx_text("\r\n        Hello\r        world\r\n    ").as_deref(),
            Some("Hello world")
        );
    }

    #[test]
    fn leading_and_trailing_lines_drop_without_extra_space() {
        assert_eq!(
            normalize_jsx_text("\n        Hello world\n    ").as_deref(),
            Some("Hello world")
        );
    }

    #[test]
    fn text_before_island_keeps_trailing_space_on_one_line() {
        assert_eq!(normalize_jsx_text("Hello ").as_deref(), Some("Hello "));
    }

    #[test]
    fn text_after_island_trims_leading_interior_indentation() {
        assert_eq!(
            normalize_jsx_text("\n        world").as_deref(),
            Some("world")
        );
    }
}
