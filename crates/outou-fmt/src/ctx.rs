//! The settings one [`crate::format_source`] call runs with, resolved once
//! up front from the caller's [`FormatOptions`] and the project the
//! formatted file belongs to, then passed down the whole pipeline.

use std::path::PathBuf;

use crate::error::FormatError;
use crate::project;
use crate::rustfmt_proc;
use crate::FormatOptions;

/// `rustfmt`'s own default `max_width`.
const DEFAULT_MAX_WIDTH: usize = 100;
/// `rustfmt`'s own default `tab_spaces`.
const DEFAULT_TAB_SPACES: usize = 4;

/// What `rustfmt`'s `newline_style` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewlineStyle {
    /// Keep the style of the input file (`rustfmt`'s default).
    Auto,
    /// Always `\n`.
    Unix,
    /// Always `\r\n`.
    Windows,
}

/// Resolved settings for one formatting call.
#[derive(Debug, Clone)]
pub(crate) struct Ctx {
    /// What the caller asked for (program, deadline).
    pub(crate) options: FormatOptions,
    /// The edition passed to `rustfmt` (`--edition`).
    pub(crate) edition: String,
    /// The directory `rustfmt` runs in, so it finds the formatted file's
    /// own `rustfmt.toml`. `None` keeps the process's working directory.
    pub(crate) work_dir: Option<PathBuf>,
    /// `max_width`: the longest a line may be.
    pub(crate) max_width: usize,
    /// `tab_spaces`: spaces per indentation level (`hard_tabs` is always
    /// forced off, see `rustfmt_proc`).
    pub(crate) indent_unit: usize,
    /// `newline_style`.
    pub(crate) newline_style: NewlineStyle,
    /// Columns that the text currently being laid out will be shifted right
    /// by when it is spliced into its final place (the enclosing indentation
    /// of an expression island, which is formatted dedented to column 0).
    pub(crate) base_column: usize,
}

impl Ctx {
    /// Resolves the settings for `options`: the explicit edition if given,
    /// else the one detected from the file's `Cargo.toml`, else 2021; and
    /// the layout settings from the `rustfmt.toml` `rustfmt` discovers.
    pub(crate) fn resolve(options: &FormatOptions) -> Result<Self, FormatError> {
        let mut ctx = Self::plain(options);
        match rustfmt_proc::print_config(&ctx) {
            Ok(text) => ctx.apply_config(&text),
            // The formatting run reports a broken configuration (or a
            // `rustfmt` too old for `--print-config`) with the real message.
            Err(FormatError::RustfmtFailed { .. }) => {}
            Err(other) => return Err(other),
        }
        Ok(ctx)
    }

    /// Settings from `options` alone: no `rustfmt` is run, the layout
    /// settings are `rustfmt`'s defaults.
    pub(crate) fn plain(options: &FormatOptions) -> Self {
        let source_path = options.source_path.as_deref();
        let edition = options
            .edition
            .clone()
            .or_else(|| source_path.and_then(project::detect_edition))
            .unwrap_or_else(|| project::DEFAULT_EDITION.to_string());
        Self {
            edition,
            work_dir: source_path.and_then(project::work_dir_of),
            options: options.clone(),
            max_width: DEFAULT_MAX_WIDTH,
            indent_unit: DEFAULT_TAB_SPACES,
            newline_style: NewlineStyle::Auto,
            base_column: 0,
        }
    }

    /// Reads `max_width`, `tab_spaces` and `newline_style` out of
    /// `rustfmt --print-config` output (`key = value` lines).
    fn apply_config(&mut self, text: &str) {
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            match key.trim() {
                "max_width" => {
                    if let Ok(width) = value.parse() {
                        self.max_width = width;
                    }
                }
                "tab_spaces" => {
                    if let Ok(spaces) = value.parse::<usize>() {
                        if spaces > 0 {
                            self.indent_unit = spaces;
                        }
                    }
                }
                "newline_style" => {
                    self.newline_style = match value {
                        "Unix" => NewlineStyle::Unix,
                        "Windows" => NewlineStyle::Windows,
                        // `Native` is the platform's: `\n` on every platform
                        // the alpha supports (Windows is out of scope).
                        "Native" => NewlineStyle::Unix,
                        _ => NewlineStyle::Auto,
                    };
                }
                _ => {}
            }
        }
    }

    /// A copy whose text will end up `columns` further right than it is laid
    /// out at.
    pub(crate) fn shifted_by(&self, columns: usize) -> Self {
        Self {
            base_column: self.base_column + columns,
            ..self.clone()
        }
    }

    /// Whether `content_len` characters starting at column `column` (within
    /// the text being laid out) stay inside `max_width`.
    pub(crate) fn fits(&self, column: usize, content_len: usize) -> bool {
        self.base_column + column + content_len <= self.max_width
    }
}

impl From<FormatOptions> for Ctx {
    fn from(options: FormatOptions) -> Self {
        Self::plain(&options)
    }
}

impl Default for Ctx {
    fn default() -> Self {
        FormatOptions::default().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_layout_settings_from_print_config_output() {
        let mut ctx = Ctx::default();
        ctx.apply_config(
            "max_width = 60\nhard_tabs = false\ntab_spaces = 2\nnewline_style = \"Windows\"\n",
        );
        assert_eq!(ctx.max_width, 60);
        assert_eq!(ctx.indent_unit, 2);
        assert_eq!(ctx.newline_style, NewlineStyle::Windows);
    }

    #[test]
    fn ignores_unparseable_values_and_a_zero_tab_width() {
        let mut ctx = Ctx::default();
        ctx.apply_config("max_width = wide\ntab_spaces = 0\nnewline_style = \"Auto\"\n");
        assert_eq!(ctx.max_width, 100);
        assert_eq!(ctx.indent_unit, 4);
        assert_eq!(ctx.newline_style, NewlineStyle::Auto);
    }

    #[test]
    fn fits_counts_the_shifted_base_column() {
        let ctx = Ctx::default().shifted_by(10);
        assert!(ctx.fits(40, 50));
        assert!(!ctx.fits(41, 50));
    }
}
