//! The recovering parser.
//!
//! `.rsx` is Rust plus the JSX expression (grammar §1). Outside function
//! bodies, this parser only needs to find `fn` and `mod` items — JSX
//! cannot appear anywhere else — so everything else at item level is kept
//! as an opaque, verbatim [`ast::Item::Rust`] slice ([`item`]). Inside a
//! function body, [`region`] scans for JSX starts using
//! [`crate::lexer::position`] (expression position, grammar §4 rule 1) and
//! [`crate::lexer::disambiguate`] (rules 2–3), splicing [`ast::Expr::Rust`]
//! and [`ast::Expr::Jsx`] nodes at the boundaries. [`jsx`] parses a
//! committed JSX expression itself, with the recovery grammar §2.1 and §9
//! describe.
//!
//! The parser never panics: every method here either makes progress (the
//! byte position strictly increases) or is at end of input, and every
//! malformed shape becomes an [`ast::ErrorNode`] or a degraded
//! [`ast::JsxTag::Incomplete`] plus a [`crate::Diagnostic`], never a
//! `panic!`, `unwrap()` on attacker-controlled data, or an out-of-bounds
//! index.

mod boundary;
mod diag;
mod item;
mod jsx;
mod region;

use outou_sourcemap::Span;

use crate::{ast, Diagnostic, Parsed, Severity};

pub use item::attribute_meta_path;

/// Parses one `.rsx` source text into a [`Parsed`] result. See the module
/// doc; this never panics.
pub fn parse(source: &str) -> Parsed {
    let mut parser = Parser::new(source);
    let file = parser.parse_file();
    Parsed {
        file,
        diagnostics: parser.diagnostics,
        source: source.to_string(),
    }
}

/// Shared parser state. Methods are split across [`item`], [`region`] and
/// [`jsx`] by what they parse, not by type — all three `impl` blocks are
/// for this one struct.
struct Parser<'s> {
    source: &'s str,
    bytes: &'s [u8],
    diagnostics: Vec<Diagnostic>,
    /// Stack of currently-open JSX element names and the span of their
    /// opening `<`, used to resolve `</Name>` against grammar §2.1's four
    /// cases (exact match, deeper match, no match, and — implicitly, by
    /// this stack simply being non-empty — "something is open").
    open_names: Vec<(String, Span)>,
    /// Current JSX element recursion depth, guarded against
    /// [`diag::MAX_NESTING`] (decision D4, grammar §9) so that
    /// pathologically deep input is diagnosed instead of overflowing the
    /// stack (H7).
    jsx_depth: u32,
    /// Current inline-`mod` recursion depth, guarded the same way.
    mod_depth: u32,
    /// Byte position of the last `}` terminator (grammar §2.2) whose
    /// "innermost open construct" diagnostic has already been reported.
    ///
    /// A single terminating `}` is discovered once, deep in the call
    /// stack (inside a tag's attribute scan, a closing tag's scan, or a
    /// content scan), but — because a cut-short nested element returns to
    /// its caller as if resolved, rather than propagating a "this was a
    /// terminator" signal — every ancestor's own content-scanning loop
    /// independently re-examines that same, unconsumed `}` on its very
    /// next iteration. Recording the position here after the first such
    /// diagnostic lets `parse_jsx_children`'s own `}` handling recognize a
    /// re-examination of the *same* byte and skip emitting a second,
    /// redundant [`diag::stray_rbrace_in_text`] diagnostic for it, while
    /// still reporting each ancestor's own legitimate "missing closing
    /// tag" (grammar §9's "one diagnostic for the innermost open
    /// construct").
    terminator_diagnosed_at: Option<usize>,
    /// How many unclosed function bodies this parse has cut at the next
    /// item so far, bounded by [`diag::MAX_RECOVERY_CUTS`].
    recovery_cuts: u32,
}

/// See [`Parser::recovery_snapshot`].
struct RecoverySnapshot {
    diagnostics_len: usize,
    open_names: Vec<(String, Span)>,
    jsx_depth: u32,
    mod_depth: u32,
    terminator_diagnosed_at: Option<usize>,
}

impl<'s> Parser<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            diagnostics: Vec::new(),
            open_names: Vec::new(),
            jsx_depth: 0,
            mod_depth: 0,
            terminator_diagnosed_at: None,
            recovery_cuts: 0,
        }
    }

    /// The mutable parse state a speculative re-parse of one item must
    /// restore (see `item::Parser::parse_function_bounded`).
    fn recovery_snapshot(&self) -> RecoverySnapshot {
        RecoverySnapshot {
            diagnostics_len: self.diagnostics.len(),
            open_names: self.open_names.clone(),
            jsx_depth: self.jsx_depth,
            mod_depth: self.mod_depth,
            terminator_diagnosed_at: self.terminator_diagnosed_at,
        }
    }

    fn restore_recovery_snapshot(&mut self, snapshot: RecoverySnapshot) {
        self.diagnostics.truncate(snapshot.diagnostics_len);
        self.open_names = snapshot.open_names;
        self.jsx_depth = snapshot.jsx_depth;
        self.mod_depth = snapshot.mod_depth;
        self.terminator_diagnosed_at = snapshot.terminator_diagnosed_at;
    }

    fn push_diag(&mut self, span: Span, message: String) {
        self.diagnostics.push(Diagnostic {
            span,
            message,
            severity: Severity::Error,
        });
    }

    fn ident_at(&self, start: usize, end: usize) -> ast::Ident {
        ast::Ident {
            span: Span::new(start as u32, end as u32),
            name: self.source[start..end].to_string(),
        }
    }
}
