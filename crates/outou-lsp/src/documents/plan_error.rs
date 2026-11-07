//! A failed re-plan, as the editor should see it (#31): the error of the
//! last [`super::Workspace`] re-plan that did not succeed, located on the
//! `mod` item that caused it where the error says which one that was.
//!
//! A failed re-plan keeps the previous plan, so the editor otherwise has no
//! way to learn that the module graph it is typing into does not resolve
//! (`mod x;` with no `x.rsx`). [`crate::diagnostics::publish_for_rsx`] merges
//! this report with the file's other diagnostics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lsp_types::Range;
use outou_cli::build::plan::PlanError;
use outou_modules::ModuleError;
use outou_sourcemap::LineIndex;
use outou_syntax::Span;

use super::workspace::LoadError;
use crate::mapping;

/// The last failed re-plan: what to say, and which file to say it on.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanErrorReport {
    /// Outou's wording of the error, the same text `outou build` prints.
    pub message: String,
    /// The file (normalized URI string) the diagnostic is shown on: the
    /// declaring file, which is a plain `.rs` file when that is where the
    /// `mod` item is.
    pub file_uri: String,
    /// Where in that file: the declaring `mod` item when the error names
    /// it, else the start of the file.
    pub range: Range,
}

impl PlanErrorReport {
    /// Reports `error`, found while planning the crate at `manifest_dir`.
    /// `overlay` is the buffer text the failed plan used (open documents);
    /// any other file is read from disk, as the planner did.
    ///
    /// Errors that do not name a `mod` item (two crate roots, an unreadable
    /// manifest directory, a plain Rust file declaring a `.rsx` module, a
    /// unit that cannot be read) are crate-wide: they are shown at the top
    /// of `crate_root`, the crate's `.rsx` root file, so they are neither
    /// lost nor tied to whichever file the user happened to be editing.
    /// `None` only when there is no root file to show them on.
    pub(super) fn new(
        error: &LoadError,
        manifest_dir: &Path,
        crate_root: Option<&Path>,
        overlay: &HashMap<PathBuf, String>,
    ) -> Option<Self> {
        let message = error.to_string();
        let (file, span) = match module_error_site(error) {
            Some((declared_in, span)) => (manifest_dir.join(declared_in), Some(span)),
            None => (crate_root?.to_path_buf(), None),
        };
        let range = span
            .and_then(|span| {
                let text = super::workspace::overlay_text_for(overlay, &file)
                    .map(str::to_string)
                    .or_else(|| std::fs::read_to_string(&file).ok())?;
                let span = skip_leading_trivia(&text, span);
                Some(mapping::to_lsp_range(
                    LineIndex::new(&text).span_to_range(span),
                ))
            })
            .unwrap_or_default();
        let file_uri = outou_sourcemap::file_uri(&file).as_str().to_string();
        Some(Self {
            message,
            file_uri,
            range,
        })
    }
}

/// `span` with its start moved past the whitespace and comments the parser
/// attaches to an item: a `ModuleError` span begins where the previous item
/// ended, not at the `mod` keyword, and the diagnostic should start on the
/// declaration itself.
fn skip_leading_trivia(text: &str, span: Span) -> Span {
    let end = (span.end as usize).min(text.len());
    let mut start = (span.start as usize).min(end);
    loop {
        let rest = text.get(start..end).unwrap_or_default();
        let trimmed = rest.trim_start();
        start += rest.len() - trimmed.len();
        let comment_len = if trimmed.starts_with("//") {
            trimmed.find('\n').map(|newline| newline + 1)
        } else if trimmed.starts_with("/*") {
            trimmed.find("*/").map(|close| close + 2)
        } else {
            None
        };
        match comment_len {
            Some(len) => start += len,
            None => return Span::new(start as u32, end as u32),
        }
    }
}

/// The declaring file (relative to the crate directory) and `mod` item span
/// of a module-resolution error. Exhaustive on purpose: a new
/// [`ModuleError`] variant must decide where it is shown.
fn module_error_site(error: &LoadError) -> Option<(&Path, Span)> {
    let LoadError::Plan(PlanError::Module(error)) = error else {
        return None;
    };
    match error {
        ModuleError::Ambiguous {
            declared_in, span, ..
        }
        | ModuleError::NotFound {
            declared_in, span, ..
        }
        | ModuleError::Io {
            declared_in, span, ..
        }
        | ModuleError::Circular {
            declared_in, span, ..
        }
        | ModuleError::TooDeep {
            declared_in, span, ..
        }
        | ModuleError::ConditionalPath {
            declared_in, span, ..
        }
        | ModuleError::OutsideCrate {
            declared_in, span, ..
        } => Some((declared_in.as_path(), *span)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_whitespace_and_comments_are_skipped_but_not_the_declaration() {
        let text = "fn f() {}\n\n// about x\n/* more */\nmod x;\n";
        let start = text.find("\n\n// about").unwrap() as u32;
        let end = text.len() as u32;
        let skipped = skip_leading_trivia(text, Span::new(start, end));
        assert_eq!(
            &text[skipped.start as usize..skipped.end as usize],
            "mod x;\n"
        );
    }

    #[test]
    fn an_all_trivia_span_does_not_run_past_its_end() {
        let skipped = skip_leading_trivia("  // x", Span::new(0, 6));
        assert!(skipped.start <= skipped.end);
    }
}
