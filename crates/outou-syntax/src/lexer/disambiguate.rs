//! The `<` ambiguity: rules 2 and 3 of grammar §4.
//!
//! Callers first establish that a `<` is in *expression position* (rule 1,
//! [`super::position`]). Only then do they call [`classify`], which
//! implements:
//!
//! - **Rule 2**, the three-token decision (`t1`, `t2`, `t3`);
//! - **Rule 3**, the bounded angle scan, reached only for the `<Name<`
//!   shape.
//!
//! Both rules need a view of `<` and `>` that never merges them into `<<`,
//! `>>`, `<=`, `>=`, … the way ordinary Rust tokenization does — the
//! grammar requires compound tokens to be split into their `<`/`>` parts
//! (§4 rule 3). [`next_dtoken`] provides that view; everywhere else, this
//! module reuses [`super::rust_token`] as-is.
//!
//! Nothing here ever backtracks: `classify` looks at a bounded number of
//! tokens (three, or one bounded scan) and returns a final answer.

use super::rust_token::{next_significant, skip_balanced_group, RtKind, RtTok};
use super::scan_jsx_name;

/// Tag names that always begin a Rust `Type` in a qualified path and are
/// therefore never available as JSX tag names in Phase 0 (grammar §4 rule
/// 2, §5).
const RESERVED_TAG_KEYWORDS: &[&str] = &["dyn", "impl", "fn", "unsafe", "extern", "for"];

/// Result of classifying a candidate `<` already known to be in expression
/// position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// Not JSX: the `<` is the less-than operator, or opens a Rust type in
    /// a qualified path. The parser should treat it as ordinary Rust and
    /// keep scanning.
    Rust,
    /// `<>`: a fragment. Reserved in Phase 0 (grammar §10).
    Fragment {
        /// Position of the `<`.
        lt: usize,
    },
    /// `</` with nothing open: a stray closing tag.
    StrayClose {
        /// Position of the `<`.
        lt: usize,
    },
    /// Committed to JSX with a tag name.
    Named(NamedTag),
}

/// A committed JSX opening tag, as far as classification alone can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedTag {
    /// Byte range of the tag name's first (and, absent hyphenation, only)
    /// segment.
    pub name_start: usize,
    /// End of that first segment.
    pub name_end: usize,
    /// Where tag parsing (attributes, `/>`, `>`) should resume. Equal to
    /// `name_end` unless rule 3 skipped a generic-argument list.
    pub resume_at: usize,
    /// Set when rule 3 fired and skipped `<...>` generic arguments; carries
    /// the position of the *inner* `<` for the diagnostic (grammar §4
    /// rule 3, §10).
    pub generic_args_skipped_at: Option<usize>,
}

/// Re-tokenizes from `pos`, skipping comments, but always splitting a
/// leading `<` or `>` off as a single-character token even when ordinary
/// Rust tokenization would fold it into `<<`, `>>`, `<=`, `>=`, `<<=` or
/// `>>=`. This is exactly rustc's own splitting rule, applied here because
/// rule 3's depth count requires it (grammar §4 rule 3).
fn next_dtoken(bytes: &[u8], pos: usize) -> RtTok {
    let tok = next_significant(bytes, pos);
    if tok.kind == RtKind::Punct && matches!(bytes.get(tok.start), Some(b'<') | Some(b'>')) {
        return RtTok {
            kind: RtKind::Punct,
            start: tok.start,
            end: tok.start + 1,
        };
    }
    tok
}

fn is_eof(tok: &RtTok) -> bool {
    tok.start == tok.end
}

/// Classifies a `<` at `lt_pos` (which must be the `<` byte) already known
/// to be in expression position. `source` is the whole file; only bytes
/// from `lt_pos` onward are read.
pub fn classify(source: &str, lt_pos: usize) -> Classification {
    let bytes = source.as_bytes();
    debug_assert_eq!(bytes.get(lt_pos), Some(&b'<'));

    let t1 = next_dtoken(bytes, lt_pos + 1);
    if is_eof(&t1) {
        return Classification::Rust;
    }
    let t1_text = t1.text(source);

    // Row: `(`, `[`, `&`, `&&`, `*`, `!`, `_`, `::`, `<`, a lifetime, or a
    // literal → Rust (begins a Type, or cannot begin a JsxName).
    let begins_type_or_cannot_be_name = match t1.kind {
        RtKind::OpenDelim => matches!(t1_text, "(" | "["),
        RtKind::Lifetime | RtKind::Literal => true,
        RtKind::Punct => matches!(t1_text, "&" | "&&" | "*" | "!" | "::" | "<"),
        _ => false,
    };
    if begins_type_or_cannot_be_name {
        return Classification::Rust;
    }

    // `_` lexes as an identifier but stands for an inferred type here.
    if t1.kind == RtKind::Ident && t1_text == "_" {
        return Classification::Rust;
    }

    // Row: reserved keywords → Rust, unconditionally.
    if t1.kind == RtKind::Ident && RESERVED_TAG_KEYWORDS.contains(&t1_text) {
        return Classification::Rust;
    }

    // Row: `>` → fragment.
    if t1.kind == RtKind::Punct && t1_text == ">" {
        return Classification::Fragment { lt: lt_pos };
    }

    // Row: `/` → stray closing tag.
    if t1.kind == RtKind::Punct && t1_text == "/" {
        return Classification::StrayClose { lt: lt_pos };
    }

    // Row: any other IDENTIFIER_OR_KEYWORD (including raw identifiers):
    // consult t2. Anything else falls to the final Rust catch-all.
    if !matches!(t1.kind, RtKind::Ident | RtKind::RawIdent) {
        return Classification::Rust;
    }

    let name_start = t1.start;
    // A `JsxName` may be hyphenated (grammar §5: `IDENTIFIER_OR_KEYWORD
    // ('-' IDENTIFIER_OR_KEYWORD)*`), but `t1` is only ever one plain Rust
    // identifier token (`scan_ident_run` stops at `-`, which is not an
    // identifier-continue byte). Extend `name_end` over any hyphenated
    // continuation with the same scanner the tag parser and the closing-tag
    // matcher use, *before* consulting `t2`, so `t2` is the token after the
    // whole name (H8) — `<my-element as="x" />` must see `t2 == "as"`, not
    // the `-` of its first segment. No Rust token can ever follow a `-`
    // that is not itself part of the tag name here: whatever `t1` matched,
    // a genuine Rust expression starting with `<` never continues past a
    // bare identifier with a `-`, so widening past every hyphenated
    // segment cannot turn a real qualified path into JSX by accident.
    let name_end = scan_jsx_name(bytes, name_start).max(t1.end);
    let t2 = next_dtoken(bytes, name_end);
    if is_eof(&t2) {
        return Classification::Named(NamedTag {
            name_start,
            name_end,
            resume_at: name_end,
            generic_args_skipped_at: None,
        });
    }
    let t2_text = t2.text(source);

    if t2.kind == RtKind::Ident && t2_text == "as" {
        let t3 = next_dtoken(bytes, t2.end);
        if !is_eof(&t3) && t3.kind == RtKind::Punct && t3.text(source) == "=" {
            return Classification::Named(NamedTag {
                name_start,
                name_end,
                resume_at: name_end,
                generic_args_skipped_at: None,
            });
        }
        return Classification::Rust;
    }

    if t2.kind == RtKind::Punct && t2_text == "::" {
        return Classification::Rust;
    }

    if t2.kind == RtKind::Punct && t2_text == "<" {
        return rule3_scan(source, lt_pos, t2.start, name_start, name_end);
    }

    if t2.kind == RtKind::Punct && t2_text == ">" {
        let t3 = next_dtoken(bytes, t2.end);
        if !is_eof(&t3) && t3.kind == RtKind::Punct && t3.text(source) == "::" {
            return Classification::Rust;
        }
        return Classification::Named(NamedTag {
            name_start,
            name_end,
            resume_at: name_end,
            generic_args_skipped_at: None,
        });
    }

    if t2.kind == RtKind::Punct && t2_text == "!" {
        let t3 = next_dtoken(bytes, t2.end);
        let is_type_macro = !is_eof(&t3) && t3.kind == RtKind::OpenDelim;
        if is_type_macro {
            return Classification::Rust;
        }
        return Classification::Named(NamedTag {
            name_start,
            name_end,
            resume_at: name_end,
            generic_args_skipped_at: None,
        });
    }

    // Catch-all: `/`, `=`, `.`, `:`, `{`, `}`, `)`, `]`, `;`, a string
    // literal, an identifier-or-keyword, end of file, or anything else →
    // JSX, committed. The tag parser (running in `JsxTag` mode from
    // `resume_at`) reports the specific reserved-syntax diagnostic for
    // `.`, `:`, `{`, … when it actually reaches them.
    Classification::Named(NamedTag {
        name_start,
        name_end,
        resume_at: name_end,
        generic_args_skipped_at: None,
    })
}

/// How [`scan_angle_depth`]'s bounded scan stopped.
enum AngleScanStop {
    /// The angle depth opened by `candidate_lt` returned to `0`; `after_gt`
    /// is the position right after that matching `>`, so a caller that
    /// still needs to decide Rust-vs-JSX (only the top-level entry point,
    /// [`rule3_scan`], does) can check what follows it.
    DepthZero { after_gt: usize },
    /// `;`, a closing delimiter opened before `candidate_lt`, or end of
    /// input stopped the scan before the depth returned to `0`; `at` is
    /// where it gave up.
    GaveUp { at: usize },
}

/// The bounded angle scan itself (grammar §4 rule 3): scans forward from
/// `candidate_lt` (the `<` being decided), splitting compound tokens into
/// their `<`/`>` parts, skipping balanced groups and atomic literals, and
/// counting depth. Shared by [`rule3_scan`] (the top-level entry point,
/// which still has a Rust-vs-JSX decision to make once depth returns to 0)
/// and [`skip_nested_generic_arguments`] (a nested child's `<Name<...>`,
/// which has no such decision left to make — grammar §2.1 already
/// committed to JSX the moment this `<` was seen inside `JsxText`).
fn scan_angle_depth(bytes: &[u8], candidate_lt: usize) -> AngleScanStop {
    let mut depth: i32 = 0;
    let mut pos = candidate_lt;
    loop {
        let tok = next_dtoken(bytes, pos);
        if is_eof(&tok) {
            return AngleScanStop::GaveUp { at: pos };
        }
        match tok.kind {
            RtKind::OpenDelim => {
                pos = skip_balanced_group(bytes, tok.start);
                continue;
            }
            RtKind::CloseDelim => return AngleScanStop::GaveUp { at: tok.start },
            RtKind::Punct if bytes[tok.start] == b';' => {
                return AngleScanStop::GaveUp { at: tok.start }
            }
            RtKind::Punct if bytes[tok.start] == b'<' => {
                depth += 1;
                pos = tok.end;
            }
            RtKind::Punct if bytes[tok.start] == b'>' => {
                depth -= 1;
                pos = tok.end;
                if depth == 0 {
                    return AngleScanStop::DepthZero { after_gt: pos };
                }
            }
            _ => pos = tok.end,
        }
    }
}

/// Rule 3: the bounded angle scan for `<Name<...`, with the Rust-vs-JSX
/// decision that only the top-level entry point needs (a nested child has
/// none — see [`skip_nested_generic_arguments`]).
fn rule3_scan(
    source: &str,
    candidate_lt: usize,
    inner_lt: usize,
    name_start: usize,
    name_end: usize,
) -> Classification {
    let bytes = source.as_bytes();
    // Where attribute parsing resumes once this commits to JSX: right
    // after the *inner* generic-argument list (`<T>`), never after the
    // point the whole-tag scan below reaches depth zero — for a
    // self-closing tag that point is the tag's own `/>`, which would
    // otherwise swallow every attribute into a widened, wrong element
    // span (HIGH-2, issue #4 fix list item 1). Computed once up front:
    // both branches below that commit to JSX use the same value.
    let resume_at = skip_nested_generic_arguments(bytes, inner_lt);
    let generic_args = || {
        Classification::Named(NamedTag {
            name_start,
            name_end,
            resume_at,
            generic_args_skipped_at: Some(inner_lt),
        })
    };
    // The Rust-vs-JSX decision itself still needs the *whole-candidate*
    // scan (from the tag's own `<`, not the inner one): only that scan
    // counts both angle brackets of a real qualified path like
    // `<Vec<i32>>::new()`, where depth must return to zero at the
    // *outer* `>` before checking for a following `::`.
    match scan_angle_depth(bytes, candidate_lt) {
        AngleScanStop::GaveUp { .. } => generic_args(),
        AngleScanStop::DepthZero { after_gt } => {
            let next = next_dtoken(bytes, after_gt);
            let next_is_path_sep =
                !is_eof(&next) && next.kind == RtKind::Punct && next.text(source) == "::";
            if next_is_path_sep {
                Classification::Rust
            } else {
                generic_args()
            }
        }
    }
}

/// A nested child's `<Name<...>` generic-argument list (grammar §4 rule 3's
/// angle-counting mechanics), used by `crate::parser::jsx` when a `<` seen
/// while already inside `JsxText` is followed by a name and then another
/// `<`. Unlike [`rule3_scan`], there is no Rust-vs-JSX ambiguity to resolve
/// here at all (decision, issue #4 fix list item 12): grammar §2.1 commits
/// every non-`/` `<` inside `JsxText` to a nested JSX element before this
/// is ever reached, so whatever follows the matching `>` — a `::` or
/// anything else — does not change that; the generic argument list is
/// simply skipped and diagnosed (grammar §7.2, §10). Returns the position
/// to resume tag parsing at, past the skipped `<...>`.
pub(crate) fn skip_nested_generic_arguments(bytes: &[u8], candidate_lt: usize) -> usize {
    match scan_angle_depth(bytes, candidate_lt) {
        AngleScanStop::GaveUp { at } => at,
        AngleScanStop::DepthZero { after_gt } => after_gt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_at(source: &str, needle: &str) -> Classification {
        let lt_pos = source.find(needle).expect("needle in source");
        classify(source, lt_pos)
    }

    #[test]
    fn rule2_t1_begins_a_type() {
        for src in [
            "<(A, B)>::default()",
            "<[T]>::len(&x)",
            "<&str>::from(s)",
            "<*const T>::f()",
        ] {
            assert_eq!(classify_at(src, "<"), Classification::Rust, "{src}");
        }
        assert_eq!(classify_at("<'a>::f()", "<"), Classification::Rust);
        assert_eq!(classify_at("<1>::f()", "<"), Classification::Rust);
        assert_eq!(classify_at("<::A>::f()", "<"), Classification::Rust);
        assert_eq!(classify_at("<<A as B>::C>::d()", "<"), Classification::Rust);
    }

    #[test]
    fn rule2_t1_reserved_keyword_is_rust() {
        for src in [
            "<dyn Trait>::f(&x)",
            "<fn() -> T>::f",
            "<for<'a> fn(&'a T)>::x",
        ] {
            assert_eq!(classify_at(src, "<"), Classification::Rust, "{src}");
        }
    }

    #[test]
    fn rule2_fragment_and_stray_close() {
        assert!(matches!(
            classify_at("<>", "<"),
            Classification::Fragment { .. }
        ));
        assert!(matches!(
            classify_at("</div>", "<"),
            Classification::StrayClose { .. }
        ));
    }

    #[test]
    fn rule2_t2_as_attribute_vs_cast() {
        assert!(matches!(
            classify_at(r#"<link as="style" />"#, "<"),
            Classification::Named(_)
        ));
        assert_eq!(classify_at("<T as Trait>::f()", "<"), Classification::Rust);
    }

    #[test]
    fn rule2_t2_path_sep_is_rust() {
        assert_eq!(classify_at("<A::B>::c()", "<"), Classification::Rust);
    }

    #[test]
    fn rule2_t2_gt_then_coloncolon_is_rust() {
        assert_eq!(classify_at("<Self>::new()", "<"), Classification::Rust);
        assert_eq!(classify_at("<A>::B</A>", "<"), Classification::Rust);
    }

    #[test]
    fn rule2_t2_gt_alone_is_jsx() {
        assert!(matches!(
            classify_at("<div>", "<"),
            Classification::Named(_)
        ));
    }

    #[test]
    fn rule2_t2_bang_type_macro_vs_error() {
        assert_eq!(classify_at("<ty!()>::default()", "<"), Classification::Rust);
        assert!(matches!(
            classify_at("<div!>", "<"),
            Classification::Named(_)
        ));
    }

    #[test]
    fn rule2_catch_all_commits_jsx() {
        for src in [
            "<div type=\"x\" />",
            "<Foo.Bar />",
            "<svg:rect />",
            "<A {...props} />",
            "let v = <Button />;",
        ] {
            assert!(
                matches!(classify_at(src, "<"), Classification::Named(_)),
                "{src}"
            );
        }
    }

    #[test]
    fn rule3_generics_with_path_sep_is_rust() {
        assert_eq!(classify_at("<Vec<i32>>::new()", "<"), Classification::Rust);
        assert_eq!(classify_at("<A<B<C>>>::x()", "<"), Classification::Rust);
        assert_eq!(
            classify_at("<A<<B as C>::D>>::x()", "<"),
            Classification::Rust
        );
    }

    #[test]
    fn rule3_generics_without_path_sep_is_jsx() {
        let src = "<List<T> items={items} />";
        match classify_at(src, "<") {
            Classification::Named(tag) => {
                assert!(tag.generic_args_skipped_at.is_some());
                // `resume_at` must point right after the skipped `<T>`
                // generic-argument list, not after the whole tag (HIGH-2,
                // issue #4 fix list item 1/7): the byte right after `resume_at`
                // is the space before `items`, not anything past `/>`.
                let expected_resume = src.find("T>").expect("needle") + 2;
                assert_eq!(tag.resume_at, expected_resume);
                assert_eq!(&src[tag.resume_at..], " items={items} />");
            }
            other => panic!("expected Named, got {other:?}"),
        }
    }
}
