//! JSX element, tag, attribute and children parsing, with recovery.
//!
//! This is the "parser-driven lexer" the grammar describes (§4 rule 1):
//! everything here runs after [`crate::lexer::disambiguate`] has already
//! committed a `<` to JSX. From that point on this module owns the
//! open-tag-name stack ([`Parser::open_names`]) that grammar §2.1's
//! closing-tag matching rules need, and attaches every [`crate::Diagnostic`]
//! at the span [`crate::parser::diag`] and grammar §9 specify.
//!
//! Split by concern, to keep any one file well under the project's 800-line
//! limit: this file holds the entry point and the small pieces shared
//! across the others; [`tag`] parses an opening tag and its attributes;
//! [`children`] parses an element's children (including a nested child
//! element); [`closing`] resolves a `</Name>` against the open-tag-name
//! stack. All four are still, in effect, one `impl Parser` — Rust allows
//! multiple `impl` blocks for the same type, and method visibility here
//! follows plain module-privacy (a private item defined in this module is
//! visible to `tag`, `children` and `closing` as its descendants).
//!
//! # Two ways an element finishes
//!
//! [`children::ChildrenOutcome::Resolved`]: the element got *some* close —
//! a real `/>`, a matching `</Name>`, a mismatched `</Other>` (diagnosed,
//! treated as this element's close), or a `</Name>` that turned out to
//! belong to an ancestor (diagnosed as *this* element's missing close, left
//! unconsumed so the ancestor sees it fresh). In every `Resolved` case the
//! caller's own scanning loop continues normally from the returned
//! position.
//!
//! [`children::ChildrenOutcome::Terminated`]: end of input, or a bare `}`
//! with no island open (grammar §2.2), cut the element short. The position
//! is left unconsumed at the terminator; every enclosing element on the
//! call stack must also stop and propagate `Terminated` upward, each
//! adding its own "missing closing tag" diagnostic — except the innermost
//! one, which already reported the specific interruption ("unexpected end
//! of file inside tag …", "… inside closing tag …", or the island's own
//! message).

mod children;
mod closing;
mod tag;

use outou_sourcemap::Span;

use crate::ast;
use crate::lexer::disambiguate::{Classification, NamedTag};
use crate::lexer::rust_token::next_significant;

use super::diag;
use super::Parser;

/// See the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildrenOutcome {
    Resolved,
    Terminated,
}

/// How a closing tag was resolved against [`Parser::open_names`].
/// [`closing`] produces this; [`children`] consumes it.
enum CloseResolution {
    Resolved {
        close: Option<ast::JsxTag>,
        end: usize,
    },
    Terminated,
}

fn skip_ws(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(bytes.get(pos), Some(b) if b.is_ascii_whitespace()) {
        pos += 1;
    }
    pos
}

/// Whether `c` can start a `JsxName` (grammar §5) — the guard used before
/// scanning an attribute or closing-tag name at the current position.
/// Delegates to [`crate::lexer::is_jsx_name_start_byte`] so a leading digit
/// (M11) is rejected here too, not just inside the scanner itself.
fn is_jsx_name_byte(c: u8) -> bool {
    crate::lexer::is_jsx_name_start_byte(c)
}

fn pos_span(pos: usize) -> Span {
    Span::new(pos as u32, pos as u32 + 1)
}

fn incomplete_element(lt_pos: usize, end: usize, expected: &str) -> ast::JsxElement {
    ast::JsxElement {
        span: Span::new(lt_pos as u32, end as u32),
        open: ast::JsxTag::Incomplete(ast::IncompleteTag {
            span: Span::new(lt_pos as u32, end as u32),
            name: None,
            missing: ast::MissingToken {
                at: end as u32,
                description: expected.to_string(),
            },
        }),
        attributes: Vec::new(),
        children: Vec::new(),
        close: None,
        errors: Vec::new(),
    }
}

impl<'s> Parser<'s> {
    /// Parses one JSX expression starting at `lt_pos` (already classified
    /// by [`disambiguate::classify`]). Returns the element and the
    /// position right after it.
    pub(super) fn parse_jsx_element(
        &mut self,
        lt_pos: usize,
        classification: Classification,
    ) -> (ast::JsxElement, usize) {
        match classification {
            Classification::Rust => unreachable!("caller only calls this for a JSX classification"),
            Classification::Fragment { lt } => self.recover_fragment(lt),
            Classification::StrayClose { lt } => self.recover_stray_close(lt),
            Classification::Named(tag) => self.parse_named_element(lt_pos, tag),
        }
    }

    /// `<>`: reserved in Phase 0 (grammar §10). Recovery: consume just the
    /// two characters and report a placeholder element.
    fn recover_fragment(&mut self, lt: usize) -> (ast::JsxElement, usize) {
        let gt = next_significant(self.bytes, lt + 1);
        self.push_diag(
            Span::new(lt as u32, lt as u32 + 1),
            diag::fragments_not_supported(),
        );
        let end = if gt.start == gt.end { lt + 1 } else { gt.end };
        (incomplete_element(lt, end, "a tag name"), end)
    }

    /// `</Name>` where nothing is open (grammar §4.1, §9).
    fn recover_stray_close(&mut self, lt: usize) -> (ast::JsxElement, usize) {
        let (resolution, end) = self.scan_closing_tag_shape(lt);
        let name = resolution.name_text(self.source).unwrap_or_default();
        self.push_diag(
            Span::new(lt as u32, lt as u32 + 1),
            diag::stray_closing_tag(&name),
        );
        (incomplete_element(lt, end, "an opening tag"), end)
    }

    /// Parses a named JSX element, guarding against unbounded recursion
    /// (decision D4, H7): every recursive descent into a child element
    /// goes through this one entry point ([`Parser::parse_nested_child_element`]
    /// calls it, and it calls itself indirectly through
    /// [`Parser::parse_jsx_children`]), so counting entries here bounds the
    /// whole element-nesting call chain.
    fn parse_named_element(&mut self, lt_pos: usize, tag: NamedTag) -> (ast::JsxElement, usize) {
        if self.jsx_depth >= diag::MAX_NESTING {
            return self.recover_too_deeply_nested_element(lt_pos, tag);
        }
        self.jsx_depth += 1;
        let result = self.parse_named_element_at_depth(lt_pos, tag);
        self.jsx_depth -= 1;
        result
    }

    /// Grammar §9's nesting-cap recovery: diagnose once, then skip to end
    /// of input rather than attempting to locate this element's true
    /// matching close. Finding that close would itself require walking the
    /// remaining (still pathologically nested) structure — effectively the
    /// same recursion this cap exists to avoid — so end of input is used as
    /// the terminator (grammar §2.2 already treats end of input as a valid
    /// JSX terminator). The enclosing elements each still get their own
    /// "missing closing tag" diagnostic as the call stack unwinds normally.
    fn recover_too_deeply_nested_element(
        &mut self,
        lt_pos: usize,
        _tag: NamedTag,
    ) -> (ast::JsxElement, usize) {
        self.push_diag(
            Span::new(lt_pos as u32, lt_pos as u32 + 1),
            diag::jsx_nested_too_deeply(),
        );
        let end = self.bytes.len();
        (
            incomplete_element(lt_pos, end, "fewer nested elements"),
            end,
        )
    }

    fn push_diag_at(&mut self, span: Span, message: String) {
        self.push_diag(span, message);
    }
}
