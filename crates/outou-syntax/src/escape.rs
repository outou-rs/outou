//! Decoding Rust's plain (non-raw) string-literal escape sequences.
//!
//! Grammar §5.1 (`docs/grammar.md`) says a plain attribute string value
//! "uses Rust's own string literal grammar", which includes escape
//! processing — the decoded string, not the raw source slice, is what
//! [`crate::ast::JsxText::value`] must carry for a plain string (MEDIUM-8,
//! issue #6 fix list item 9). Raw strings (`r"…"`, `r#"…"#`) have no
//! escapes at all, so their content is used as-is and never passed
//! through this module.

/// Decodes Rust's plain string-literal escapes in `content` — the bytes
/// between the opening and closing `"`, with those quotes already
/// stripped by the caller.
///
/// Handles every escape a plain string literal can contain: `\\`, `\"`,
/// `\'`, `\0`, `\n`, `\r`, `\t`, `\x` followed by two hex digits whose
/// value is at most `0x7F` (a plain string's ASCII escape — above `0x7F`
/// is a byte-string-only escape, already rejected before this ever
/// runs), `\u{…}` (one to six hex digits naming a Unicode scalar value),
/// and the string-continuation escape (a `\` immediately followed by a
/// newline, which consumes that newline and any leading whitespace on
/// the line after it, per Rust's own grammar).
///
/// The lexer ([`crate::lexer::rust_token`]) already accepted the literal
/// this content came from as a whole; this function's job is decoding,
/// not re-validating. An escape it does not recognize, or one whose
/// operand is missing or malformed (a truncated `\x`, an unclosed
/// `\u{`, a `\u{}` naming a value that is not a valid Unicode scalar
/// value), is left exactly as written — backslash and all — rather than
/// panicking or dropping bytes.
pub(crate) fn decode_plain_string_escapes(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut pos = 0usize;
    while pos < bytes.len() {
        if bytes[pos] != b'\\' {
            let len = utf8_char_len(bytes[pos]);
            let end = (pos + len).min(bytes.len());
            out.push_str(&content[pos..end]);
            pos = end;
            continue;
        }
        let Some(&next) = bytes.get(pos + 1) else {
            // A trailing lone backslash is not a real escape; the lexer
            // would not have closed the literal here, but keep it
            // verbatim rather than panicking if it somehow does.
            out.push('\\');
            pos += 1;
            continue;
        };
        match next {
            b'\\' => {
                out.push('\\');
                pos += 2;
            }
            b'"' => {
                out.push('"');
                pos += 2;
            }
            b'\'' => {
                out.push('\'');
                pos += 2;
            }
            b'0' => {
                out.push('\0');
                pos += 2;
            }
            b'n' => {
                out.push('\n');
                pos += 2;
            }
            b'r' => {
                out.push('\r');
                pos += 2;
            }
            b't' => {
                out.push('\t');
                pos += 2;
            }
            b'\n' => {
                pos += 2;
                while pos < bytes.len() && matches!(bytes[pos], b' ' | b'\t' | b'\n' | b'\r') {
                    pos += 1;
                }
            }
            b'x' => match decode_ascii_escape(bytes, pos + 2) {
                Some((ch, new_pos)) => {
                    out.push(ch);
                    pos = new_pos;
                }
                None => {
                    out.push('\\');
                    pos += 1;
                }
            },
            b'u' => match decode_unicode_escape(bytes, pos + 2) {
                Some((ch, new_pos)) => {
                    out.push(ch);
                    pos = new_pos;
                }
                None => {
                    out.push('\\');
                    pos += 1;
                }
            },
            _ => {
                // Unknown escape: keep the backslash; the next iteration
                // handles `next` as ordinary content.
                out.push('\\');
                pos += 1;
            }
        }
    }
    out
}

/// The byte length of the UTF-8 character starting at a byte whose value
/// is `first`.
fn utf8_char_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// Decodes `\xNN` (already past the `\x`, `pos` at the first hex digit),
/// returning the ASCII character and the position just after it, or
/// `None` if the two hex digits are missing/invalid or the value exceeds
/// `0x7F` (out of range for a plain string's `\x` escape).
fn decode_ascii_escape(bytes: &[u8], pos: usize) -> Option<(char, usize)> {
    let hex = bytes.get(pos..pos + 2)?;
    let text = std::str::from_utf8(hex).ok()?;
    let value = u8::from_str_radix(text, 16).ok()?;
    if value > 0x7F {
        return None;
    }
    Some((value as char, pos + 2))
}

/// Decodes `\u{…}` (already past the `\u`, `pos` at the expected `{`),
/// returning the decoded character and the position just after the
/// closing `}`, or `None` if the braces or hex digits are malformed or
/// the value is not a valid Unicode scalar value.
fn decode_unicode_escape(bytes: &[u8], pos: usize) -> Option<(char, usize)> {
    if bytes.get(pos) != Some(&b'{') {
        return None;
    }
    let digits_start = pos + 1;
    let mut end = digits_start;
    while end < bytes.len() && bytes[end] != b'}' && end - digits_start < 6 {
        end += 1;
    }
    if end == digits_start || bytes.get(end) != Some(&b'}') {
        return None;
    }
    let text = std::str::from_utf8(&bytes[digits_start..end]).ok()?;
    let value = u32::from_str_radix(text, 16).ok()?;
    let ch = char::from_u32(value)?;
    Some((ch, end + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_escapes_decode() {
        assert_eq!(decode_plain_string_escapes("tab\\there"), "tab\there");
        assert_eq!(decode_plain_string_escapes("a\\nb"), "a\nb");
        assert_eq!(decode_plain_string_escapes("a\\rb"), "a\rb");
        assert_eq!(decode_plain_string_escapes("a\\\\b"), "a\\b");
        assert_eq!(decode_plain_string_escapes("a\\\"b"), "a\"b");
        assert_eq!(decode_plain_string_escapes("a\\'b"), "a'b");
        assert_eq!(decode_plain_string_escapes("a\\0b"), "a\0b");
    }

    #[test]
    fn ascii_and_unicode_escapes_decode() {
        assert_eq!(decode_plain_string_escapes("\\x41"), "A");
        assert_eq!(decode_plain_string_escapes("\\u{1F600}"), "\u{1F600}");
        assert_eq!(decode_plain_string_escapes("\\u{41}"), "A");
    }

    #[test]
    fn out_of_range_ascii_escape_is_left_verbatim() {
        // 0xFF is not a valid plain-string `\x` escape (byte-string-only).
        assert_eq!(decode_plain_string_escapes("\\xFF"), "\\xFF");
    }

    #[test]
    fn malformed_escapes_are_left_verbatim() {
        assert_eq!(decode_plain_string_escapes("\\x"), "\\x");
        assert_eq!(decode_plain_string_escapes("\\u{"), "\\u{");
        assert_eq!(decode_plain_string_escapes("\\q"), "\\q");
        assert_eq!(decode_plain_string_escapes("trailing\\"), "trailing\\");
    }

    #[test]
    fn string_continuation_trims_following_whitespace() {
        assert_eq!(decode_plain_string_escapes("a\\\n    b"), "ab");
    }

    #[test]
    fn plain_text_with_no_escapes_round_trips() {
        assert_eq!(decode_plain_string_escapes("Hello, world"), "Hello, world");
    }

    #[test]
    fn non_ascii_text_round_trips() {
        assert_eq!(decode_plain_string_escapes("café"), "café");
    }
}
