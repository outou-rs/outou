//! Incremental construction of a [`SourceMap`], for codegen.

use crate::{Mapping, MappingKind, SourceId, SourceMap, SourceSpan, Span, Uri};

/// Errors [`SourceMapBuilder::try_with_mapping`] rejects a mapping for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildError {
    /// The generated span's `end` is before its `start`.
    #[error("generated span is inverted: {start}..{end}")]
    InvertedGeneratedSpan {
        /// The span's start offset.
        start: u32,
        /// The span's end offset.
        end: u32,
    },
    /// A source span's `end` is before its `start`.
    #[error("source span is inverted: {start}..{end}")]
    InvertedSourceSpan {
        /// The span's start offset.
        start: u32,
        /// The span's end offset.
        end: u32,
    },
    /// A source span referenced a [`SourceId`] that was never registered
    /// with [`SourceMapBuilder::with_source`].
    #[error(
        "mapping references source id {index}, but only {registered} source(s) are registered"
    )]
    UnknownSourceId {
        /// The unknown source index.
        index: usize,
        /// How many sources are currently registered.
        registered: usize,
    },
}

/// Builds a [`SourceMap`] incrementally while lowering one `.rsx` file (or
/// several, for a mapping that draws on more than one source) to Rust.
///
/// Immutable style, matching the rest of this crate: every `with_*` method
/// consumes the builder and returns a new one. Mappings may be added in any
/// order; [`SourceMapBuilder::finish`] sorts them by generated start offset
/// so that [`SourceMap::to_source`] and friends can assume that order.
///
/// ```
/// use outou_sourcemap::{MappingKind, SourceMapBuilder, SourceSpan, Span, Uri};
///
/// let (builder, source) = SourceMapBuilder::new(Uri::new("file:///g.rs"))
///     .with_source(Uri::new("file:///a.rsx"));
/// let map = builder
///     .try_with_mapping(
///         Span::new(0, 8),
///         &[SourceSpan::new(source, Span::new(1, 9))],
///         MappingKind::Identifier,
///         None,
///     )
///     .expect("valid mapping")
///     .finish();
/// assert_eq!(map.sources.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct SourceMapBuilder {
    generated: Uri,
    sources: Vec<Uri>,
    mappings: Vec<Mapping>,
}

impl SourceMapBuilder {
    /// Starts a builder for the generated file at `generated`.
    pub fn new(generated: Uri) -> Self {
        Self {
            generated,
            sources: Vec::new(),
            mappings: Vec::new(),
        }
    }

    /// Registers a source file and returns the builder together with the
    /// [`SourceId`] to use for it in subsequent mappings.
    ///
    /// Each call appends a new id, even for a URI already registered;
    /// codegen that wants to reuse an id for a file it visits more than
    /// once should keep its own `Uri -> SourceId` map.
    pub fn with_source(self, source: Uri) -> (Self, SourceId) {
        let id = SourceId(self.sources.len());
        let mut sources = self.sources;
        sources.push(source);
        (Self { sources, ..self }, id)
    }

    /// Adds a mapping from a generated span to zero or more source spans.
    ///
    /// Rejects an inverted generated span, an inverted source span, or a
    /// source span whose [`SourceId`] was never registered with
    /// [`SourceMapBuilder::with_source`], without ever panicking on user
    /// data.
    pub fn try_with_mapping(
        self,
        generated: Span,
        sources: &[SourceSpan],
        kind: MappingKind,
        label: Option<String>,
    ) -> Result<Self, BuildError> {
        if generated.start > generated.end {
            return Err(BuildError::InvertedGeneratedSpan {
                start: generated.start,
                end: generated.end,
            });
        }
        for source in sources {
            if source.span.start > source.span.end {
                return Err(BuildError::InvertedSourceSpan {
                    start: source.span.start,
                    end: source.span.end,
                });
            }
            if source.source.0 >= self.sources.len() {
                return Err(BuildError::UnknownSourceId {
                    index: source.source.0,
                    registered: self.sources.len(),
                });
            }
        }

        let mapping = Mapping {
            generated,
            sources: sources.to_vec(),
            kind,
            label,
        };
        let mut mappings = self.mappings;
        mappings.push(mapping);
        Ok(Self { mappings, ..self })
    }

    /// Finishes the builder into a [`SourceMap`], sorting mappings by
    /// generated start offset (ties keep insertion order).
    pub fn finish(self) -> SourceMap {
        let mut mappings = self.mappings;
        mappings.sort_by_key(|m| m.generated.start);
        SourceMap {
            generated: self.generated,
            sources: self.sources,
            mappings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `<Greeting>Hello</Greeting>` lowers to `Greeting { "Hello" }`: the
    /// opening and closing tag are two source spans that both feed the one
    /// generated identifier, built up through the builder rather than by
    /// hand as the `lib.rs` test does.
    #[test]
    fn open_and_close_tag_produce_one_generated_identifier_with_two_sources() {
        let (builder, source) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));

        let map = builder
            .try_with_mapping(
                Span::new(0, 8),
                &[
                    SourceSpan::new(source, Span::new(1, 9)),
                    SourceSpan::new(source, Span::new(18, 26)),
                ],
                MappingKind::Identifier,
                None,
            )
            .expect("valid mapping")
            .finish();

        assert_eq!(map.mappings.len(), 1);
        assert_eq!(map.mappings[0].sources.len(), 2);
    }

    /// One `.rsx` expression can be lowered into several generated spans
    /// (e.g. a prop value used both to build the node and forwarded to a
    /// child); the builder must preserve every one of them.
    #[test]
    fn one_source_expression_produces_several_generated_spans() {
        let (builder, source) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));

        let map = builder
            .try_with_mapping(
                Span::new(20, 24),
                &[SourceSpan::new(source, Span::new(5, 9))],
                MappingKind::Expression,
                None,
            )
            .expect("valid mapping")
            .try_with_mapping(
                Span::new(40, 44),
                &[SourceSpan::new(source, Span::new(5, 9))],
                MappingKind::Expression,
                Some("forwarded prop".to_string()),
            )
            .expect("valid mapping")
            .finish();

        assert_eq!(
            map.to_generated(source, Span::new(6, 7)).len(),
            2,
            "the one source span must reach both generated spans"
        );
    }

    #[test]
    fn finish_sorts_mappings_by_generated_start() {
        let (builder, source) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));

        let map = builder
            .try_with_mapping(
                Span::new(10, 14),
                &[SourceSpan::new(source, Span::new(0, 4))],
                MappingKind::Expression,
                None,
            )
            .expect("valid mapping")
            .try_with_mapping(
                Span::new(0, 4),
                &[SourceSpan::new(source, Span::new(0, 4))],
                MappingKind::Expression,
                None,
            )
            .expect("valid mapping")
            .finish();

        assert_eq!(map.mappings[0].generated, Span::new(0, 4));
        assert_eq!(map.mappings[1].generated, Span::new(10, 14));
    }

    #[test]
    fn try_with_mapping_rejects_an_inverted_generated_span() {
        let (builder, source) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));

        let err = builder
            .try_with_mapping(
                Span { start: 10, end: 5 },
                &[SourceSpan::new(source, Span::new(0, 1))],
                MappingKind::Expression,
                None,
            )
            .unwrap_err();

        assert_eq!(err, BuildError::InvertedGeneratedSpan { start: 10, end: 5 });
    }

    #[test]
    fn try_with_mapping_rejects_an_unknown_source_id() {
        let (builder, _source) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));

        let err = builder
            .try_with_mapping(
                Span::new(0, 4),
                &[SourceSpan::new(SourceId(4), Span::new(0, 1))],
                MappingKind::Expression,
                None,
            )
            .unwrap_err();

        assert_eq!(
            err,
            BuildError::UnknownSourceId {
                index: 4,
                registered: 1
            }
        );
    }

    #[test]
    fn finish_keeps_insertion_order_for_equal_starts() {
        let (builder, source) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));

        let map = builder
            .try_with_mapping(
                Span::new(0, 10),
                &[SourceSpan::new(source, Span::new(0, 4))],
                MappingKind::Expression,
                Some("first".to_string()),
            )
            .expect("valid mapping")
            .try_with_mapping(
                Span::new(0, 5),
                &[SourceSpan::new(source, Span::new(0, 4))],
                MappingKind::Expression,
                Some("second".to_string()),
            )
            .expect("valid mapping")
            .finish();

        assert_eq!(map.mappings[0].generated, Span::new(0, 10));
        assert_eq!(map.mappings[0].label.as_deref(), Some("first"));
    }

    #[test]
    fn with_source_assigns_increasing_ids() {
        let (builder, first) =
            SourceMapBuilder::new(Uri::new("file:///g.rs")).with_source(Uri::new("file:///a.rsx"));
        let (builder, second) = builder.with_source(Uri::new("file:///b.rsx"));
        let map = builder.finish();

        assert_eq!(first, SourceId(0));
        assert_eq!(second, SourceId(1));
        assert_eq!(map.sources.len(), 2);
    }
}
