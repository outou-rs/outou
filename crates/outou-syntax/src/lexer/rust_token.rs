//! A coarse, exact-where-it-matters tokenizer for the Rust portions of a
//! `.rsx` file.
//!
//! Outou never needs a full Rust parser: it only needs to know, byte by
//! byte, where one Rust token ends and the next begins, so that it can (a)
//! track the previous significant token for expression-position (grammar
//! §4 rule 1), (b) skip opaque macro/attribute token trees without ever
//! entering a JSX mode inside them (grammar §3), and (c) find the `{` and
//! `}` that delimit a block or an island. Everything else about a token's
//! *meaning* is irrelevant here.
//!
//! This scanner is a pure function of `(source, position)`: tokenizing
//! never depends on anything the caller has seen before. That makes
//! arbitrary lookahead cheap (see `lexer::disambiguate`), since "peek three
//! tokens ahead" is just "call `next_token` three times from a scratch
//! position" with no shared mutable state to unwind.

/// What kind of Rust token was scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtKind {
    /// A plain identifier or keyword.
    Ident,
    /// A raw identifier, `r#ident`.
    RawIdent,
    /// A lifetime, `'a`. Distinct from a char literal.
    Lifetime,
    /// Any literal: string, raw string, byte, byte-string, C-string, char,
    /// or number. Outou does not need to tell these apart internally; it
    /// only needs the token to be atomic so `{`, `}`, `<`, `>` inside it
    /// are never mistaken for structure.
    Literal,
    /// A `//` line comment (including `///` and `//!` doc comments), not
    /// including the trailing line break.
    LineComment,
    /// A `/* ... */` block comment, arbitrarily nested.
    BlockComment,
    /// A single opening delimiter: `(`, `[`, `{`.
    OpenDelim,
    /// A single closing delimiter: `)`, `]`, `}`.
    CloseDelim,
    /// Any other punctuation, possibly multi-character (`::`, `->`, `=>`,
    /// `..=`, `&&`, …). Never includes delimiters.
    Punct,
    /// A byte the scanner could not classify (stray non-ASCII control byte,
    /// unterminated literal at end of input, etc.). Recovery, not a panic.
    Unknown,
}

/// One scanned Rust token, as a byte range into the original source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtTok {
    /// The token's kind.
    pub kind: RtKind,
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl RtTok {
    /// The token's exact source text.
    pub fn text<'s>(&self, source: &'s str) -> &'s str {
        &source[self.start..self.end]
    }
}

const EOF: RtTok = RtTok {
    kind: RtKind::Unknown,
    start: 0,
    end: 0,
};

/// Returns the byte at `pos`, or `None` past the end of `bytes`.
fn at(bytes: &[u8], pos: usize) -> Option<u8> {
    bytes.get(pos).copied()
}

/// Skips ASCII whitespace starting at `pos`. Only ASCII whitespace bytes are
/// meaningful token separators in Rust source.
fn skip_whitespace(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(at(bytes, pos), Some(b) if b.is_ascii_whitespace()) {
        pos += 1;
    }
    pos
}

/// Advances past one UTF-8 scalar value starting at `pos`, so the scanner
/// never splits a multi-byte character even when it does not otherwise
/// understand it.
///
/// `pos` may already be at or past end of input (a trailing, unescaped
/// backslash at end of a string/char body is exactly this case, H1): in
/// that case there is no character to advance past, so `pos` is returned
/// unchanged rather than computing `pos + 1`, which would place `end` one
/// byte beyond `bytes.len()` and panic the first time it was used to slice
/// `bytes` or the source string.
fn advance_char(bytes: &[u8], pos: usize) -> usize {
    if pos >= bytes.len() {
        return pos;
    }
    let mut end = pos + 1;
    while matches!(at(bytes, end), Some(b) if b & 0b1100_0000 == 0b1000_0000) {
        end += 1;
    }
    end
}

fn is_ident_start(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphabetic() || c >= 0x80
}

fn is_ident_continue(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric() || c >= 0x80
}

/// Scans a run of identifier bytes (ASCII fast path; non-ASCII bytes are
/// accepted too since Rust identifiers allow most Unicode XID continue
/// characters, and Outou never needs to validate that precisely).
fn scan_ident_run(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(at(bytes, pos), Some(b) if is_ident_continue(b)) {
        pos = advance_char(bytes, pos);
    }
    pos
}

/// Scans the body of a `"..."` string starting just after the opening quote,
/// honoring backslash escapes. Returns the position just after the closing
/// quote, or end of input if the string is unterminated (recovery, not an
/// error: an unterminated string still consumes to EOF as one literal).
fn scan_string_body(bytes: &[u8], mut pos: usize) -> usize {
    loop {
        match at(bytes, pos) {
            None => return pos,
            Some(b'"') => return pos + 1,
            Some(b'\\') => pos = advance_char(bytes, advance_char(bytes, pos)),
            Some(_) => pos = advance_char(bytes, pos),
        }
    }
}

/// Scans the body of a `r#"..."#`-style raw string given the hash count,
/// starting just after the opening quote. Returns the position just after
/// the closing quote + hashes, or end of input if unterminated.
fn scan_raw_string_body(bytes: &[u8], mut pos: usize, hashes: usize) -> usize {
    loop {
        match at(bytes, pos) {
            None => return pos,
            Some(b'"') => {
                let after_quote = pos + 1;
                let mut h = 0;
                while h < hashes && at(bytes, after_quote + h) == Some(b'#') {
                    h += 1;
                }
                if h == hashes {
                    return after_quote + h;
                }
                pos = advance_char(bytes, pos);
            }
            Some(_) => pos = advance_char(bytes, pos),
        }
    }
}

/// Counts the `#` characters starting at `pos` (used to detect `r#"`,
/// `r##"`, … and their byte/C variants).
fn count_hashes(bytes: &[u8], mut pos: usize) -> usize {
    let start = pos;
    while at(bytes, pos) == Some(b'#') {
        pos += 1;
    }
    pos - start
}

/// Tries to scan a raw string (`r"…"`, `r#"…"#`, …) starting at `pos`
/// (which must be the `r`). Returns the end position if this really is a
/// raw string, `None` if `r` was just the start of an ordinary identifier.
fn try_scan_raw_string(bytes: &[u8], pos: usize) -> Option<usize> {
    let hashes_start = pos + 1;
    let hashes = count_hashes(bytes, hashes_start);
    let quote_pos = hashes_start + hashes;
    if at(bytes, quote_pos) == Some(b'"') {
        Some(scan_raw_string_body(bytes, quote_pos + 1, hashes))
    } else {
        None
    }
}

/// Scans a char literal body (after the opening `'`), given that the caller
/// has already decided this is a char, not a lifetime.
fn scan_char_body(bytes: &[u8], mut pos: usize) -> usize {
    // One escape (possibly `\u{...}`) or one scalar value.
    if at(bytes, pos) == Some(b'\\') {
        pos = advance_char(bytes, pos);
        if at(bytes, pos) == Some(b'u') {
            pos = advance_char(bytes, pos);
            if at(bytes, pos) == Some(b'{') {
                pos = advance_char(bytes, pos);
                while matches!(at(bytes, pos), Some(b) if b != b'}' && b != b'\'') {
                    pos = advance_char(bytes, pos);
                }
                if at(bytes, pos) == Some(b'}') {
                    pos = advance_char(bytes, pos);
                }
            }
        } else if at(bytes, pos).is_some() {
            pos = advance_char(bytes, pos);
        }
    } else if at(bytes, pos).is_some() {
        pos = advance_char(bytes, pos);
    }
    // Consume until the closing quote (bounded: real Rust char literals are
    // short; an unterminated one just runs to end of input or newline).
    while let Some(b) = at(bytes, pos) {
        if b == b'\'' {
            return pos + 1;
        }
        if b == b'\n' {
            return pos;
        }
        pos = advance_char(bytes, pos);
    }
    pos
}

/// Decides whether `'` at `pos` starts a lifetime or a char literal, per
/// grammar §2.1: `'IDENT` not followed by another `'` is a lifetime;
/// `'IDENT'` or `'<escape>'` is a char literal.
fn scan_quote(bytes: &[u8], pos: usize) -> RtTok {
    let after_quote = pos + 1;
    match at(bytes, after_quote) {
        Some(b'\\') => RtTok {
            kind: RtKind::Literal,
            start: pos,
            end: scan_char_body(bytes, after_quote),
        },
        Some(c) if is_ident_start(c) => {
            let ident_end = scan_ident_run(bytes, after_quote);
            // `'static`, `'a`, … followed by a closing `'` is a one-char
            // char literal (`'a'`); otherwise it is a lifetime. "One char"
            // means one Unicode *scalar value*, not one byte (M2): `'é'`
            // is a single-character char literal even though `é` is two
            // UTF-8 bytes, so the ident run must span exactly one
            // `advance_char` step, not exactly one byte.
            if advance_char(bytes, after_quote) == ident_end && at(bytes, ident_end) == Some(b'\'')
            {
                RtTok {
                    kind: RtKind::Literal,
                    start: pos,
                    end: ident_end + 1,
                }
            } else {
                RtTok {
                    kind: RtKind::Lifetime,
                    start: pos,
                    end: ident_end,
                }
            }
        }
        Some(_) => RtTok {
            kind: RtKind::Literal,
            start: pos,
            end: scan_char_body(bytes, after_quote),
        },
        None => RtTok {
            kind: RtKind::Unknown,
            start: pos,
            end: after_quote,
        },
    }
}

/// Scans a numeric literal starting at `pos` (a digit). Outou does not need
/// to validate the number, only to consume it atomically, including any
/// suffix (`1u32`, `1.0f64`, `0x1_00`).
fn scan_number(bytes: &[u8], mut pos: usize) -> usize {
    // Hex/octal/binary prefix.
    if at(bytes, pos) == Some(b'0') {
        if let Some(b'x') | Some(b'o') | Some(b'b') = at(bytes, pos + 1) {
            pos += 2;
            while matches!(at(bytes, pos), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
                pos += 1;
            }
            return pos;
        }
    }
    while matches!(at(bytes, pos), Some(b) if b.is_ascii_digit() || b == b'_') {
        pos += 1;
    }
    if at(bytes, pos) == Some(b'.') && matches!(at(bytes, pos + 1), Some(b) if b.is_ascii_digit()) {
        pos += 1;
        while matches!(at(bytes, pos), Some(b) if b.is_ascii_digit() || b == b'_') {
            pos += 1;
        }
    }
    if let Some(b'e') | Some(b'E') = at(bytes, pos) {
        let mut look = pos + 1;
        if let Some(b'+') | Some(b'-') = at(bytes, look) {
            look += 1;
        }
        if matches!(at(bytes, look), Some(b) if b.is_ascii_digit()) {
            pos = look;
            while matches!(at(bytes, pos), Some(b) if b.is_ascii_digit() || b == b'_') {
                pos += 1;
            }
        }
    }
    // Suffix: an identifier glued to the number (`u32`, `f64`, `usize`, …).
    while matches!(at(bytes, pos), Some(b) if is_ident_continue(b)) {
        pos += 1;
    }
    pos
}

/// Scans a nested `/* ... */` block comment starting at `pos` (the first
/// `/`). Nesting is tracked exactly, per grammar's Rust-scanner
/// requirements.
fn scan_block_comment(bytes: &[u8], pos: usize) -> usize {
    let mut depth: u32 = 1;
    let mut cursor = pos + 2;
    while depth > 0 {
        match (at(bytes, cursor), at(bytes, cursor + 1)) {
            (Some(b'/'), Some(b'*')) => {
                depth += 1;
                cursor += 2;
            }
            (Some(b'*'), Some(b'/')) => {
                depth -= 1;
                cursor += 2;
            }
            (Some(_), _) => cursor = advance_char(bytes, cursor),
            (None, _) => return cursor,
        }
    }
    cursor
}

/// Scans exactly one significant token (trivia consumed as a leading step,
/// never returned itself except comments) starting at `pos`.
///
/// Whitespace before the token is skipped silently: callers that need the
/// original bytes back use `source[prev.end..next.start]`, which still
/// includes any whitespace and comments in between when the caller wants
/// comments included, or can call [`next_token`] repeatedly to see comments
/// as their own tokens.
pub fn next_token(bytes: &[u8], pos: usize) -> RtTok {
    let tok = next_token_inner(bytes, pos);
    debug_assert!(
        tok.end <= bytes.len(),
        "next_token must never return a span past end of input"
    );
    tok
}

fn next_token_inner(bytes: &[u8], pos: usize) -> RtTok {
    let pos = skip_whitespace(bytes, pos);
    let Some(c) = at(bytes, pos) else {
        return RtTok {
            start: pos,
            end: pos,
            ..EOF
        };
    };

    match c {
        b'/' if at(bytes, pos + 1) == Some(b'/') => {
            let mut end = pos + 2;
            while matches!(at(bytes, end), Some(b) if b != b'\n') {
                end = advance_char(bytes, end);
            }
            RtTok {
                kind: RtKind::LineComment,
                start: pos,
                end,
            }
        }
        b'/' if at(bytes, pos + 1) == Some(b'*') => RtTok {
            kind: RtKind::BlockComment,
            start: pos,
            end: scan_block_comment(bytes, pos),
        },
        b'"' => RtTok {
            kind: RtKind::Literal,
            start: pos,
            end: scan_string_body(bytes, pos + 1),
        },
        b'\'' => scan_quote(bytes, pos),
        b'r' if matches!(at(bytes, pos + 1), Some(b'"') | Some(b'#')) => {
            match try_scan_raw_string(bytes, pos) {
                Some(end) => RtTok {
                    kind: RtKind::Literal,
                    start: pos,
                    end,
                },
                None => scan_ident_like(bytes, pos),
            }
        }
        b'b' if at(bytes, pos + 1) == Some(b'"') => RtTok {
            kind: RtKind::Literal,
            start: pos,
            end: scan_string_body(bytes, pos + 2),
        },
        b'b' if at(bytes, pos + 1) == Some(b'\'') => {
            let inner = scan_quote(bytes, pos + 1);
            RtTok {
                kind: RtKind::Literal,
                start: pos,
                end: inner.end,
            }
        }
        b'b' if matches!(at(bytes, pos + 1), Some(b'r')) => {
            match try_scan_raw_string(bytes, pos + 1) {
                Some(end) => RtTok {
                    kind: RtKind::Literal,
                    start: pos,
                    end,
                },
                None => scan_ident_like(bytes, pos),
            }
        }
        b'c' if at(bytes, pos + 1) == Some(b'"') => RtTok {
            kind: RtKind::Literal,
            start: pos,
            end: scan_string_body(bytes, pos + 2),
        },
        b'c' if matches!(at(bytes, pos + 1), Some(b'r')) => {
            match try_scan_raw_string(bytes, pos + 1) {
                Some(end) => RtTok {
                    kind: RtKind::Literal,
                    start: pos,
                    end,
                },
                None => scan_ident_like(bytes, pos),
            }
        }
        b'0'..=b'9' => RtTok {
            kind: RtKind::Literal,
            start: pos,
            end: scan_number(bytes, pos),
        },
        b'(' | b'[' | b'{' => RtTok {
            kind: RtKind::OpenDelim,
            start: pos,
            end: pos + 1,
        },
        b')' | b']' | b'}' => RtTok {
            kind: RtKind::CloseDelim,
            start: pos,
            end: pos + 1,
        },
        c if is_ident_start(c) => scan_ident_like(bytes, pos),
        _ => scan_punct(bytes, pos),
    }
}

/// Scans an identifier, keyword, or raw identifier (`r#ident`).
fn scan_ident_like(bytes: &[u8], pos: usize) -> RtTok {
    if at(bytes, pos) == Some(b'r') && at(bytes, pos + 1) == Some(b'#') {
        let name_start = pos + 2;
        if matches!(at(bytes, name_start), Some(b) if is_ident_start(b)) {
            return RtTok {
                kind: RtKind::RawIdent,
                start: pos,
                end: scan_ident_run(bytes, name_start),
            };
        }
    }
    RtTok {
        kind: RtKind::Ident,
        start: pos,
        end: scan_ident_run(bytes, pos),
    }
}

/// Multi-character punctuation, longest match first. `<` and `>` are
/// deliberately included here (ordinary Rust tokenization merges `<<`,
/// `>>`, `<=`, `>=`, …); the disambiguation scanner in
/// `lexer::disambiguate` uses its own single-character view of `<`/`>`
/// where the grammar requires it (§4 rule 3).
const MULTI_PUNCT: &[&str] = &[
    "<<=", ">>=", "..=", "...", "::", "->", "=>", "<<", ">>", ">=", "<=", "&&", "||", "..", "+=",
    "-=", "*=", "/=", "%=", "^=", "&=", "|=", "==", "!=",
];

fn scan_punct(bytes: &[u8], pos: usize) -> RtTok {
    for candidate in MULTI_PUNCT {
        let end = pos + candidate.len();
        if bytes.len() >= end && &bytes[pos..end] == candidate.as_bytes() {
            return RtTok {
                kind: RtKind::Punct,
                start: pos,
                end,
            };
        }
    }
    // A single byte of punctuation. Non-ASCII stray bytes are `Unknown` but
    // still consumed one scalar value at a time so the scanner never gets
    // stuck.
    let end = advance_char(bytes, pos);
    let kind = if bytes[pos].is_ascii() {
        RtKind::Punct
    } else {
        RtKind::Unknown
    };
    RtTok {
        kind,
        start: pos,
        end,
    }
}

/// Returns the next token that is not a comment, per grammar §4: "Trivia
/// (whitespace, comments) is skipped when identifying a token."
pub fn next_significant(bytes: &[u8], mut pos: usize) -> RtTok {
    loop {
        let tok = next_token(bytes, pos);
        if !matches!(tok.kind, RtKind::LineComment | RtKind::BlockComment) {
            return tok;
        }
        pos = tok.end;
    }
}

/// Skips a `(...)`, `[...]` or `{...}` group starting at its opening
/// delimiter, returning the position just after the matching close.
///
/// Delimiter *kind* is not checked against a stack (Outou never needs to
/// validate Rust's own delimiter matching, and a mismatch is rustc's error
/// to report). An unterminated group runs to end of input: safe recovery,
/// never a panic or an infinite loop. Shared by [`super::disambiguate`]'s
/// rule 3 (skipping a parenthesized/bracketed group during the angle scan)
/// and [`super::opaque`] (skipping a whole macro or attribute token tree).
pub fn skip_balanced_group(bytes: &[u8], open_pos: usize) -> usize {
    let mut depth: u32 = 1;
    let mut pos = open_pos + 1;
    loop {
        let tok = next_significant(bytes, pos);
        if tok.start == tok.end {
            return pos; // end of input
        }
        match tok.kind {
            RtKind::OpenDelim => depth += 1,
            RtKind::CloseDelim => {
                depth -= 1;
                if depth == 0 {
                    return tok.end;
                }
            }
            _ => {}
        }
        pos = tok.end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(RtKind, &str)> {
        let bytes = source.as_bytes();
        let mut pos = 0;
        let mut out = Vec::new();
        loop {
            let tok = next_token(bytes, pos);
            if tok.start == tok.end && at(bytes, pos).is_none() {
                break;
            }
            out.push((tok.kind, tok.text(source)));
            pos = tok.end;
        }
        out
    }

    #[test]
    fn identifiers_and_raw_identifiers() {
        assert_eq!(
            kinds("foo r#match bar"),
            vec![
                (RtKind::Ident, "foo"),
                (RtKind::RawIdent, "r#match"),
                (RtKind::Ident, "bar"),
            ]
        );
    }

    #[test]
    fn lifetime_vs_char_literal() {
        assert_eq!(kinds("'a"), vec![(RtKind::Lifetime, "'a")]);
        assert_eq!(kinds("'a'"), vec![(RtKind::Literal, "'a'")]);
        assert_eq!(kinds("'\\n'"), vec![(RtKind::Literal, "'\\n'")]);
        assert_eq!(kinds("'static"), vec![(RtKind::Lifetime, "'static")]);
    }

    /// Regression for M2: a non-ASCII character is one Unicode scalar value
    /// but more than one byte, so deciding "closed by a following `'`"
    /// (char literal) versus "not" (lifetime) by comparing byte lengths
    /// mistook `'é'` for a lifetime followed by a stray quote.
    #[test]
    fn non_ascii_char_literal_is_not_a_lifetime() {
        assert_eq!(kinds("'é'"), vec![(RtKind::Literal, "'é'")]);
        assert_eq!(kinds("'é"), vec![(RtKind::Lifetime, "'é")]);
    }

    #[test]
    fn string_literal_forms() {
        assert_eq!(kinds(r#""hi""#), vec![(RtKind::Literal, r#""hi""#)]);
        assert_eq!(
            kinds(r##"r#"hi"#"##),
            vec![(RtKind::Literal, r##"r#"hi"#"##)]
        );
        assert_eq!(kinds(r#"b"hi""#), vec![(RtKind::Literal, r#"b"hi""#)]);
        assert_eq!(kinds(r#"b'x'"#), vec![(RtKind::Literal, r#"b'x'"#)]);
        assert_eq!(kinds(r#"c"hi""#), vec![(RtKind::Literal, r#"c"hi""#)]);
        assert_eq!(
            kinds(r##"br#"hi"#"##),
            vec![(RtKind::Literal, r##"br#"hi"#"##)]
        );
        assert_eq!(
            kinds(r##"cr#"hi"#"##),
            vec![(RtKind::Literal, r##"cr#"hi"#"##)]
        );
    }

    #[test]
    fn numeric_literals() {
        assert_eq!(kinds("1_000u32"), vec![(RtKind::Literal, "1_000u32")]);
        assert_eq!(kinds("1.0f64"), vec![(RtKind::Literal, "1.0f64")]);
        assert_eq!(kinds("0x1_00"), vec![(RtKind::Literal, "0x1_00")]);
        assert_eq!(kinds("1e10"), vec![(RtKind::Literal, "1e10")]);
    }

    #[test]
    fn comments_line_block_nested_doc() {
        assert_eq!(kinds("// hi"), vec![(RtKind::LineComment, "// hi")]);
        assert_eq!(kinds("/// doc"), vec![(RtKind::LineComment, "/// doc")]);
        assert_eq!(kinds("//! doc"), vec![(RtKind::LineComment, "//! doc")]);
        assert_eq!(
            kinds("/* a /* b */ c */"),
            vec![(RtKind::BlockComment, "/* a /* b */ c */"),]
        );
    }

    #[test]
    fn multi_char_punctuation() {
        for op in [
            "::", "->", "=>", "<<", ">>", ">=", "<=", "&&", "||", "..", "..=", "...",
        ] {
            assert_eq!(kinds(op), vec![(RtKind::Punct, op)], "op {op}");
        }
        assert_eq!(kinds("<<="), vec![(RtKind::Punct, "<<=")]);
        assert_eq!(kinds(">>="), vec![(RtKind::Punct, ">>=")]);
    }

    #[test]
    fn delimiters_are_their_own_kind() {
        assert_eq!(
            kinds("(){}[]"),
            vec![
                (RtKind::OpenDelim, "("),
                (RtKind::CloseDelim, ")"),
                (RtKind::OpenDelim, "{"),
                (RtKind::CloseDelim, "}"),
                (RtKind::OpenDelim, "["),
                (RtKind::CloseDelim, "]"),
            ]
        );
    }

    #[test]
    fn next_significant_skips_comments() {
        let bytes = "// c\nfoo".as_bytes();
        let tok = next_significant(bytes, 0);
        assert_eq!(tok.text("// c\nfoo"), "foo");
    }

    #[test]
    fn unterminated_string_reaches_eof_without_panic() {
        let bytes = "\"abc".as_bytes();
        let tok = next_token(bytes, 0);
        assert_eq!(tok.kind, RtKind::Literal);
        assert_eq!(tok.end, bytes.len());
    }
}
