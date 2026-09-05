//! Mode-aware lexing primitives.
//!
//! The lexer switches between three conceptual modes while parsing a
//! `.rsx` file:
//!
//! ```text
//! Rust ──(JSX start, §4)──▶ JsxTag ──(>)──▶ JsxText ──({)──▶ Rust
//!   ▲                                          ▲               │
//!   └──────────────────(matching })────────────┘◀───────────────┘
//! ```
//!
//! Rust macro token trees and attribute bodies are lexed as opaque Rust
//! token trees; the lexer never enters a JSX mode inside them (grammar §3).
//!
//! There is exactly one compiler (`AGENTS.md`), so this mode machine has
//! exactly one implementation: `crate::parser` drives these mode
//! transitions directly, token by token, via [`disambiguate`], [`position`],
//! [`opaque`] and [`rust_token`], because grammar §4 rule 1 makes mode
//! transitions the *parser's* knowledge, not a lexer's ("expression
//! position is the parser's knowledge, not the lexer's; the lexer is
//! parser-driven"). A closing tag's name must also be checked against the
//! parser's open-tag-name stack (grammar §2.1), which is itself a small
//! parse, not a lexical fact. A free-standing, context-free tokenizer
//! (`tokenize`) existed here early in Phase 0 as an independently testable
//! view of the mode/brace/angle plumbing, but nothing in the workspace
//! called it, it could not perform the parser's own closing-tag matching,
//! and it duplicated (and could drift from) the recovering parser's
//! decisions — so it was deleted (issue #4, decision D5) rather than kept
//! in sync with no consumer. If a token stream is ever needed (issue #14,
//! droppable in Phase 0), it should be emitted as a by-product of the
//! parser itself, not a second, independent lexer.
//!
//! What remains here are the primitives every consumer of this module
//! actually needs: [`scan_jsx_name`] (a `JsxName` scan shared by the
//! disambiguation rules and the parser's tag/attribute/closing-tag
//! scanning) and the four submodules below.

pub mod disambiguate;
pub mod opaque;
pub mod position;
pub mod rust_token;

/// Whether `b` can *start* an identifier segment: a Rust `IDENTIFIER`
/// never starts with a digit (M11). Non-ASCII bytes (`>= 0x80`) are
/// accepted the same way [`rust_token`]'s own identifier scanner accepts
/// them: Outou never needs to validate Unicode XID precisely (a
/// deliberate `TODO(phase0)` — see [`scan_jsx_name`]'s doc).
pub(crate) fn is_jsx_name_start_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic() || b >= 0x80
}

/// Whether `b` can *continue* an identifier segment once started (a digit
/// is fine here, just not as the first byte).
fn is_jsx_name_continue_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Scans one `IDENTIFIER_OR_KEYWORD` segment starting at `pos`: an
/// optional raw-identifier `r#` prefix (Rust's own raw-identifier syntax,
/// which a `JsxName` may use the same way an ordinary Rust identifier
/// does, M11), followed by a byte that can start an identifier and zero
/// or more that can continue one. Returns `pos` unchanged (no progress)
/// if there is no valid segment here — in particular, if the first byte
/// is a digit.
fn scan_jsx_name_segment(bytes: &[u8], pos: usize) -> usize {
    let start = pos;
    let mut p = pos;
    if bytes.get(p) == Some(&b'r')
        && bytes.get(p + 1) == Some(&b'#')
        && matches!(bytes.get(p + 2), Some(b) if is_jsx_name_start_byte(*b))
    {
        p += 2;
    }
    if !matches!(bytes.get(p), Some(b) if is_jsx_name_start_byte(*b)) {
        return start;
    }
    p += 1;
    while matches!(bytes.get(p), Some(b) if is_jsx_name_continue_byte(*b)) {
        p += 1;
    }
    p
}

/// A `JsxName` is `IDENTIFIER_OR_KEYWORD ('-' IDENTIFIER_OR_KEYWORD)*`
/// (grammar §5), where `IDENTIFIER_OR_KEYWORD` follows Rust's own
/// identifier grammar (M11): it may start with `r#` (a raw identifier,
/// `r#type`) but never with a digit. This scanner accepts the same byte
/// classes for every segment; it does not attempt to distinguish a Rust
/// keyword from an ordinary identifier, since both are valid `JsxName`
/// segments. Shared by [`disambiguate::classify`] (an opening tag's name)
/// and `crate::parser::jsx` (nested elements, closing tags, and attribute
/// names), so every one of grammar §5's `JsxName` occurrences agrees on
/// what counts as one name.
///
/// `TODO(phase0)`: non-XID-continue bytes above 0x7F are accepted at
/// every level (not just top-level vs. nested — that dependence was
/// checked and rejected, see issue #4's fix list, item 19's "SKIP"
/// entry); a full Unicode XID check is not worth the dependency in
/// Phase 0.
pub(crate) fn scan_jsx_name(bytes: &[u8], mut pos: usize) -> usize {
    pos = scan_jsx_name_segment(bytes, pos);
    while bytes.get(pos) == Some(&b'-') {
        let after_hyphen = pos + 1;
        let seg_end = scan_jsx_name_segment(bytes, after_hyphen);
        if seg_end == after_hyphen {
            break;
        }
        pos = seg_end;
    }
    pos
}
