//! Cleaning the text of a diagnostic: no generated-directory path, no
//! backend vocabulary.
//!
//! rustc prints file names inside its prose as well as in spans: a closure
//! type is `{closure@app/src/.generated/task_list.rs:15:14: 15:22}`,
//! `include_str!` fails with ``couldn't read `app/src/.generated/x.txt` ``.
//! Both name a file nobody wrote, so every piece of text a user can see
//! (message, label, note) goes through [`Context::clean`].
//!
//! - `path:L:C` (and `path:L:C: L:C`) into a generated file is mapped to
//!   the `.rsx` position through the unit's source map; when that is not
//!   possible it becomes `<generated code>`.
//! - A bare path into the generated directory has its prefix replaced by
//!   `<generated directory>/`, and [`Context::hid_generated_path`] is set
//!   so the caller can add [`GENERATED_DIRECTORY_NOTE`]: the path is
//!   real (`include_str!` resolves relative paths against it, ADR 0009)
//!   but names a directory the user never sees.

use std::path::Path;

use outou_sourcemap::Span;
use outou_syntax::vocabulary;

use super::locate::map_generated_span;
use super::render::char_line_col;
use super::translate::{Resolved, Resolver};

/// Path fragment of a generated directory.
pub(super) const GENERATED_DIR: &str = ".generated";

/// What replaces the directory prefix of a path into generated code.
const GENERATED_DIRECTORY_PLACEHOLDER: &str = "<generated directory>";

/// What replaces a position in generated code that cannot be mapped.
const GENERATED_CODE_PLACEHOLDER: &str = "<generated code>";

/// Added once to a diagnostic whose text named the generated directory.
pub(super) const GENERATED_DIRECTORY_NOTE: &str = "relative paths in `include_str!` and `include_bytes!` are resolved relative to the directory of the generated Rust, not the directory of the `.rsx` file";

/// `src/.generated/../state.rs` to `src/state.rs`: a plain `.rs` module
/// reached through the generated directory's `#[path]`.
pub fn normalize_generated_paths(text: &str) -> String {
    text.replace("/.generated/../", "/")
}

/// `text` unchanged when it has no backend vocabulary that
/// [`vocabulary::translate_message`] would rewrite or that names the
/// storage cell of a `State` (issue #31: a note such as "required for
/// `&'static UnsyncStorage` to implement `Send`"), `None` otherwise.
pub(super) fn backend_free(text: &str) -> Option<String> {
    (vocabulary::translate_message(text) == text && !vocabulary::names_state_storage(text))
        .then(|| text.to_string())
}

/// What cleaning a text needs: where file names are relative to, and how
/// to find the source map of a generated file.
pub(super) struct Context<'a> {
    pub(super) manifest: &'a Path,
    pub(super) resolver: &'a mut dyn Resolver,
    /// Set when a path into the generated directory was hidden.
    pub(super) hid_generated_path: bool,
}

impl<'a> Context<'a> {
    pub(super) fn new(manifest: &'a Path, resolver: &'a mut dyn Resolver) -> Self {
        Self {
            manifest,
            resolver,
            hid_generated_path: false,
        }
    }

    pub(super) fn resolve(&mut self, file_name: &str) -> Resolved {
        self.resolver.resolve(file_name, self.manifest)
    }

    /// `text` with generated paths mapped or hidden (see the module docs).
    pub(super) fn clean(&mut self, text: &str) -> String {
        let text = normalize_generated_paths(text);
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(marker) = find_marker(rest) {
            let start = path_start(rest, marker);
            let end = path_end(rest, marker);
            out.push_str(&rest[..start]);
            let path = &rest[start..end];
            rest = &rest[end..];
            match parse_positions(rest) {
                Some((positions, consumed)) => {
                    out.push_str(&self.map_positions(path, positions));
                    rest = &rest[consumed..];
                }
                None => out.push_str(&self.hide_path(path)),
            }
        }
        out.push_str(rest);
        out
    }

    /// [`backend_free`], then [`Context::clean`].
    pub(super) fn clean_backend_free(&mut self, text: &str) -> Option<String> {
        backend_free(text).map(|text| self.clean(&text))
    }

    fn hide_path(&mut self, path: &str) -> String {
        self.hid_generated_path = true;
        let Some(marker) = find_marker(path) else {
            return path.to_string();
        };
        let after = &path[marker + GENERATED_DIR.len()..];
        format!("{GENERATED_DIRECTORY_PLACEHOLDER}{after}")
    }

    /// `path:L:C[: L:C]` in generated code, as `file.rsx:L:C[: L:C]`.
    fn map_positions(&mut self, path: &str, positions: Positions) -> String {
        self.try_map_positions(path, positions)
            .unwrap_or_else(|| GENERATED_CODE_PLACEHOLDER.to_string())
    }

    fn try_map_positions(&mut self, path: &str, positions: Positions) -> Option<String> {
        let Resolved::Unit(unit) = self.resolve(path) else {
            return None;
        };
        let start = offset_of(&unit.generated_text, positions.start)?;
        let end = match positions.end {
            Some(end) => offset_of(&unit.generated_text, end)?.max(start),
            None => start,
        };
        let mapped = map_generated_span(&unit.source_map, Span::new(start, end))?;
        let file = unit.sources.get(mapped.source.0)?;
        let (line, column) = char_line_col(&file.text, mapped.span.start);
        let mut out = format!("{}:{line}:{column}", file.display);
        if positions.end.is_some() {
            let (end_line, end_column) = char_line_col(&file.text, mapped.span.end);
            out.push_str(&format!(": {end_line}:{end_column}"));
        }
        Some(out)
    }
}

/// A `line:column` pair, 1-based, columns in characters.
type LineColumn = (usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Positions {
    start: LineColumn,
    end: Option<LineColumn>,
}

/// Byte offset of a 1-based line and character column in `text`.
fn offset_of(text: &str, (line, column): LineColumn) -> Option<u32> {
    let mut line_start = 0;
    for _ in 1..line {
        line_start += text[line_start..].find('\n')? + 1;
    }
    let rest = &text[line_start..];
    let line_text = rest.split('\n').next().unwrap_or("");
    let in_line = match line_text.char_indices().nth(column.checked_sub(1)?) {
        Some((index, _)) => index,
        None if line_text.chars().count() + 1 == column => line_text.len(),
        None => return None,
    };
    u32::try_from(line_start + in_line).ok()
}

/// Whether `ch` ends a path in prose.
fn is_path_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '`' | '\'' | '"' | '(' | ')' | '{' | '}' | '@' | ',' | ';'
        )
}

/// Byte index of the next `.generated` that is a whole path component.
fn find_marker(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = text[from..].find(GENERATED_DIR) {
        let at = from + found;
        let after = text[at + GENERATED_DIR.len()..].chars().next();
        if after.is_none_or(|ch| ch == '/' || is_path_boundary(ch) || ch == ':') {
            return Some(at);
        }
        from = at + GENERATED_DIR.len();
    }
    None
}

fn path_start(text: &str, marker: usize) -> usize {
    text[..marker]
        .char_indices()
        .rev()
        .find(|(_, ch)| is_path_boundary(*ch))
        .map_or(0, |(index, ch)| index + ch.len_utf8())
}

/// End of the path: a boundary, or the `:` of a following `:L:C`.
fn path_end(text: &str, marker: usize) -> usize {
    let mut chars = text[marker..].char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let digit_follows = chars.peek().is_some_and(|(_, next)| next.is_ascii_digit());
        if is_path_boundary(ch) || (ch == ':' && digit_follows) {
            return marker + index;
        }
    }
    text.len()
}

/// `:L:C` or `:L:C: L:C` at the start of `text`, with its byte length.
fn parse_positions(text: &str) -> Option<(Positions, usize)> {
    let (start, consumed) = parse_pair(text)?;
    if let Some(after) = text[consumed..].strip_prefix(": ") {
        if let Some((end, used)) = parse_bare_pair(after) {
            let positions = Positions {
                start,
                end: Some(end),
            };
            return Some((positions, consumed + 2 + used));
        }
    }
    Some((Positions { start, end: None }, consumed))
}

/// `:L:C` with its leading colon.
fn parse_pair(text: &str) -> Option<(LineColumn, usize)> {
    let (pair, used) = parse_bare_pair(text.strip_prefix(':')?)?;
    Some((pair, used + 1))
}

/// `L:C`.
fn parse_bare_pair(text: &str) -> Option<(LineColumn, usize)> {
    let (line, after_line) = take_number(text)?;
    let after_colon = after_line.strip_prefix(':')?;
    let (column, rest) = take_number(after_colon)?;
    Some(((line, column), text.len() - rest.len()))
}

fn take_number(text: &str) -> Option<(usize, &str)> {
    let digits = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    let number = text[..digits].parse().ok()?;
    Some((number, &text[digits..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_must_be_a_whole_path_component() {
        assert_eq!(find_marker("a/.generated/b.rs"), Some(2));
        assert_eq!(find_marker("a/.generated_x/b.rs"), None);
        assert_eq!(find_marker("no such"), None);
    }

    #[test]
    fn positions_are_read_with_and_without_an_end() {
        assert_eq!(
            parse_positions(":15:14: 15:22}"),
            Some((
                Positions {
                    start: (15, 14),
                    end: Some((15, 22))
                },
                13
            ))
        );
        assert_eq!(
            parse_positions(":3:4`: x"),
            Some((
                Positions {
                    start: (3, 4),
                    end: None
                },
                4
            ))
        );
        assert_eq!(parse_positions("`: x"), None);
    }

    #[test]
    fn a_line_and_column_become_a_byte_offset() {
        let text = "ab\ncafé x\n";
        assert_eq!(offset_of(text, (1, 1)), Some(0));
        assert_eq!(offset_of(text, (2, 6)), Some(9));
        assert_eq!(offset_of(text, (9, 1)), None);
    }
}
