//! Mode-aware lexer.
//!
//! The lexer switches between three modes:
//!
//! ```text
//! Rust ──(JSX start)──▶ JsxTag ──(>)──▶ JsxText ──({)──▶ Rust
//!   ▲                                      ▲               │
//!   └──────────────────(})──────────────────┘◀──────────────┘
//! ```
//!
//! Rust macro token trees and attribute bodies are lexed as opaque Rust
//! token trees; the lexer never enters a JSX mode inside them.

use outou_sourcemap::Span;

/// Lexer mode. Determines how `<`, `>`, `{`, `}` and text are tokenized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Ordinary Rust tokens.
    Rust,
    /// Inside `<` … `>` of a JSX tag: names, attributes, `/`, `=`.
    JsxTag,
    /// Between a JSX opening and closing tag: text and child elements.
    JsxText,
}

/// Token kinds. The Rust subset is intentionally coarse: Outou never needs to
/// understand Rust beyond finding where expressions start and end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// A Rust token or opaque token tree (macro body, attribute body).
    Rust(String),
    /// `<` that starts a JSX element.
    JsxOpen,
    /// `</` that starts a closing tag.
    JsxCloseOpen,
    /// `>` that ends a tag.
    JsxTagEnd,
    /// `/>` that ends a self-closing tag.
    JsxSelfClose,
    /// Element or component name inside a tag.
    JsxName(String),
    /// Attribute name inside a tag.
    JsxAttrName(String),
    /// `=` inside a tag.
    JsxEq,
    /// A string literal attribute value.
    JsxString(String),
    /// Raw text between tags, before whitespace normalization.
    JsxText(String),
    /// `{` that opens a Rust expression island.
    ExprOpen,
    /// `}` that closes a Rust expression island.
    ExprClose,
    /// Something the lexer could not classify. Parsing continues.
    Error,
    /// End of input.
    Eof,
}

/// A token with its span in the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// What it is.
    pub kind: TokenKind,
    /// Where it is.
    pub span: Span,
}

/// Tokenizes `source`, starting in [`Mode::Rust`].
pub fn tokenize(source: &str) -> Vec<Token> {
    let _ = source;
    todo!("outou-syntax: lexer is implemented in Phase 0, Week 3 (Gate 1)")
}
