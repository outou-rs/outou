//! Source maps between `.rsx` files and the Rust generated from them.
//!
//! The mapping is **many-to-many**. A single source span may produce several
//! generated spans, and a single generated span may originate from several
//! source spans (an element name is written once in the generated Rust but
//! appears in both the opening and the closing tag of the `.rsx` file).
//! Closing tags and attribute names frequently have no generated counterpart
//! at all; that is a normal, expected state of a map.
//!
//! # Reverse-mapping rule
//!
//! When a tool receives a location in generated Rust:
//!
//! 1. If a `.rsx` file produced it, map it back to the `.rsx` file.
//! 2. Otherwise, return the Rust location untouched. This covers plain `.rs`
//!    modules and dependency crates, whose locations must not be corrupted.
//!
//! [`Registry`] implements this rule across a whole workspace.

mod builder;
mod json;
mod line_index;
mod registry;
mod span;

pub use builder::{BuildError, SourceMapBuilder};
pub use json::{
    FromJsonError, MappingJson, SourceMapJson, SourceSpanJson, ToJsonError, SOURCE_MAP_JSON_VERSION,
};
pub use line_index::{LineIndex, Position, PositionRange};
pub use registry::Registry;
pub use span::{file_uri, SourceId, SourceSpan, Span, Uri};

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// What kind of syntax a mapping entry describes. Used by tools that treat
/// identifiers, expressions and text differently (rename, hover, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MappingKind {
    /// A Rust or component identifier.
    Identifier,
    /// A Rust expression island (`{ ... }` in JSX, or plain Rust).
    Expression,
    /// A JSX attribute name.
    Attribute,
    /// JSX text content.
    Text,
    /// Anything else.
    Other,
}

/// One entry of a source map: a generated span and all source spans that
/// contributed to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    /// Span inside the generated Rust file.
    pub generated: Span,
    /// Zero or more originating spans. Empty means "synthesized by codegen".
    pub sources: Vec<SourceSpan>,
    /// Classification of the mapped syntax.
    pub kind: MappingKind,
    /// Optional human-readable note (the spike JSON's `what` field), for
    /// tools that show provenance to a developer. Not interpreted by this
    /// crate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Mapping {
    /// Creates a mapping with no label.
    pub fn new(generated: Span, sources: Vec<SourceSpan>, kind: MappingKind) -> Self {
        Self {
            generated,
            sources,
            kind,
            label: None,
        }
    }

    /// Returns a new mapping with `label` attached.
    pub fn with_label(self, label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            ..self
        }
    }
}

/// Result of [`SourceMap::map_range`]: the reverse-mapping query an LSP
/// implementation runs when it receives a location in generated Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapped {
    /// Source spans that apply to the queried generated span, in mapping
    /// order and deduplicated.
    pub sources: Vec<SourceSpan>,
    /// `true` only when no mapping intersects the generated span at all. A
    /// matched mapping with an empty `sources` list (synthesized code) is
    /// not unmapped: it correctly has no source.
    pub unmapped: bool,
}

/// Source map for a single generated Rust file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMap {
    /// URI of the generated Rust file.
    pub generated: Uri,
    /// URIs of the `.rsx` files that contributed, indexed by [`SourceId`].
    pub sources: Vec<Uri>,
    /// Mapping entries, in generated-span order.
    pub mappings: Vec<Mapping>,
}

impl SourceMap {
    /// Creates an empty map for `generated` with the given source files.
    pub fn new(generated: Uri, sources: Vec<Uri>) -> Self {
        Self {
            generated,
            sources,
            mappings: Vec::new(),
        }
    }

    /// Returns a new map with `mapping` appended.
    pub fn with_mapping(self, mapping: Mapping) -> Self {
        let mut mappings = self.mappings;
        mappings.push(mapping);
        Self { mappings, ..self }
    }

    /// All source spans that contributed to a span of the generated file.
    ///
    /// Several entries may overlap `generated`; the union of their sources is
    /// returned in mapping order.
    pub fn to_source(&self, generated: Span) -> Vec<SourceSpan> {
        self.mappings
            .iter()
            .filter(|m| m.generated.overlaps(generated))
            .flat_map(|m| m.sources.iter().copied())
            .collect()
    }

    /// All generated spans that a span of source `source` contributed to.
    pub fn to_generated(&self, source: SourceId, span: Span) -> Vec<Span> {
        self.mappings
            .iter()
            .filter(|m| {
                m.sources
                    .iter()
                    .any(|s| s.source == source && s.span.overlaps(span))
            })
            .map(|m| m.generated)
            .collect()
    }

    /// Resolves a [`SourceId`] to its URI.
    pub fn source_uri(&self, id: SourceId) -> Option<&Uri> {
        self.sources.get(id.0)
    }

    /// Reverse-maps a generated span the way an LSP implementation needs:
    /// mappings whose generated span *contains* `generated` take precedence
    /// over ones that merely overlap it (matching the rust-analyzer spike
    /// client's `mapRange`, `spikes/rust-analyzer/client/source-map.mjs`).
    /// Source spans from every matching mapping are flattened, in mapping
    /// order, and deduplicated by resolved URI (falling back to
    /// [`SourceId`] when it does not resolve) plus span, matching the
    /// spike's `file|start|end` key.
    pub fn map_range(&self, generated: Span) -> Mapped {
        let contained: Vec<&Mapping> = self
            .mappings
            .iter()
            .filter(|m| m.generated.contains(generated))
            .collect();
        let matches: Vec<&Mapping> = if contained.is_empty() {
            self.mappings
                .iter()
                .filter(|m| overlaps_strict(m.generated, generated))
                .collect()
        } else {
            contained
        };

        if matches.is_empty() {
            return Mapped {
                sources: Vec::new(),
                unmapped: true,
            };
        }

        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for mapping in matches {
            for source in &mapping.sources {
                let key = match self.source_uri(source.source) {
                    Some(uri) => SourceKey::Resolved(uri.as_str(), source.span),
                    None => SourceKey::Unresolved(source.source.0, source.span),
                };
                if seen.insert(key) {
                    sources.push(*source);
                }
            }
        }
        Mapped {
            sources,
            unmapped: false,
        }
    }
}

/// Strict overlap, exactly as the spike client's `overlapsRange`: a
/// zero-width mapping never overlaps a non-empty query. Empty *queries* are
/// still matched, through [`Span::contains`].
const fn overlaps_strict(a: Span, b: Span) -> bool {
    a.start < b.end && b.start < a.end
}

/// Dedup key for [`SourceMap::map_range`]: sources are the same if they
/// resolve to the same URI (or, failing that, the same [`SourceId`]) and
/// cover the same span.
#[derive(PartialEq, Eq, Hash)]
enum SourceKey<'a> {
    Resolved(&'a str, Span),
    Unresolved(usize, Span),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rsx() -> Uri {
        Uri::new("file:///app/src/App.rsx")
    }

    /// `<Greeting>Hello</Greeting>` lowers to `Greeting { "Hello" }`: the
    /// generated identifier maps back to *both* tags.
    #[test]
    fn one_generated_span_maps_to_many_source_spans() {
        let map = SourceMap::new(Uri::new("file:///app/src/.generated/App.rs"), vec![rsx()])
            .with_mapping(Mapping::new(
                Span::new(0, 8),
                vec![
                    SourceSpan::new(SourceId(0), Span::new(1, 9)),
                    SourceSpan::new(SourceId(0), Span::new(18, 26)),
                ],
                MappingKind::Identifier,
            ));

        let back = map.to_source(Span::new(2, 3));
        assert_eq!(back.len(), 2);
        assert_eq!(
            map.to_generated(SourceId(0), Span::new(20, 21)),
            vec![Span::new(0, 8)]
        );
    }

    #[test]
    fn one_source_span_maps_to_many_generated_spans() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()])
            .with_mapping(Mapping::new(
                Span::new(0, 4),
                vec![SourceSpan::new(SourceId(0), Span::new(0, 4))],
                MappingKind::Expression,
            ))
            .with_mapping(Mapping::new(
                Span::new(10, 14),
                vec![SourceSpan::new(SourceId(0), Span::new(0, 4))],
                MappingKind::Expression,
            ));

        assert_eq!(map.to_generated(SourceId(0), Span::new(1, 2)).len(), 2);
    }

    #[test]
    fn synthesized_code_has_no_source() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()]).with_mapping(Mapping::new(
            Span::new(0, 4),
            vec![],
            MappingKind::Other,
        ));
        assert!(map.to_source(Span::new(0, 1)).is_empty());
    }

    #[test]
    fn with_label_attaches_an_optional_note() {
        let mapping =
            Mapping::new(Span::new(0, 4), vec![], MappingKind::Other).with_label("rsx! wrapper");
        assert_eq!(mapping.label.as_deref(), Some("rsx! wrapper"));
    }

    /// Matches `overlapsRange` in `spikes/rust-analyzer/client/source-map.mjs`:
    /// a zero-width mapping never overlaps a non-empty query, only a query
    /// that contains (or equals) it.
    #[test]
    fn map_range_ignores_a_zero_width_mapping_that_only_touches_the_query() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()]).with_mapping(Mapping::new(
            Span::new(8, 8),
            vec![SourceSpan::new(SourceId(0), Span::new(0, 5))],
            MappingKind::Identifier,
        ));

        assert!(map.map_range(Span::new(8, 10)).unmapped);
        assert!(map.map_range(Span::new(6, 8)).unmapped);
        assert_eq!(map.map_range(Span::new(8, 8)).sources.len(), 1);
    }

    #[test]
    fn map_range_prefers_a_containing_mapping_over_an_overlapping_one() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()])
            .with_mapping(Mapping::new(
                Span::new(0, 10),
                vec![SourceSpan::new(SourceId(0), Span::new(100, 105))],
                MappingKind::Expression,
            ))
            .with_mapping(Mapping::new(
                Span::new(5, 12),
                vec![SourceSpan::new(SourceId(0), Span::new(200, 205))],
                MappingKind::Expression,
            ));

        assert_eq!(
            map.map_range(Span::new(4, 6)).sources,
            vec![SourceSpan::new(SourceId(0), Span::new(100, 105))]
        );
    }

    #[test]
    fn map_range_returns_every_overlap_in_mapping_order() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()])
            .with_mapping(Mapping::new(
                Span::new(0, 10),
                vec![SourceSpan::new(SourceId(0), Span::new(100, 105))],
                MappingKind::Expression,
            ))
            .with_mapping(Mapping::new(
                Span::new(5, 12),
                vec![SourceSpan::new(SourceId(0), Span::new(200, 205))],
                MappingKind::Expression,
            ));

        assert_eq!(
            map.map_range(Span::new(8, 15)).sources,
            vec![
                SourceSpan::new(SourceId(0), Span::new(100, 105)),
                SourceSpan::new(SourceId(0), Span::new(200, 205)),
            ]
        );
    }

    #[test]
    fn map_range_dedupes_sources_that_resolve_to_the_same_uri() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx(), rsx()]).with_mapping(
            Mapping::new(
                Span::new(0, 10),
                vec![
                    SourceSpan::new(SourceId(0), Span::new(1, 5)),
                    SourceSpan::new(SourceId(1), Span::new(1, 5)),
                ],
                MappingKind::Identifier,
            ),
        );

        let mapped = map.map_range(Span::new(2, 3));
        assert_eq!(mapped.sources.len(), 1);
        assert_eq!(mapped.sources[0].source, SourceId(0));
    }

    #[test]
    fn map_range_falls_back_to_overlap_when_nothing_contains() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()]).with_mapping(Mapping::new(
            Span::new(4, 10),
            vec![SourceSpan::new(SourceId(0), Span::new(0, 5))],
            MappingKind::Expression,
        ));

        // Span(2, 6) overlaps but is not contained by (4, 10).
        let mapped = map.map_range(Span::new(2, 6));
        assert!(!mapped.unmapped);
        assert_eq!(mapped.sources.len(), 1);
    }

    #[test]
    fn map_range_reports_unmapped_when_nothing_intersects() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()]).with_mapping(Mapping::new(
            Span::new(4, 10),
            vec![SourceSpan::new(SourceId(0), Span::new(0, 5))],
            MappingKind::Expression,
        ));

        let mapped = map.map_range(Span::new(20, 25));
        assert!(mapped.unmapped);
        assert!(mapped.sources.is_empty());
    }

    #[test]
    fn map_range_matched_but_synthesized_is_not_unmapped() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()]).with_mapping(Mapping::new(
            Span::new(0, 4),
            vec![],
            MappingKind::Other,
        ));

        let mapped = map.map_range(Span::new(1, 2));
        assert!(!mapped.unmapped);
        assert!(mapped.sources.is_empty());
    }
}
