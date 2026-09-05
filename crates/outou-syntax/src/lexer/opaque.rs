//! Opaque token trees (grammar §3).
//!
//! The lexer MUST NOT enter a JSX mode inside a macro invocation, a
//! `macro_rules!` definition, or an attribute — it tracks delimiter
//! balance only. [`try_skip_opaque_region`] recognizes the start of one of
//! these three regions at a given position and, if found, returns the
//! position just past its matching closing delimiter; callers skip
//! straight there without looking at anything in between.

use super::rust_token::{next_significant, skip_balanced_group, RtKind};

/// If an opaque region (macro invocation, `macro_rules!` definition, or
/// `#[...]`/`#![...]` attribute) starts at `pos`, returns the position
/// just after it. Otherwise returns `None` and consumes nothing.
///
/// `prev_is_path_continuation` — whether the previous significant token
/// is an identifier, a raw identifier, or `::` — lets a caller that is
/// scanning token by token (as [`crate::parser::region::Parser::scan_region`]
/// and [`crate::parser::item::Parser::parse_items`] both do) skip
/// re-attempting macro-invocation detection at every segment of a long
/// path (M12): [`try_macro_invocation`] only ever succeeds by finding a
/// `!` and a delimiter *ahead* of `pos`, and a macro invocation can only
/// ever *start* where an expression may begin, never in the middle of an
/// already-established path — so a `true` here means this position could
/// not possibly be a macro invocation's start, and the (otherwise
/// path-length-proportional) attempt is skipped outright. The O(1)
/// `#`-prefixed attribute check is unaffected: it terminates on its first
/// byte either way and never walks a path.
pub fn try_skip_opaque_region(
    bytes: &[u8],
    pos: usize,
    prev_is_path_continuation: bool,
) -> Option<usize> {
    try_attribute(bytes, pos)
        .map(|(_start, end)| end)
        .or_else(|| try_macro_rules(bytes, pos))
        .or_else(|| {
            if prev_is_path_continuation {
                None
            } else {
                try_macro_invocation(bytes, pos)
            }
        })
}

/// `#[ ... ]` or `#![ ... ]`, including `#[doc = "..."]` and
/// `#[react_import(...)]` (grammar §1, §3). `pub(crate)` because
/// `crate::parser::item` also needs to recognize a *leading* attribute on
/// its own (to collect it before deciding whether the item it precedes is
/// a `fn`, a `mod`, or plain Rust), separately from the full opaque-region
/// skip this module otherwise only exposes as one bundled check.
///
/// Trivia-tolerant like its siblings [`try_macro_rules`] and
/// [`try_macro_invocation`] (regression for H5): `pos` may point at
/// whitespace or a comment preceding the `#`, not the `#` itself, so this
/// looks ahead with [`next_significant`] rather than requiring
/// `bytes[pos] == b'#'` exactly. Returns `(attribute_start, end)` rather
/// than just `end` so a caller that records the attribute's own span (only
/// [`crate::parser::item::Parser::scan_leading_attributes`] does) can keep
/// any leading trivia in the surrounding Rust text instead of folding it
/// into the attribute node.
pub(crate) fn try_attribute(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    let head = next_significant(bytes, pos);
    if bytes.get(head.start) != Some(&b'#') {
        return None;
    }
    let hash_pos = head.start;
    let bracket_pos = match bytes.get(hash_pos + 1) {
        Some(b'[') => hash_pos + 1,
        Some(b'!') if bytes.get(hash_pos + 2) == Some(&b'[') => hash_pos + 2,
        _ => return None,
    };
    Some((hash_pos, skip_balanced_group(bytes, bracket_pos)))
}

/// `macro_rules ! IDENTIFIER MacroRulesDef`, where `MacroRulesDef` is a
/// delimited token tree following the *name*, not the `!` (grammar §3).
fn try_macro_rules(bytes: &[u8], pos: usize) -> Option<usize> {
    let head = next_significant(bytes, pos);
    if head.kind != RtKind::Ident || &bytes[head.start..head.end] != b"macro_rules" {
        return None;
    }
    let bang = next_significant(bytes, head.end);
    if !is_exact_punct(bytes, &bang, b"!") {
        return None;
    }
    let name = next_significant(bytes, bang.end);
    if !matches!(name.kind, RtKind::Ident | RtKind::RawIdent) {
        return None;
    }
    let delim = next_significant(bytes, name.end);
    if delim.kind == RtKind::OpenDelim {
        Some(skip_balanced_group(bytes, delim.start))
    } else {
        None
    }
}

/// `SimplePath ! DelimTokenTree`: `println!(...)`, `path::to::mac!{...}`,
/// `::krate::mac![...]`, `r#mac!(...)`. The `!` of `!=` never matches here
/// because the general Rust tokenizer already scans `!=` as one token, so
/// a bare, single-character `!` token only exists when nothing continues
/// it into a comparison.
fn try_macro_invocation(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut cursor = pos;
    let mut tok = next_significant(bytes, cursor);
    if is_exact_punct(bytes, &tok, b"::") {
        cursor = tok.end;
        tok = next_significant(bytes, cursor);
    }
    if !matches!(tok.kind, RtKind::Ident | RtKind::RawIdent) {
        return None;
    }
    cursor = tok.end;
    loop {
        let sep = next_significant(bytes, cursor);
        if !is_exact_punct(bytes, &sep, b"::") {
            break;
        }
        let segment = next_significant(bytes, sep.end);
        if !matches!(segment.kind, RtKind::Ident | RtKind::RawIdent) {
            return None;
        }
        cursor = segment.end;
    }
    let bang = next_significant(bytes, cursor);
    if !is_exact_punct(bytes, &bang, b"!") {
        return None;
    }
    let delim = next_significant(bytes, bang.end);
    if delim.kind == RtKind::OpenDelim {
        Some(skip_balanced_group(bytes, delim.start))
    } else {
        None
    }
}

fn is_exact_punct(bytes: &[u8], tok: &super::rust_token::RtTok, text: &[u8]) -> bool {
    tok.kind == RtKind::Punct && &bytes[tok.start..tok.end] == text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_macro_invocation() {
        let src = b"println!(\"x < y\") rest";
        let end = try_skip_opaque_region(src, 0, false).unwrap();
        assert_eq!(&src[..end], b"println!(\"x < y\")");
    }

    #[test]
    fn qualified_path_macro_invocation() {
        let src = b"path::to::mac!{ <not jsx> }";
        let end = try_skip_opaque_region(src, 0, false).unwrap();
        assert_eq!(&src[..end], b"path::to::mac!{ <not jsx> }");
    }

    #[test]
    fn leading_colon_colon_path() {
        let src = b"::krate::mac![a, b]";
        let end = try_skip_opaque_region(src, 0, false).unwrap();
        assert_eq!(&src[..end], b"::krate::mac![a, b]");
    }

    #[test]
    fn macro_rules_definition() {
        let src = b"macro_rules! m { (x) => {}; }";
        let end = try_skip_opaque_region(src, 0, false).unwrap();
        assert_eq!(&src[..end], b"macro_rules! m { (x) => {}; }");
    }

    #[test]
    fn attribute_with_nested_delimiters() {
        let src = b"#[cfg(feature = \"x\")] fn f() {}";
        let end = try_skip_opaque_region(src, 0, false).unwrap();
        assert_eq!(&src[..end], b"#[cfg(feature = \"x\")]");
    }

    #[test]
    fn inner_attribute() {
        let src = b"#![allow(dead_code)]";
        let end = try_skip_opaque_region(src, 0, false).unwrap();
        assert_eq!(&src[..end], b"#![allow(dead_code)]");
    }

    #[test]
    fn not_equal_is_not_a_macro_bang() {
        assert_eq!(try_skip_opaque_region(b"a != b", 0, false), None);
    }

    #[test]
    fn unary_not_is_not_a_macro_invocation() {
        assert_eq!(try_skip_opaque_region(b"!foo(x)", 0, false), None);
    }

    #[test]
    fn plain_identifier_is_not_opaque() {
        assert_eq!(try_skip_opaque_region(b"foo(x)", 0, false), None);
    }
}
