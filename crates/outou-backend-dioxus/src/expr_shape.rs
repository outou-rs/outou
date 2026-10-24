//! Whether a child island is one Rust *expression* (issue #27).
//!
//! `child(EXPR)` takes its argument as written, so the island must parse as a
//! single expression. A top-level `;` is the obvious sign of statements, but
//! Rust also ends a statement without one: after a block-like expression
//! (`if`, `while`, `loop`, `match`, `unsafe`, `for`, a bare or labelled
//! block) that is not continued, and after an item (`fn`, `struct`, ...).
//! `{ if ready {} "done" }` is two statements as a block body but a syntax
//! error as a call argument, so those shapes keep their braces.

use outou_syntax::ast::Expr;
use outou_syntax::lexer::rust_token::{next_token, RtKind, RtTok};

/// Keywords that start an item. At nesting depth 0 they mean the island holds
/// statements, whether or not a `;` appears. (`const` is handled separately:
/// `const { .. }` is an expression.)
const ITEM_KEYWORDS: [&str; 8] = [
    "fn", "struct", "enum", "impl", "trait", "mod", "use", "static",
];

/// Keywords that start a block-like expression.
const BLOCK_KEYWORDS: [&str; 6] = ["if", "while", "loop", "match", "unsafe", "for"];

/// Punctuation that, after a closing `}`, may be either a binary operator or
/// the start of the next statement (`{} - 1`, `{} *p = 1`, `{} &x`): the two
/// readings differ, so the braces stay.
const AMBIGUOUS_AFTER_BLOCK: [&str; 8] = ["-", "*", "&", "&&", "|", "||", "!", "#"];

/// How the current depth-0 expression started.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Head {
    Unset,
    BlockLike,
    Other,
}

struct Scan {
    depth: i32,
    head: Head,
    /// A depth-0 block-like head just closed its `}`; the next token decides
    /// whether the expression continues.
    block_closed: bool,
    /// Inside the `#[..]` / `#![..]` attributes before the expression's head.
    in_attribute: bool,
    /// The previous depth-0 token was `const`.
    after_const: bool,
}

/// Whether the parts of one island (a partition of its source: Rust text and
/// nested JSX, which is opaque) form a single expression with no statements.
pub(crate) fn parts_are_one_expression(parts: &[Expr]) -> bool {
    let mut scan = Scan {
        depth: 0,
        head: Head::Unset,
        block_closed: false,
        in_attribute: false,
        after_const: false,
    };
    parts.iter().all(|part| match part {
        Expr::Rust(rust) => scan.rust(&rust.text),
        Expr::Jsx(_) => scan.jsx(),
        Expr::Error(_) => true,
    })
}

impl Scan {
    fn jsx(&mut self) -> bool {
        if self.depth != 0 {
            return true;
        }
        if self.block_closed || self.after_const {
            return false;
        }
        if self.head == Head::Unset {
            self.head = Head::Other;
        }
        true
    }

    /// Scans one Rust part; `false` as soon as a statement shape shows up.
    fn rust(&mut self, text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut pos = 0usize;
        loop {
            let tok = next_token(bytes, pos);
            if tok.start == tok.end {
                return true;
            }
            pos = tok.end;
            if matches!(tok.kind, RtKind::LineComment | RtKind::BlockComment) {
                continue;
            }
            if !self.token(&tok, tok.text(text)) {
                return false;
            }
        }
    }

    fn token(&mut self, tok: &RtTok, word: &str) -> bool {
        if self.depth == 0 && !self.at_depth_zero(tok, word) {
            return false;
        }
        match tok.kind {
            RtKind::OpenDelim => self.depth += 1,
            RtKind::CloseDelim => {
                self.depth -= 1;
                if self.depth == 0 {
                    if self.in_attribute {
                        self.in_attribute = false;
                    } else if self.head == Head::BlockLike && word == "}" {
                        self.block_closed = true;
                    }
                }
            }
            _ => {}
        }
        true
    }

    /// The checks for a token that sits at nesting depth 0 (before it moves
    /// the depth).
    fn at_depth_zero(&mut self, tok: &RtTok, word: &str) -> bool {
        if word == ";" {
            return false;
        }
        if self.after_const {
            self.after_const = false;
            if word != "{" {
                return false; // `const NAME: T = ..`, `const fn`
            }
        }
        if self.block_closed {
            self.block_closed = false;
            let continues = match tok.kind {
                RtKind::Punct => !AMBIGUOUS_AFTER_BLOCK.contains(&word),
                RtKind::Ident => matches!(word, "else" | "as"),
                _ => false,
            };
            if !continues {
                return false;
            }
            // `else` stays inside the same `if`; anything else continues an
            // ordinary expression.
            if word != "else" {
                self.head = Head::Other;
            }
            return true;
        }
        if self.in_attribute {
            return true;
        }
        match tok.kind {
            RtKind::Ident => self.ident(word),
            RtKind::Lifetime => self.set_head(Head::BlockLike), // a loop or block label
            RtKind::OpenDelim if word == "{" => self.set_head(Head::BlockLike),
            RtKind::Punct if word == "#" && self.head == Head::Unset => {
                self.in_attribute = true;
                true
            }
            _ => self.set_head(Head::Other),
        }
    }

    fn ident(&mut self, word: &str) -> bool {
        if ITEM_KEYWORDS.contains(&word) || word == "macro_rules" {
            return false;
        }
        if word == "const" {
            self.after_const = true;
            return self.set_head(Head::Other);
        }
        let head = if BLOCK_KEYWORDS.contains(&word) {
            Head::BlockLike
        } else {
            Head::Other
        };
        self.set_head(head)
    }

    fn set_head(&mut self, head: Head) -> bool {
        if self.head == Head::Unset {
            self.head = head;
        }
        true
    }
}
