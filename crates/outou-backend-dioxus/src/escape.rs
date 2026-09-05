//! Text and identifier escaping rules specific to Dioxus's `rsx!` syntax.

/// Rust keywords (2021 edition: strict, strict-2018, reserved and weak in
/// keyword position), used to decide whether a JSX name can be written as
/// a plain `name: value` field key or needs Dioxus's raw string-key syntax
/// (`"name": value`; see [`super::attribute`]).
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "union",
];

/// Rust keywords that cannot be written as a raw identifier (`r#name`) at
/// all — Rust reserves the escape from applying to these four, so a JSX
/// name that happens to be one of them has no representable plain-ish
/// field key and must keep falling back to Dioxus's string-key syntax
/// (element-only; see `docs/backend-leakage.md`).
const NON_RAW_IDENTIFIABLE_KEYWORDS: &[&str] = &["self", "Self", "crate", "super"];

/// Whether `name` is shaped like a legal Rust identifier: starts with an
/// ASCII letter or `_`, followed by ASCII alphanumerics or `_`. Shared by
/// [`is_plain_ident`] and [`is_raw_identifiable_keyword`], which differ
/// only in what they require of a keyword.
fn is_identifier_shaped(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.clone().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `name` can be written as a plain Rust field-key identifier
/// (`name: value`): an ASCII identifier that is not a Rust keyword.
///
/// A hyphenated JSX name (`data-id`) or a JSX name that happens to be a
/// Rust keyword (`type`, `for`, …) — both legal per grammar §5 — fails this
/// check; a keyword name that [`is_raw_identifiable_keyword`] accepts is
/// instead written as `r#name: value` (works for elements and
/// components); the remaining four keywords, and any hyphenated name,
/// fall back to Dioxus's raw string-key syntax (`docs/backend-leakage.md`,
/// "attribute keys that are not a plain Rust identifier").
pub fn is_plain_ident(name: &str) -> bool {
    is_identifier_shaped(name) && !RUST_KEYWORDS.contains(&name)
}

/// Whether `name` is a Rust keyword that can be written as a raw
/// identifier (`r#name`) — every reserved word [`RUST_KEYWORDS`] lists
/// except the four Rust itself refuses as a raw identifier
/// ([`NON_RAW_IDENTIFIABLE_KEYWORDS`]). Verified (issue #6 fix list item
/// 3) to compile as an `rsx!` field key for both elements and
/// components, unlike the string-key syntax, which HIGH-3 found only
/// exists for elements.
pub fn is_raw_identifiable_keyword(name: &str) -> bool {
    is_identifier_shaped(name)
        && RUST_KEYWORDS.contains(&name)
        && !NON_RAW_IDENTIFIABLE_KEYWORDS.contains(&name)
}

/// Builds a Rust string literal for `value` suitable for splicing into
/// `rsx!` text or an attribute value.
///
/// Dioxus's `rsx!` treats every string literal as an interpolated format
/// string (ifmt), so a literal `{` or `}` byte in `value` must be doubled
/// to `{{`/`}}` or it would be parsed as (the start of) an interpolation —
/// see `docs/backend-leakage.md`. Backslashes and double quotes are
/// escaped so the emitted literal is valid Rust; control characters that
/// cannot appear literally inside a Rust string on one line are escaped
/// too (JSX whitespace normalization already collapses text to single
/// lines, per grammar §8, but this function makes no such assumption for
/// attribute values, which are not normalized).
pub fn rust_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '{' => out.push_str("{{"),
            '}' => out.push_str("}}"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Builds a plain Rust string literal for `value` — an ordinary Rust
/// string, never one of Dioxus's `rsx!` ifmt format strings — escaping
/// only `\`, `"` and the control characters that cannot appear literally
/// inside a Rust string on one line. `{`/`}` are left exactly as
/// written: doubling them (as [`rust_string_literal`] does for splicing
/// into `rsx!` text) would corrupt a value like `#[path = "…"]`, which
/// is not a format string at all (MEDIUM-9, issue #6 fix list item 10).
pub fn plain_rust_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Builds the format-string literal Dioxus's `rsx!` requires for the
/// `key` attribute's value (`key: "{expr}"`; HIGH-2, issue #6 fix list
/// item 2): `expr_source`, the `key={expr}` island's own source text, is
/// spliced verbatim inside `{…}` so the expression keeps whatever type it
/// already had, since `rsx!` rejects `key: {expr}` (the bare island form
/// every other attribute uses) with "Key must be in the form of a
/// formatted string like `key: \"{value}\"`".
///
/// Unlike [`rust_string_literal`], `{`/`}` are never doubled here — they
/// are the meaningful ifmt interpolation syntax this function exists to
/// produce, not literal text — so only `\` and `"` (which could otherwise
/// end the literal early or misquote it) are escaped. A literal newline
/// inside `expr_source` is valid unescaped inside a Rust string, so it is
/// left as-is.
pub fn key_interpolation_literal(expr_source: &str) -> String {
    let mut out = String::with_capacity(expr_source.len() + 4);
    out.push_str("\"{");
    for ch in expr_source.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out.push_str("}\"");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_identifiers_pass() {
        assert!(is_plain_ident("class"));
        assert!(is_plain_ident("_private"));
        assert!(is_plain_ident("onclick"));
    }

    #[test]
    fn hyphenated_and_keyword_names_fail() {
        assert!(!is_plain_ident("data-id"));
        assert!(!is_plain_ident("type"));
        assert!(!is_plain_ident("for"));
        assert!(!is_plain_ident(""));
        assert!(!is_plain_ident("1abc"));
    }

    #[test]
    fn braces_are_doubled() {
        assert_eq!(rust_string_literal("a{b}c"), "\"a{{b}}c\"");
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        assert_eq!(rust_string_literal("he\"llo\\"), "\"he\\\"llo\\\\\"");
    }

    #[test]
    fn plain_text_round_trips() {
        assert_eq!(rust_string_literal("Hello "), "\"Hello \"");
    }

    #[test]
    fn raw_identifiable_keywords_are_recognized() {
        // Legal raw identifiers (issue #6 fix list item 3): Dioxus's
        // `r#name` escape works uniformly for elements and components,
        // unlike the string-key syntax, which HIGH-3 found is
        // element-only.
        assert!(is_raw_identifiable_keyword("type"));
        assert!(is_raw_identifiable_keyword("for"));
        assert!(is_raw_identifiable_keyword("async"));
    }

    #[test]
    fn non_raw_identifiable_keywords_are_excluded() {
        // Rust itself refuses `r#self`, `r#Self`, `r#crate`, `r#super` as
        // raw identifiers; these four must keep falling back to the
        // string-key form.
        assert!(!is_raw_identifiable_keyword("self"));
        assert!(!is_raw_identifiable_keyword("Self"));
        assert!(!is_raw_identifiable_keyword("crate"));
        assert!(!is_raw_identifiable_keyword("super"));
    }

    #[test]
    fn plain_identifiers_and_hyphenated_names_are_not_raw_identifiable() {
        assert!(!is_raw_identifiable_keyword("class"));
        assert!(!is_raw_identifiable_keyword("data-id"));
        assert!(!is_raw_identifiable_keyword(""));
    }

    #[test]
    fn key_interpolation_literal_wraps_the_expression_in_braces() {
        assert_eq!(key_interpolation_literal("x.clone()"), "\"{x.clone()}\"");
    }

    #[test]
    fn plain_rust_string_literal_does_not_double_braces() {
        // MEDIUM-9, issue #6 fix list item 10: `#[path]` is not a format
        // string; doubling braces in its value would corrupt the path.
        assert_eq!(
            plain_rust_string_literal("a{b}c\\d.rs"),
            "\"a{b}c\\\\d.rs\""
        );
    }

    #[test]
    fn plain_rust_string_literal_escapes_quotes_and_backslashes() {
        assert_eq!(plain_rust_string_literal("he\"llo\\"), "\"he\\\"llo\\\\\"");
    }

    #[test]
    fn key_interpolation_literal_escapes_quotes_and_backslashes_only() {
        // `{`/`}` are meaningful ifmt interpolation syntax here, not
        // literal text, so — unlike `rust_string_literal` — they are
        // never doubled.
        assert_eq!(
            key_interpolation_literal(r#"x.replace("a","b")"#),
            r#""{x.replace(\"a\",\"b\")}""#
        );
    }
}
