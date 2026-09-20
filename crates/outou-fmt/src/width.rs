//! The line-width budget the JSX pretty printer wraps at.
//!
//! `TODO(phase0)`: read this from the project's `rustfmt.toml`
//! (`max_width`) when one is present and cheap to locate, rather than
//! always using rustfmt's own default. The repository's own
//! `rustfmt.toml` does not set `max_width`, so the default is exactly
//! right today; a project that customizes it would get a formatter that
//! wraps JSX attributes at a different width than the option it set for
//! plain Rust.
//!
//! `TODO(phase0)`: every [`fits`] call site is handed the *line's*
//! leading indent (`crate::snippet::indent_of_line_containing`), not the
//! placeholder's actual column on that line — deliberately, since the
//! placeholder's exact column is not always meaningful (see that
//! function's own doc comment). This is usually a fine approximation,
//! but for a JSX expression placed after other code on the same line
//! (`let long_name = <div class="…" />;`) it undercounts the true
//! starting column, so the single-line check can accept a candidate that
//! actually lands well past [`MAX_WIDTH`] once spliced back in —
//! reproduced with `let some_long_name = <div class="…"/>;` rendering a
//! 131-column line while `fits` still saw it as fitting. A fix would
//! need `crate::snippet` to report the placeholder's own column, not
//! just its line's indent.
pub(crate) const MAX_WIDTH: usize = 100;

/// The indent step used everywhere in the JSX pretty printer, matching
/// rustfmt's own default `tab_spaces`.
pub(crate) const INDENT_UNIT: usize = 4;

/// Whether a single-line candidate of length `content_len` starting at
/// column `indent` fits inside [`MAX_WIDTH`].
pub(crate) fn fits(indent: usize, content_len: usize) -> bool {
    indent + content_len <= MAX_WIDTH
}
