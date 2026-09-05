//! Expression-position tracking (grammar §4 rule 1).
//!
//! "Is a `<` at this point in a position where an expression may begin?" is
//! answered from the *previous significant Rust token* alone: no
//! backtracking, no parse tree required. [`ExprPosition::from_prev`] is the
//! single source of truth: every caller that needs to know whether a `<`
//! could possibly start JSX goes through it before ever calling into
//! [`super::disambiguate`].
//!
//! The three buckets, in the terms the issue and grammar use:
//!
//! - [`ExprPosition::Operand`]: the previous token completed a value
//!   (identifier, literal, a closing delimiter of an expression, `?`).
//!   `<` here is the less-than operator; JSX can never start.
//! - [`ExprPosition::Type`]: the previous token can only be followed by a
//!   `Type`, never an `Expression` (`:`, `::`, `->`, `as`, `where`, `impl`,
//!   `dyn`, `fn`, `type`, `struct`, `enum`, `trait`). `<` here opens a
//!   qualified path or a generic argument list; JSX can never start.
//! - [`ExprPosition::Expr`]: everything else — operators, `(`, `[`, `{`,
//!   `,`, `;`, `=>`, block/statement start, keywords such as `return`,
//!   `break`, `in` that always open an expression. `<` here is a
//!   *candidate*; rules 2 and 3 in [`super::disambiguate`] still have to
//!   decide Rust vs JSX.
//!
//! `Expr` is the default for anything not explicitly recognized as
//! `Operand` or `Type`. This is deliberately permissive: a wrong guess of
//! `Expr` merely asks rules 2/3 to look harder (and they only ever commit
//! to JSX for shapes that cannot be a Rust type), whereas a wrong guess of
//! `Operand` or `Type` would silently hide real JSX from the parser. See
//! `docs/grammar.md` §4 rule 1.

use super::rust_token::{RtKind, RtTok};

/// Whether a `<` following the previous significant token could start a
/// JSX expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprPosition {
    /// The previous token completed an operand; `<` is the operator.
    Operand,
    /// The previous token can only be followed by a Rust `Type`.
    Type,
    /// An expression may begin here; `<` is a JSX candidate.
    Expr,
}

/// Identifiers (including keywords) after which `<` is definitely a type
/// position, never an expression.
const TYPE_POSITION_KEYWORDS: &[&str] = &[
    "as", "where", "impl", "dyn", "fn", "type", "struct", "enum", "trait",
];

/// Identifiers that, despite being keywords, always open an expression
/// position. `if`, `else`, `while` and `match` are here (decision D2, H4):
/// each is always followed by an expression (a condition, a scrutinee, or
/// the body of an `else` with no condition), never a `Type`, so `<` right
/// after one of them is always a JSX candidate. This is safe even for a
/// real qualified path: `if <T as Tr>::C {}` still reaches rule 2 of
/// `super::disambiguate`, which sends `t2 == as` to Rust.
const EXPR_POSITION_KEYWORDS: &[&str] = &["return", "break", "in", "if", "else", "while", "match"];

/// Punctuation tokens after which `<` is a type position (never JSX).
///
/// `:` is deliberately absent (decision D2, H4). Unlike `::` (a path
/// separator) and `->` (a return-type arrow), a bare `:` is not
/// exclusively a type position in Rust: it also introduces a struct
/// field's *value* (`Props { child: <A/> }`), an expression position.
/// Treating `:` as `Expr` merely asks `super::disambiguate::classify` to
/// look harder; it still answers Rust for every genuine type-ascription
/// `:` (`let x: <T as Tr>::A`, `fn f(x: <A<B>>::C)`), because those shapes
/// always have `as` at `t2`/`t3` or a matching `>` followed by `::`
/// (grammar §4 rule 2/3) — the same reasoning that already makes
/// `let v: Vec<A> = …` Rust (grammar §4.1), where the `<` follows the
/// identifier `Vec` directly, not the `:`. A half-typed `let x: <T` mid-edit
/// commits to JSX in an IDE until more is typed; this is the documented
/// residual risk, acceptable because incomplete input staying JSX is what
/// keeps IDE features alive (grammar §4, "Commitment is final").
const TYPE_POSITION_PUNCT: &[&str] = &["::", "->"];

/// Classifies the position right after `prev`, per grammar §4 rule 1.
/// `prev = None` means start of file or start of a block: expression
/// position.
pub fn from_prev(prev: Option<(RtTok, &str)>) -> ExprPosition {
    let Some((tok, text)) = prev else {
        return ExprPosition::Expr;
    };
    match tok.kind {
        RtKind::Ident | RtKind::RawIdent => {
            if tok.kind == RtKind::Ident && TYPE_POSITION_KEYWORDS.contains(&text) {
                ExprPosition::Type
            } else if tok.kind == RtKind::Ident && EXPR_POSITION_KEYWORDS.contains(&text) {
                ExprPosition::Expr
            } else {
                // A plain identifier (or raw identifier) completes an
                // operand: `self`, `true`, `false`, `foo`, `r#fn`, …
                ExprPosition::Operand
            }
        }
        RtKind::Literal => ExprPosition::Operand,
        // No valid Rust puts JSX directly after a lifetime (decision D2):
        // a lifetime is always followed by a type, a bound, or a binder,
        // never an expression. `Type`, not the permissive `Expr` default.
        RtKind::Lifetime => ExprPosition::Type,
        RtKind::CloseDelim => {
            // `)` and `]` always complete an operand: `<` right after one
            // is the less-than operator. `}` is different (decision D2,
            // H4): at rustc's own statement grammar, a `}` almost always
            // ends a *statement* (an `if`/`while`/`for`/`match`/block used
            // as a statement, which yields `()`), and the parser is about
            // to start a brand new statement at the following token — an
            // expression position. Treating `}` as `Expr` fixes the
            // realistic case (`if !ready { return <A/>; }\n<div/>`, a
            // `for` loop followed by JSX, `struct Local {}` followed by
            // JSX) at the cost of one accepted divergence: a block used as
            // a comparison *operand* (`let b = { 1 } < y;`), which is
            // vanishingly rare and stays Rust-as-comparison only because
            // `<` is not a JSX candidate in that position under this rule
            // — see `block_operand_comparison_stays_rust` in
            // `tests/disambiguation.rs`, which documents this trade-off by
            // asserting the accepted (non-JSX) behavior.
            if text == "}" {
                ExprPosition::Expr
            } else {
                ExprPosition::Operand
            }
        }
        RtKind::OpenDelim => ExprPosition::Expr,
        RtKind::Punct => {
            if TYPE_POSITION_PUNCT.contains(&text) {
                ExprPosition::Type
            } else if text == "?" {
                ExprPosition::Operand
            } else {
                ExprPosition::Expr
            }
        }
        RtKind::LineComment | RtKind::BlockComment | RtKind::Unknown => ExprPosition::Expr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::rust_token::next_significant;

    fn prev_of(source: &str, needle: &str) -> ExprPosition {
        let idx = source.find(needle).expect("needle in source");
        let bytes = source.as_bytes();
        // Find the token immediately before `idx` by scanning tokens from
        // the start (simple and correct; test-only).
        let mut pos = 0;
        let mut prev: Option<RtTok> = None;
        loop {
            let tok = next_significant(bytes, pos);
            if tok.start >= idx || tok.start == tok.end {
                break;
            }
            prev = Some(tok);
            pos = tok.end;
        }
        from_prev(prev.map(|t| (t, t.text(source))))
    }

    #[test]
    fn operand_then_less_than_is_operand_position() {
        assert_eq!(prev_of("a < b", "< b"), ExprPosition::Operand);
        assert_eq!(prev_of("f::<T>()", "<T>"), ExprPosition::Type);
    }

    #[test]
    fn colon_and_arrow_are_type_position() {
        // The `<` right after `Vec` is Operand position (an identifier
        // precedes it directly); the grammar row `let v: Vec<A> = …` is
        // Rust because of *that*, not because `:` forces type position
        // all the way through — see `docs/grammar.md` §4.1.
        assert_eq!(prev_of("let v: Vec<A> = x;", "<A"), ExprPosition::Operand);
        // `:` itself is `Expr`, not `Type` (decision D2): it also
        // introduces a struct field's value (`Props { child: <A/> }`), so
        // `super::disambiguate::classify` — not this module — is what
        // still resolves a real type-ascription `<` to Rust (rule 2/3).
        assert_eq!(prev_of("x: <T", "<T"), ExprPosition::Expr);
        assert_eq!(prev_of("fn f() -> <T", "<T"), ExprPosition::Type);
    }

    #[test]
    fn block_and_statement_start_are_expr_position() {
        assert_eq!(prev_of("{ <A/> }", "<A"), ExprPosition::Expr);
        assert_eq!(prev_of("return <A/>", "<A"), ExprPosition::Expr);
        assert_eq!(prev_of("match m { _ => <A/> }", "<A"), ExprPosition::Expr);
    }
}
