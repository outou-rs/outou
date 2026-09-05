use std::path::Path;

use serde::{Deserialize, Serialize};

/// Byte range `[start, end)` inside one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    /// Inclusive start byte offset.
    pub start: u32,
    /// Exclusive end byte offset.
    pub end: u32,
}

impl Span {
    /// Creates a span. `end` must not be less than `start`; this constructor
    /// does not validate. Values that reach the crate from outside are
    /// checked at the boundaries ([`SourceMapBuilder::try_with_mapping`] and
    /// [`SourceMapJson::into_source_map`]).
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Whether two spans share at least one byte. An empty span at a
    /// position inside `other` also counts as overlapping, so that cursor
    /// positions can be mapped.
    ///
    /// [`crate::SourceMap::map_range`] deliberately uses a stricter rule
    /// (see there).
    pub const fn overlaps(self, other: Span) -> bool {
        self.start < other.end && other.start < self.end
            || (self.start == self.end && other.start <= self.start && self.start <= other.end)
            || (other.start == other.end && self.start <= other.start && other.start <= self.end)
    }

    /// Whether `self` fully contains `other`, including when they are
    /// equal. An empty `other` counts as contained when it falls anywhere
    /// inside (or at either edge of) `self`, so that a cursor position can
    /// be tested for containment.
    pub const fn contains(self, other: Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

/// Index into [`crate::SourceMap::sources`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub usize);

/// A span inside a specific source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    /// Which source file.
    pub source: SourceId,
    /// Span inside that file.
    pub span: Span,
}

impl SourceSpan {
    /// Pairs a source id with a span.
    pub const fn new(source: SourceId, span: Span) -> Self {
        Self { source, span }
    }
}

/// A document URI as exchanged with editors and rust-analyzer.
///
/// Kept as a plain string on purpose: the registry only needs equality and
/// hashing, and every LSP implementation already round-trips strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uri(String);

impl Uri {
    /// Wraps a URI string.
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    /// The URI as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A `file://` URI for an absolute filesystem path (issue #8 fix list step
/// 6, MEDIUM-8): `/a/b/c.rs` becomes `file:///a/b/c.rs`. Every byte
/// outside `A-Za-z0-9-._~/:` is percent-encoded — the previous
/// `format!("file://{}", path.display())` was not a URI at all, so a
/// space or `#` byte in the path (e.g. from a workspace directory name)
/// produced text a conforming URI parser reads wrong (a `#` starts a
/// fragment). `\` is treated as a path separator so a Windows path
/// (`C:\work\a.rs`) becomes `file:///C:/work/a.rs`, prefixing the leading
/// `/` a drive letter does not already have.
///
/// Lives here (rather than in `outou-cli`, where this was first written)
/// so the future `outou-lsp` can share it without a cross-crate
/// dependency on `outou-cli`.
pub fn file_uri(path: &Path) -> Uri {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::from("file://");
    if !normalized.starts_with('/') {
        encoded.push('/');
    }
    for byte in normalized.bytes() {
        if is_uri_safe_byte(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    Uri::new(encoded)
}

/// Whether `byte` may appear unescaped in the URI this module produces:
/// RFC 3986's `unreserved` set, plus `/` (path separator) and `:` (a
/// Windows drive letter's own separator, and otherwise never present in
/// an absolute filesystem path).
fn is_uri_safe_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
}

/// The uppercase hex digit for a nibble (`0..=15`), for percent-encoding.
fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + (nibble - 10)) as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_reflexive_and_respects_edges() {
        let outer = Span::new(2, 10);
        assert!(outer.contains(outer));
        assert!(outer.contains(Span::new(2, 8)));
        assert!(outer.contains(Span::new(2, 10)));
        assert!(!outer.contains(Span::new(2, 11)));
        assert!(!outer.contains(Span::new(1, 10)));
    }

    #[test]
    fn contains_accepts_empty_spans_at_the_edges() {
        let outer = Span::new(4, 8);
        assert!(outer.contains(Span::new(4, 4)));
        assert!(outer.contains(Span::new(8, 8)));
        assert!(outer.contains(Span::new(6, 6)));
        assert!(!outer.contains(Span::new(9, 9)));
    }

    #[test]
    fn overlap_but_not_contained() {
        let a = Span::new(0, 5);
        let b = Span::new(3, 8);
        assert!(a.overlaps(b));
        assert!(!a.contains(b));
        assert!(!b.contains(a));
    }

    #[test]
    fn file_uri_percent_encodes_a_space_and_a_hash() {
        // MEDIUM-8, issue #8: a bare `format!("file://{}", ...)` left a
        // literal `#` in place, which a conforming URI parser reads as
        // the start of a fragment, silently truncating the path.
        let uri = file_uri(std::path::Path::new("/a b/c#1.rs"));
        assert_eq!(uri.as_str(), "file:///a%20b/c%231.rs");
    }

    #[test]
    fn file_uri_percent_encodes_non_ascii_utf8_bytes() {
        let uri = file_uri(std::path::Path::new("/a/日本.rs"));
        let expected: String = "日本"
            .bytes()
            .map(|b| format!("%{b:02X}"))
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(uri.as_str(), format!("file:///a/{expected}.rs"));
    }

    #[test]
    fn file_uri_prefixes_a_slash_before_a_windows_drive_letter() {
        // `\` is treated as a path separator, and a `/` is prefixed,
        // regardless of the host platform — `Path` on a non-Windows host
        // never splits on `\` either, so this exercises `file_uri`'s own
        // manual translation, not `Path`'s.
        let uri = file_uri(std::path::Path::new("C:\\work\\a.rs"));
        assert_eq!(uri.as_str(), "file:///C:/work/a.rs");
    }
}
