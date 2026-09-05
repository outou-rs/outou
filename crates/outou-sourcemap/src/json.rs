//! JSON representation of a [`SourceMap`], compatible with the rust-analyzer
//! spike's hand-written format (`spikes/rust-analyzer/source-map.json`) and
//! the spike client that reads it (`spikes/rust-analyzer/client/source-map.mjs`).
//!
//! [`SourceMap`] itself stores UTF-8 byte [`Span`]s, which are convenient
//! for the compiler but not what the JSON format (or LSP) uses on the wire:
//! 0-based `{line, character}` positions with `character` in UTF-16 code
//! units. [`SourceMap::to_json`] and [`SourceMapJson::into_source_map`]
//! convert between the two through a [`LineIndex`] built from the relevant
//! file's text.

use serde::{Deserialize, Serialize};

use crate::{
    LineIndex, Mapping, MappingKind, Position, PositionRange, SourceId, SourceMap, SourceSpan, Uri,
};

/// The only source map JSON format version this crate writes or accepts.
pub const SOURCE_MAP_JSON_VERSION: u32 = 1;

/// JSON-serializable form of a [`SourceMap`].
///
/// Field shape matches `spikes/rust-analyzer/source-map.json`:
///
/// ```json
/// {
///   "version": 1,
///   "generated": "fixture/src/.generated/App.rs",
///   "sources": ["fixture/src/App.rsx"],
///   "mappings": [
///     {
///       "kind": "identifier",
///       "what": "user declaration",
///       "generated": { "start": {"line": 8, "character": 8}, "end": {"line": 8, "character": 12} },
///       "sources": [{"file": 0, "start": {"line": 4, "character": 8}, "end": {"line": 4, "character": 12}}]
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMapJson {
    /// Format version. Always [`SOURCE_MAP_JSON_VERSION`].
    pub version: u32,
    /// Generated file path or URI, as written by the compiler.
    pub generated: String,
    /// Source file paths or URIs, indexed by [`SourceId`].
    pub sources: Vec<String>,
    /// Free-form note for humans reading the file. Ignored by tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Mapping entries, in generated-position order.
    pub mappings: Vec<MappingJson>,
}

/// JSON-serializable form of a [`Mapping`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingJson {
    /// Classification of the mapped syntax.
    pub kind: MappingKind,
    /// Optional human-readable label (the [`Mapping::label`] field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what: Option<String>,
    /// Range inside the generated file.
    pub generated: PositionRange,
    /// Zero or more originating spans.
    pub sources: Vec<SourceSpanJson>,
}

/// JSON-serializable form of a [`SourceSpan`]: a source file index alongside
/// a flat `start`/`end` range (matching the spike format, which does not
/// nest the range under its own key here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpanJson {
    /// Index into the enclosing [`SourceMapJson::sources`].
    pub file: usize,
    /// Inclusive start position.
    pub start: Position,
    /// Exclusive end position.
    pub end: Position,
}

/// Errors converting a [`SourceMapJson`] into a [`SourceMap`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FromJsonError {
    /// The `version` field is not one this crate understands.
    #[error("unsupported source map version {found} (expected {SOURCE_MAP_JSON_VERSION})")]
    UnsupportedVersion {
        /// The version found in the JSON.
        found: u32,
    },
    /// A mapping's source referenced a file index with no matching text.
    #[error(
        "mapping references source file index {index}, but only {available} source text(s) were given"
    )]
    SourceIndexOutOfRange {
        /// The out-of-range index.
        index: usize,
        /// How many source texts were actually provided.
        available: usize,
    },
    /// A mapping's `generated` or `sources[]` range has `end` before `start`.
    #[error("mapping range is inverted: start {start:?} is after end {end:?}")]
    InvertedRange {
        /// The range's start position.
        start: Position,
        /// The range's end position.
        end: Position,
    },
    /// A mapping's source referenced a file index not present in the
    /// declared `sources` list.
    #[error(
        "mapping references source file index {index}, but only {declared} source(s) are declared"
    )]
    SourceIndexNotDeclared {
        /// The undeclared index.
        index: usize,
        /// How many sources are declared.
        declared: usize,
    },
}

/// Errors [`SourceMap::to_json`] rejects a conversion for.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToJsonError {
    /// A mapping's source referenced a [`SourceId`] beyond the map's
    /// declared `sources`.
    #[error(
        "mapping references source id {index}, but the map declares only {declared} source(s)"
    )]
    UnknownSourceId {
        /// The unknown source index.
        index: usize,
        /// How many sources the map declares.
        declared: usize,
    },
    /// A mapping's source referenced a [`SourceId`] with no matching entry
    /// in the `source_texts` given to [`SourceMap::to_json`].
    #[error(
        "mapping references source id {index}, but only {available} source text(s) were given"
    )]
    MissingSourceText {
        /// The source index with no matching text.
        index: usize,
        /// How many source texts were given.
        available: usize,
    },
}

impl SourceMap {
    /// Converts this map into its JSON representation.
    ///
    /// `generated_text` is the full text of the generated Rust file, and
    /// `source_texts` holds the full text of each source file, in the same
    /// order as [`SourceMap::sources`]; both are needed to convert byte
    /// [`crate::Span`]s into `{line, character}` positions.
    pub fn to_json(
        &self,
        generated_text: &str,
        source_texts: &[&str],
    ) -> Result<SourceMapJson, ToJsonError> {
        let generated_index = LineIndex::new(generated_text);
        let source_indexes: Vec<LineIndex> = source_texts
            .iter()
            .map(|text| LineIndex::new(text))
            .collect();

        let mut mappings = Vec::with_capacity(self.mappings.len());
        for mapping in &self.mappings {
            let mut sources = Vec::with_capacity(mapping.sources.len());
            for source in &mapping.sources {
                if source.source.0 >= self.sources.len() {
                    return Err(ToJsonError::UnknownSourceId {
                        index: source.source.0,
                        declared: self.sources.len(),
                    });
                }
                let index =
                    source_indexes
                        .get(source.source.0)
                        .ok_or(ToJsonError::MissingSourceText {
                            index: source.source.0,
                            available: source_indexes.len(),
                        })?;
                let range = index.span_to_range(source.span);
                sources.push(SourceSpanJson {
                    file: source.source.0,
                    start: range.start,
                    end: range.end,
                });
            }
            mappings.push(MappingJson {
                kind: mapping.kind,
                what: mapping.label.clone(),
                generated: generated_index.span_to_range(mapping.generated),
                sources,
            });
        }

        Ok(SourceMapJson {
            version: SOURCE_MAP_JSON_VERSION,
            generated: self.generated.as_str().to_owned(),
            sources: self
                .sources
                .iter()
                .map(|uri| uri.as_str().to_owned())
                .collect(),
            comment: None,
            mappings,
        })
    }
}

impl SourceMapJson {
    /// Converts this JSON representation into a [`SourceMap`], resolving
    /// `{line, character}` positions to byte offsets through a
    /// [`LineIndex`] built from `generated_text` and `source_texts`.
    ///
    /// `source_texts` must hold one entry per file referenced by a
    /// mapping's `sources[].file` index (normally one per
    /// [`SourceMapJson::sources`] entry, in the same order).
    pub fn into_source_map(
        self,
        generated_text: &str,
        source_texts: &[&str],
    ) -> Result<SourceMap, FromJsonError> {
        if self.version != SOURCE_MAP_JSON_VERSION {
            return Err(FromJsonError::UnsupportedVersion {
                found: self.version,
            });
        }

        let generated_index = LineIndex::new(generated_text);
        let source_indexes: Vec<LineIndex> = source_texts
            .iter()
            .map(|text| LineIndex::new(text))
            .collect();

        let mut mappings = Vec::with_capacity(self.mappings.len());
        for mapping in self.mappings {
            if mapping.generated.start > mapping.generated.end {
                return Err(FromJsonError::InvertedRange {
                    start: mapping.generated.start,
                    end: mapping.generated.end,
                });
            }
            let mut sources = Vec::with_capacity(mapping.sources.len());
            for source in mapping.sources {
                if source.start > source.end {
                    return Err(FromJsonError::InvertedRange {
                        start: source.start,
                        end: source.end,
                    });
                }
                if source.file >= self.sources.len() {
                    return Err(FromJsonError::SourceIndexNotDeclared {
                        index: source.file,
                        declared: self.sources.len(),
                    });
                }
                let line_index = source_indexes.get(source.file).ok_or(
                    FromJsonError::SourceIndexOutOfRange {
                        index: source.file,
                        available: source_indexes.len(),
                    },
                )?;
                let span = line_index.range_to_span(PositionRange::new(source.start, source.end));
                sources.push(SourceSpan::new(SourceId(source.file), span));
            }
            mappings.push(Mapping {
                generated: generated_index.range_to_span(mapping.generated),
                sources,
                kind: mapping.kind,
                label: mapping.what,
            });
        }

        Ok(SourceMap {
            generated: Uri::new(self.generated),
            sources: self.sources.into_iter().map(Uri::new).collect(),
            mappings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;
    use std::path::Path;

    /// The spike JSON must parse into the same shape the spike client reads:
    /// version 1, one source, and mappings whose `kind`/`what` and (for the
    /// synthesized `rsx!` wrapper) empty `sources` round-trip untouched.
    #[test]
    fn spike_json_shape_parses() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = Path::new(manifest_dir).join("../../spikes/rust-analyzer/source-map.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let parsed: SourceMapJson =
            serde_json::from_str(&text).expect("spike source-map.json must parse");

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.generated, "fixture/src/.generated/App.rs");
        assert_eq!(parsed.sources, vec!["fixture/src/App.rsx".to_string()]);
        assert_eq!(parsed.mappings.len(), 6);

        let synthesized = parsed
            .mappings
            .iter()
            .find(|m| m.kind == MappingKind::Other)
            .expect("the rsx! wrapper mapping");
        assert!(synthesized.sources.is_empty());
        assert_eq!(
            synthesized.what.as_deref(),
            Some("rsx! wrapper: synthesized, no source span")
        );

        let identifier = parsed
            .mappings
            .iter()
            .find(|m| m.kind == MappingKind::Attribute)
            .expect("the attribute mapping");
        assert_eq!(identifier.sources.len(), 1);
        assert_eq!(identifier.sources[0].file, 0);
    }

    /// A `SourceMap` built by hand converts to JSON and back to an equal
    /// `SourceMap`, using synthetic (but structurally representative) source
    /// texts, exercising every field including the optional label and the
    /// synthesized (zero-source) case.
    #[test]
    fn round_trips_through_json() {
        let generated_text = "pub fn App() -> Element {\n    let user = load_user();\n    UserCard { user: user }\n}\n";
        let source_text =
            "fn App() -> Element {\n    let user = load_user();\n    <UserCard user={user} />\n}\n";

        let map = SourceMap::new(
            Uri::new("file:///app/src/.generated/App.rs"),
            vec![Uri::new("file:///app/src/App.rsx")],
        )
        .with_mapping(
            Mapping::new(
                Span::new(34, 38),
                vec![SourceSpan::new(SourceId(0), Span::new(30, 34))],
                MappingKind::Identifier,
            )
            .with_label("user declaration"),
        )
        .with_mapping(Mapping::new(Span::new(0, 3), vec![], MappingKind::Other));

        let json = map
            .to_json(generated_text, &[source_text])
            .expect("every source has text");
        assert_eq!(json.version, SOURCE_MAP_JSON_VERSION);
        assert_eq!(json.generated, "file:///app/src/.generated/App.rs");
        assert_eq!(json.sources, vec!["file:///app/src/App.rsx".to_string()]);
        assert_eq!(json.mappings.len(), 2);
        assert_eq!(json.mappings[0].what.as_deref(), Some("user declaration"));
        assert!(json.mappings[1].sources.is_empty());

        let round_tripped = json
            .clone()
            .into_source_map(generated_text, &[source_text])
            .expect("valid json converts back");
        assert_eq!(round_tripped, map);

        // The JSON itself also round-trips through serde_json unchanged.
        let text = serde_json::to_string(&json).unwrap();
        let reparsed: SourceMapJson = serde_json::from_str(&text).unwrap();
        assert_eq!(reparsed, json);
    }

    #[test]
    fn rejects_unsupported_version() {
        let json = SourceMapJson {
            version: 2,
            generated: "g.rs".into(),
            sources: vec![],
            comment: None,
            mappings: vec![],
        };
        let err = json.into_source_map("", &[]).unwrap_err();
        assert_eq!(err, FromJsonError::UnsupportedVersion { found: 2 });
    }

    #[test]
    fn rejects_out_of_range_source_index() {
        let json = SourceMapJson {
            version: 1,
            generated: "g.rs".into(),
            sources: vec![
                "s0.rsx".into(),
                "s1.rsx".into(),
                "s2.rsx".into(),
                "s3.rsx".into(),
            ],
            comment: None,
            mappings: vec![MappingJson {
                kind: MappingKind::Identifier,
                what: None,
                generated: PositionRange::new(Position::new(0, 0), Position::new(0, 1)),
                sources: vec![SourceSpanJson {
                    file: 3,
                    start: Position::new(0, 0),
                    end: Position::new(0, 1),
                }],
            }],
        };
        let err = json.into_source_map("x", &["s"]).unwrap_err();
        assert_eq!(
            err,
            FromJsonError::SourceIndexOutOfRange {
                index: 3,
                available: 1
            }
        );
    }

    #[test]
    fn rejects_an_inverted_generated_range() {
        let json = SourceMapJson {
            version: 1,
            generated: "g.rs".into(),
            sources: vec![],
            comment: None,
            mappings: vec![MappingJson {
                kind: MappingKind::Identifier,
                what: None,
                generated: PositionRange::new(Position::new(0, 2), Position::new(0, 1)),
                sources: vec![],
            }],
        };
        let err = json.into_source_map("abcdef", &[]).unwrap_err();
        assert_eq!(
            err,
            FromJsonError::InvertedRange {
                start: Position::new(0, 2),
                end: Position::new(0, 1),
            }
        );
    }

    #[test]
    fn rejects_a_source_index_absent_from_the_declared_sources() {
        let json = SourceMapJson {
            version: 1,
            generated: "g.rs".into(),
            sources: vec![],
            comment: None,
            mappings: vec![MappingJson {
                kind: MappingKind::Identifier,
                what: None,
                generated: PositionRange::new(Position::new(0, 0), Position::new(0, 1)),
                sources: vec![SourceSpanJson {
                    file: 0,
                    start: Position::new(0, 0),
                    end: Position::new(0, 1),
                }],
            }],
        };
        let err = json.into_source_map("abcdef", &["hello"]).unwrap_err();
        assert_eq!(
            err,
            FromJsonError::SourceIndexNotDeclared {
                index: 0,
                declared: 0
            }
        );
    }

    #[test]
    fn to_json_rejects_a_source_without_text() {
        let map = SourceMap::new(Uri::new("file:///g.rs"), vec![Uri::new("file:///a.rsx")])
            .with_mapping(Mapping::new(
                Span::new(0, 4),
                vec![SourceSpan::new(SourceId(0), Span::new(0, 4))],
                MappingKind::Expression,
            ));

        let err = map.to_json("abcdef", &[]).unwrap_err();
        assert_eq!(
            err,
            ToJsonError::MissingSourceText {
                index: 0,
                available: 0
            }
        );
    }
}
