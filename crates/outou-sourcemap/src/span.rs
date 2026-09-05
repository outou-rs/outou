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
    /// Creates a span. `end` must not be smaller than `start`.
    pub const fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    /// Whether two spans share at least one byte. An empty span at a
    /// position inside `other` also counts as overlapping, so that cursor
    /// positions can be mapped.
    pub const fn overlaps(self, other: Span) -> bool {
        self.start < other.end && other.start < self.end
            || (self.start == self.end && other.start <= self.start && self.start <= other.end)
            || (other.start == other.end && self.start <= other.start && other.start <= self.end)
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
