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
}
