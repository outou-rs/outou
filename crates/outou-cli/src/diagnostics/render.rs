//! The text `outou cargo` prints for a [`Translated`] diagnostic.
//!
//! ```text
//! error[E0308]: mismatched types
//!   --> app/src/task_list.rsx:34:22
//!    |
//! 34 |     let wrong: u32 = "oops";
//!    |                      ^^^^^^ expected `u32`, found `&str`
//!    = note: expected due to this (app/src/task_list.rsx:34:16)
//! ```
//!
//! The UI harness keeps using the compact format of
//! [`outou_syntax::render::render_diagnostics`]: its `expected.stderr`
//! files predate this renderer.

use super::translate::{Location, Translated};

const RED: &str = "\x1b[1;31m";
const YELLOW: &str = "\x1b[1;33m";
const BLUE: &str = "\x1b[1;34m";
const RESET: &str = "\x1b[0m";

/// 1-based `(line, column)` of byte `offset` in `text`, counting columns
/// in characters like `outou_syntax::render` does. An offset past the end
/// is clamped; one inside a character is walked down to its start.
pub(crate) fn char_line_col(text: &str, offset: u32) -> (usize, usize) {
    let offset = (offset as usize).min(text.len());
    let mut line = 1;
    let mut col = 1;
    for (index, ch) in text.char_indices() {
        if index >= offset {
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

/// The text of the line that holds byte `offset`, without its newline.
fn line_at(text: &str, offset: u32) -> &str {
    let offset = (offset as usize).min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    text[start..end].trim_end_matches('\r')
}

/// Renders `translated` for a terminal. `color` adds ANSI colour to the
/// severity and the carets; it is off when output is not a terminal.
pub fn render_translated(translated: &Translated, color: bool) -> String {
    if let Some(rendered) = &translated.rustc_rendered {
        let text = if color {
            rendered.clone()
        } else {
            strip_ansi(rendered)
        };
        return text.trim_end().to_string() + "\n";
    }

    let mut out = String::new();
    let (severity, paint) = match translated.level.as_str() {
        "warning" => ("warning", YELLOW),
        "failure-note" => ("note", BLUE),
        _ => ("error", RED),
    };
    let paint = |text: &str| {
        if color {
            format!("{paint}{text}{RESET}")
        } else {
            text.to_string()
        }
    };
    let code = translated
        .code
        .as_deref()
        .filter(|code| code.starts_with('E') && code[1..].chars().all(|c| c.is_ascii_digit()))
        .map(|code| format!("[{code}]"))
        .unwrap_or_default();
    out.push_str(&paint(&format!("{severity}{code}")));
    out.push_str(": ");
    out.push_str(&translated.diagnostic.message);
    out.push('\n');

    let label = translated.label.as_deref().unwrap_or("");
    match &translated.location {
        Location::Source {
            unit,
            source,
            span,
            coarse,
        } => {
            if let Some(file) = unit.sources.get(source.0) {
                let (line, column) = char_line_col(&file.text, span.start);
                let text = line_at(&file.text, span.start);
                let width = if *coarse || span.end <= span.start {
                    1
                } else {
                    let end = span.end.min(span.start + text.len() as u32);
                    let (_, end_col) = char_line_col(&file.text, end);
                    let on_one_line =
                        !file.text[span.start as usize..span.end as usize].contains('\n');
                    if on_one_line {
                        end_col.saturating_sub(column).max(1)
                    } else {
                        text.chars().count().saturating_sub(column - 1).max(1)
                    }
                };
                snippet(
                    &mut out,
                    &Snippet {
                        path: &file.display,
                        line,
                        column,
                        text,
                        width,
                        label,
                    },
                    &paint,
                );
            }
        }
        Location::Plain {
            path,
            line,
            column,
            end_column,
            line_text,
        } => {
            let text = line_text.as_deref().unwrap_or("");
            let width = end_column
                .map(|end| end.saturating_sub(*column).max(1))
                .unwrap_or_else(|| text.chars().count().saturating_sub(column - 1).max(1));
            snippet(
                &mut out,
                &Snippet {
                    path,
                    line: *line,
                    column: *column,
                    text,
                    width,
                    label,
                },
                &paint,
            );
        }
        Location::None => {}
    }

    let gutter = " ".repeat(gutter_width(&translated.location));
    for note in &translated.notes {
        let mut lines = note.text.lines();
        out.push_str(&format!(
            "{gutter} = {}: {}\n",
            note.level,
            lines.next().unwrap_or("")
        ));
        for continuation in lines {
            out.push_str(&format!("{gutter}   {continuation}\n"));
        }
    }
    out
}

/// Width of the line-number gutter for the location (at least 1).
fn gutter_width(location: &Location) -> usize {
    match location {
        Location::Source {
            unit, source, span, ..
        } => unit.sources.get(source.0).map_or(1, |file| {
            char_line_col(&file.text, span.start).0.to_string().len()
        }),
        Location::Plain { line, .. } => line.to_string().len(),
        Location::None => 1,
    }
}

/// One source line with carets under the diagnosed columns.
struct Snippet<'a> {
    path: &'a str,
    line: usize,
    column: usize,
    text: &'a str,
    width: usize,
    label: &'a str,
}

fn snippet(out: &mut String, snippet: &Snippet<'_>, paint: &dyn Fn(&str) -> String) {
    let Snippet {
        path,
        line,
        column,
        text,
        width,
        label,
    } = *snippet;
    let number = line.to_string();
    let gutter = " ".repeat(number.len());
    out.push_str(&format!("{gutter}--> {path}:{line}:{column}\n"));
    if text.is_empty() {
        return;
    }
    out.push_str(&format!("{gutter} |\n"));
    out.push_str(&format!("{number} | {text}\n"));
    let carets = paint(&"^".repeat(width));
    let label = if label.is_empty() {
        String::new()
    } else {
        format!(" {label}")
    };
    out.push_str(&format!(
        "{gutter} | {}{carets}{label}\n",
        " ".repeat(column - 1)
    ));
}

/// Removes ANSI colour escapes (`ESC [ ... m`).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_count_characters() {
        let text = "café\nx";
        assert_eq!(char_line_col(text, text.find('x').unwrap() as u32), (2, 1));
        assert_eq!(char_line_col("ab", 1), (1, 2));
    }

    #[test]
    fn a_line_is_found_by_offset() {
        assert_eq!(line_at("one\ntwo\nthree", 5), "two");
        assert_eq!(line_at("one\r\ntwo", 1), "one");
    }

    #[test]
    fn ansi_escapes_are_stripped() {
        assert_eq!(strip_ansi("\x1b[1;31merror\x1b[0m: x"), "error: x");
    }
}
