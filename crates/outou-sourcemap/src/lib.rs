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

mod registry;
mod span;

pub use registry::Registry;
pub use span::{SourceId, SourceSpan, Span, Uri};

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
            .with_mapping(Mapping {
                generated: Span::new(0, 8),
                sources: vec![
                    SourceSpan::new(SourceId(0), Span::new(1, 9)),
                    SourceSpan::new(SourceId(0), Span::new(18, 26)),
                ],
                kind: MappingKind::Identifier,
            });

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
            .with_mapping(Mapping {
                generated: Span::new(0, 4),
                sources: vec![SourceSpan::new(SourceId(0), Span::new(0, 4))],
                kind: MappingKind::Expression,
            })
            .with_mapping(Mapping {
                generated: Span::new(10, 14),
                sources: vec![SourceSpan::new(SourceId(0), Span::new(0, 4))],
                kind: MappingKind::Expression,
            });

        assert_eq!(map.to_generated(SourceId(0), Span::new(1, 2)).len(), 2);
    }

    #[test]
    fn synthesized_code_has_no_source() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![rsx()]).with_mapping(Mapping {
            generated: Span::new(0, 4),
            sources: vec![],
            kind: MappingKind::Other,
        });
        assert!(map.to_source(Span::new(0, 1)).is_empty());
    }
}
