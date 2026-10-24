//! Mapping a byte span of generated Rust back to the `.rsx` file.
//!
//! This is the logic that used to live only in the UI test harness
//! (`crates/outou-cli/tests/ui.rs`); `outou cargo` and the harness now
//! share it ("there is one compiler", `AGENTS.md`). It deliberately does
//! not replace `outou-lsp`'s `mapping.rs`, which answers a different
//! question (a cursor position) and keeps its own wrappers around the same
//! [`SourceMap::narrow`] primitive.

use outou_sourcemap::{NarrowOutcome, SourceMap, SourceSpan, Span};

/// A diagnostic span that covers at least this many lines is mapped by its
/// start only. rustc reports the whole `rsx!` call (every line of a
/// component body) for some errors; that range does not exist in the
/// `.rsx` file, and mapping both ends would produce a range that ends at
/// an unrelated place. Measured in `docs/alpha/issues/25`'s prototype: the
/// spans of this kind are 15 lines and longer, ordinary multi-line spans
/// (a borrow error over 4 lines) are shorter.
pub const COARSE_SPAN_LINES: usize = 15;

/// Where a diagnostic span lands in the `.rsx` sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MappedSpan {
    /// The `.rsx` file and byte range.
    pub source: SourceSpan,
    /// `true` when only the start of the generated span was mapped
    /// ([`COARSE_SPAN_LINES`]); the range is then one position.
    pub coarse: bool,
}

/// Maps a diagnostic's primary span (`generated`, `lines` lines long) to
/// the `.rsx` source, in up to four steps:
///
/// 0. A span of [`COARSE_SPAN_LINES`] lines or more is first narrowed as
///    a whole: when the whole span narrows [exactly](NarrowOutcome::Exact)
///    (a long closure body inside one spliced Rust run), that range is
///    used as it is, so the carets keep their width. Otherwise it is
///    reduced to its first byte before the steps below, and the result is
///    `coarse`.
/// 1. [`SourceMap::narrow`]: the containing mapping has exactly one
///    source of the same length, so the query is scaled exactly. Without
///    this step every diagnostic inside a spliced Rust run (one mapping
///    covers all the plain statements between two JSX elements) would
///    land on the run's first byte. A [`NarrowOutcome::LengthMismatch`]
///    (a transformed mapping, such as an escaped string literal) is
///    treated like "not applicable" here and continues with step 2, as
///    the UI harness always did.
/// 2. [`SourceMap::map_range`]'s coarser whole-span answer.
/// 3. The nearest mapped position, when the span is synthesized code
///    (the scaffolding of the macro expansion, an `{}` child): never drop
///    a hard error because its span has no direct mapping.
///
/// `None` only when the map has no mapping with a source at all.
pub fn map_diagnostic_span(map: &SourceMap, generated: Span, lines: usize) -> Option<MappedSpan> {
    let coarse = lines >= COARSE_SPAN_LINES;
    if coarse {
        if let NarrowOutcome::Exact { source, span } = map.narrow(generated) {
            return Some(MappedSpan {
                source: SourceSpan::new(source, span),
                coarse: false,
            });
        }
    }
    let query = if coarse {
        Span::new(generated.start, generated.start)
    } else {
        generated
    };
    map_generated_span(map, query).map(|source| MappedSpan { source, coarse })
}

/// Steps 1 to 3 of [`map_diagnostic_span`].
pub fn map_generated_span(map: &SourceMap, generated: Span) -> Option<SourceSpan> {
    if let NarrowOutcome::Exact { source, span } = map.narrow(generated) {
        return Some(SourceSpan::new(source, span));
    }
    if let Some(first) = map.map_range(generated).sources.first() {
        return Some(*first);
    }
    map.mappings
        .iter()
        .filter(|mapping| !mapping.sources.is_empty())
        .min_by_key(|mapping| generated_distance(mapping.generated, generated))
        .map(|mapping| mapping.sources[0])
}

/// Maps `generated` only when it narrows exactly and the generated text is
/// byte for byte the `.rsx` text it maps to. Used for secondary spans and
/// suggested replacements, where a coarse answer would be wrong rather
/// than merely imprecise.
pub fn map_verbatim(
    map: &SourceMap,
    generated: Span,
    generated_text: &str,
    source_text: impl Fn(SourceSpan) -> Option<String>,
) -> Option<SourceSpan> {
    let NarrowOutcome::Exact { source, span } = map.narrow(generated) else {
        return None;
    };
    let mapped = SourceSpan::new(source, span);
    let original = source_text(mapped)?;
    let generated_slice = generated_text.get(generated.start as usize..generated.end as usize)?;
    (original == generated_slice).then_some(mapped)
}

/// Byte distance between two spans: `0` when they overlap or touch,
/// otherwise the gap between the closer pair of endpoints.
fn generated_distance(a: Span, b: Span) -> u32 {
    if a.end <= b.start {
        b.start.saturating_sub(a.end)
    } else if b.end <= a.start {
        a.start.saturating_sub(b.end)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outou_sourcemap::{Mapping, MappingKind, SourceId, Uri};

    fn map(mappings: Vec<Mapping>) -> SourceMap {
        mappings.into_iter().fold(
            SourceMap::new(Uri::new("file:///g.rs"), vec![Uri::new("file:///s.rsx")]),
            SourceMap::with_mapping,
        )
    }

    fn verbatim(generated: (u32, u32), source: (u32, u32)) -> Mapping {
        Mapping::new(
            Span::new(generated.0, generated.1),
            vec![SourceSpan::new(SourceId(0), Span::new(source.0, source.1))],
            MappingKind::Expression,
        )
    }

    #[test]
    fn a_span_inside_a_verbatim_run_narrows_exactly() {
        let map = map(vec![verbatim((100, 200), (10, 110))]);
        let mapped = map_diagnostic_span(&map, Span::new(130, 140), 1).unwrap();
        assert_eq!(mapped.source.span, Span::new(40, 50));
        assert!(!mapped.coarse);
    }

    #[test]
    fn a_long_span_that_does_not_narrow_is_mapped_by_its_start_only() {
        // Two mappings: the span straddles both, so it does not narrow.
        let map = map(vec![
            verbatim((100, 200), (10, 110)),
            verbatim((200, 400), (500, 700)),
        ]);
        let mapped = map_diagnostic_span(&map, Span::new(120, 390), COARSE_SPAN_LINES).unwrap();
        assert_eq!(mapped.source.span, Span::new(30, 30));
        assert!(mapped.coarse);
        let short = map_diagnostic_span(&map, Span::new(120, 390), COARSE_SPAN_LINES - 1).unwrap();
        assert!(!short.coarse);
    }

    #[test]
    fn a_long_span_that_narrows_exactly_keeps_its_whole_range() {
        let map = map(vec![verbatim((100, 400), (10, 310))]);
        let mapped = map_diagnostic_span(&map, Span::new(120, 390), COARSE_SPAN_LINES + 3).unwrap();
        assert_eq!(mapped.source.span, Span::new(30, 300));
        assert!(!mapped.coarse);
    }

    #[test]
    fn a_synthesized_span_falls_back_to_the_nearest_mapping() {
        let map = map(vec![
            verbatim((0, 10), (0, 10)),
            Mapping::new(Span::new(10, 50), vec![], MappingKind::Other),
            verbatim((60, 70), (20, 30)),
        ]);
        let mapped = map_diagnostic_span(&map, Span::new(55, 56), 1).unwrap();
        assert_eq!(mapped.source.span, Span::new(20, 30));
    }

    #[test]
    fn a_map_with_no_sourced_mapping_maps_nothing() {
        let map = map(vec![Mapping::new(
            Span::new(0, 10),
            vec![],
            MappingKind::Other,
        )]);
        assert_eq!(map_diagnostic_span(&map, Span::new(2, 3), 1), None);
    }

    #[test]
    fn verbatim_mapping_requires_identical_text() {
        let map = map(vec![verbatim((0, 5), (0, 5))]);
        let same = map_verbatim(&map, Span::new(1, 3), "abcde", |_| Some("bc".into()));
        assert!(same.is_some());
        let changed = map_verbatim(&map, Span::new(1, 3), "abcde", |_| Some("xx".into()));
        assert!(changed.is_none());
    }

    #[test]
    fn a_length_mismatch_is_never_verbatim() {
        let map = map(vec![verbatim((0, 6), (0, 4))]);
        assert!(map_verbatim(&map, Span::new(1, 3), "abcdef", |_| Some("bc".into())).is_none());
    }
}
