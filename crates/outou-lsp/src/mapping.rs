//! Position and location translation between `.rsx` files and generated
//! Rust, built on [`outou_sourcemap::LineIndex`] and [`outou_sourcemap::Registry`].
//!
//! Every rust-analyzer request this server forwards goes through
//! [`rsx_position_to_generated`] on the way out and every generated
//! location coming back goes through [`generated_location_to_source`] —
//! the same two functions serve hover, completion, definition and
//! diagnostics, so a single bug fix here fixes all four.

use outou_sourcemap::{MappingKind, Position as OutouPosition, PositionRange, SourceId, Span};

use crate::documents::Workspace;
use crate::uri;

/// Converts an LSP position to this crate's own [`OutouPosition`] (same
/// shape, different crate).
fn to_outou_position(position: lsp_types::Position) -> OutouPosition {
    OutouPosition::new(position.line, position.character)
}

/// Converts this crate's own [`OutouPosition`] to an LSP position.
fn to_lsp_position(position: OutouPosition) -> lsp_types::Position {
    lsp_types::Position::new(position.line, position.character)
}

/// Converts a [`PositionRange`] to an LSP [`lsp_types::Range`].
pub fn to_lsp_range(range: PositionRange) -> lsp_types::Range {
    lsp_types::Range::new(to_lsp_position(range.start), to_lsp_position(range.end))
}

/// Converts an LSP [`lsp_types::Range`] to a [`PositionRange`].
fn to_outou_range(range: lsp_types::Range) -> PositionRange {
    PositionRange::new(to_outou_position(range.start), to_outou_position(range.end))
}

/// A position resolved into a specific generated file.
pub struct GeneratedPosition {
    /// URI of the generated file, as known to rust-analyzer.
    pub generated_uri: lsp_types::Uri,
    /// The mapped position inside it.
    pub position: lsp_types::Position,
}

/// Maps a `.rsx` position to the corresponding position in its generated
/// unit, per the architecture note: "map the `.rsx` position -> byte
/// offset -> `SourceMap::to_generated` (pick the first generated span; if
/// none, the position is in synthesized code or unmapped -> return
/// null/empty) -> generated position".
///
/// Unlike a plain `SourceMap::to_generated` lookup, this does not just
/// jump to the *start* of the matched generated span: a mapping's source
/// span is frequently much larger than a single token (`Writer::verbatim`
/// maps a whole spliced Rust block — e.g. every plain-Rust statement
/// between JSX in a component body — as one mapping), so landing on the
/// span's start for every position inside it would put a hover deep
/// inside a `let` block back at the top of the function. Instead the
/// offset is translated proportionally within the matched source/generated
/// span pair, which is exact whenever the two are the same length (true
/// for `Writer::verbatim`, the common case for plain Rust) and a
/// reasonable approximation otherwise (e.g. an escaped string literal).
///
/// Returns `None` when `rsx_uri` is not a known unit, or when the position
/// maps to nothing generated (synthesized code, or a position genuinely
/// outside any mapping) — callers should treat that the same way they
/// would treat an empty LSP response.
pub fn rsx_position_to_generated(
    workspace: &Workspace,
    rsx_uri: &lsp_types::Uri,
    position: lsp_types::Position,
) -> Option<GeneratedPosition> {
    let rsx_uri_string = uri::to_outou(rsx_uri).as_str().to_string();
    let doc = workspace.rsx.get(&rsx_uri_string)?;
    let offset = doc
        .line_index
        .position_to_offset(to_outou_position(position));

    let generated_uri_string = workspace.rsx_to_generated.get(&rsx_uri_string)?;
    let unit = workspace.generated.get(generated_uri_string)?;
    let map = workspace.registry.map_for_generated(&unit.generated_uri)?;

    // Every generated unit's writer registers exactly one source (its own
    // `.rsx` file, `outou_codegen::Writer::new`), so it is always
    // `SourceId(0)`. Several mappings can contain the same source offset
    // at once — a JSX element nested inside a Rust `if` expression has a
    // mapping for the whole verbatim-copied `if`/braces scaffolding *and*
    // one for the element's own name, both of whose source spans contain
    // any offset inside the element — so this picks the *smallest*
    // containing source span (the most specific one) rather than
    // whichever happens to come first in generated-span order.
    let query = Span::new(offset, offset);
    let (source_span, mapping) = map
        .mappings
        .iter()
        .filter_map(|mapping| {
            mapping
                .sources
                .iter()
                .find(|source| source.source == SourceId(0) && source.span.contains(query))
                .map(|source| (source.span, mapping))
        })
        .min_by_key(|(source_span, _)| source_span.end - source_span.start)?;
    let generated_span = mapping.generated;

    // S1 (issue #9 Gate 3 review): a transformed mapping — source and
    // generated spans of different lengths, e.g. `type` lowering to
    // `r#type` (ledger row 29) — cannot be
    // translated proportionally without risking a confidently *wrong*
    // answer rather than an honest empty one. Confirmed live: hovering
    // `tag` inside that attribute value returned `extern crate std` at a
    // scaled-but-meaningless generated offset. Only an exact
    // (same-length, almost always `Writer::verbatim`) mapping is
    // translated; everything else maps to nothing rather than to a
    // plausible-looking wrong position.
    //
    // The one exception (ADR 0013, amendment of #31, ledger row 61) is
    // the exact `r#` + name shape of a keyword-named attribute
    // ([`raw_prefixed_name`]), translated with a fixed offset of
    // [`RAW_PREFIX_LEN`], never by proportion.
    let generated_offset = if span_len(source_span) == span_len(generated_span) {
        outou_sourcemap::scale_offset(offset, source_span, generated_span)
    } else if raw_prefixed_name(workspace, map, mapping, &unit.text).is_some() {
        generated_span.start + RAW_PREFIX_LEN + (offset - source_span.start)
    } else {
        return None;
    };
    let generated_position = unit.line_index.offset_to_position(generated_offset);

    Some(GeneratedPosition {
        generated_uri: uri::to_lsp(&unit.generated_uri),
        position: to_lsp_position(generated_position),
    })
}

/// The length of `span`, in bytes. Used only for this file's own
/// same-length check above; the proportional scaling itself is
/// `outou_sourcemap::scale_offset`, shared with
/// `outou_sourcemap::SourceMap::narrow` (issue #12 corpus review, F4) —
/// see [`narrow_single_source`] below.
fn span_len(span: Span) -> u32 {
    span.end - span.start
}

/// The raw-identifier prefix Rust needs on a keyword (`r#type`).
const RAW_PREFIX: &str = "r#";
/// [`RAW_PREFIX`]'s length in bytes.
const RAW_PREFIX_LEN: u32 = 2;

/// Whether `mapping` is the one transformed shape this server translates
/// exactly: a JSX attribute name (`MappingKind::Attribute`) with exactly
/// one source whose generated text is `r#` followed by byte-for-byte the
/// source text (`type` lowered to `r#type`, ledger row 17). Returns that
/// source.
///
/// This is the narrow exception to ADR 0013's refusal of length-mismatched
/// mappings (ledger row 61): the generated span is the source span shifted
/// by a fixed [`RAW_PREFIX_LEN`] bytes, so every offset translates exactly,
/// with no proportional guessing. Anything else — another kind, several
/// sources, a different prefix, a different name (a hyphenated
/// `data-id` lowered to `"data-id"`, a string key) — is still refused.
/// Both texts are compared, so a stale map that no longer matches the text
/// fails closed.
fn raw_prefixed_name(
    workspace: &Workspace,
    map: &outou_sourcemap::SourceMap,
    mapping: &outou_sourcemap::Mapping,
    generated_text: &str,
) -> Option<outou_sourcemap::SourceSpan> {
    if mapping.kind != outou_sourcemap::MappingKind::Attribute {
        return None;
    }
    let [source] = mapping.sources.as_slice() else {
        return None;
    };
    if span_len(mapping.generated) != span_len(source.span) + RAW_PREFIX_LEN {
        return None;
    }
    let source_uri = map.source_uri(source.source)?;
    let source_text = workspace.rsx.get(source_uri.as_str())?.line_index.text();
    let source_name = source_text.get(source.span.start as usize..source.span.end as usize)?;
    let generated_name = generated_text
        .get(mapping.generated.start as usize..mapping.generated.end as usize)?
        .strip_prefix(RAW_PREFIX)?;
    let is_identifier = !source_name.is_empty()
        && source_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    (is_identifier && generated_name == source_name).then_some(*source)
}

/// Reverse direction of [`raw_prefixed_name`]: narrows a generated `query`
/// that lies inside such a mapping to the exact `.rsx` span. The whole
/// generated `r#type` maps to the whole source `type`; a query inside the
/// name part (rust-analyzer may report an occurrence without the prefix; only mocks exercised this, real rust-analyzer always returned the whole `r#type`)
/// maps with the same fixed offset. A query that touches the `r#` prefix
/// but is not the whole token has no source, so it is `None`. That is
/// deliberate: rust-analyzer's completion ranges such as `r#ty` are not
/// translated, completion inside a keyword name is answered locally
/// (`crate::complete`).
fn narrow_raw_prefixed(
    workspace: &Workspace,
    map: &outou_sourcemap::SourceMap,
    generated_text: &str,
    query: Span,
) -> Option<outou_sourcemap::SourceSpan> {
    let mapping = map
        .mappings
        .iter()
        .filter(|m| m.generated.contains(query))
        .min_by_key(|m| span_len(m.generated))?;
    let source = raw_prefixed_name(workspace, map, mapping, generated_text)?;
    let name_start = mapping.generated.start + RAW_PREFIX_LEN;
    let span = if query == mapping.generated {
        source.span
    } else if query.start >= name_start {
        Span::new(
            source.span.start + (query.start - name_start),
            source.span.start + (query.end - name_start),
        )
    } else {
        return None;
    };
    Some(outou_sourcemap::SourceSpan::new(source.source, span))
}

/// A location mapped back from generated Rust to its `.rsx` source, or
/// left untouched because the registry does not own that file (ADR 0007's
/// reverse-mapping rule).
pub enum MappedLocation {
    /// The location was generated by this server; here is its `.rsx`
    /// equivalent.
    Source {
        /// The `.rsx` file's URI.
        uri: lsp_types::Uri,
        /// The mapped range within it.
        range: lsp_types::Range,
    },
    /// The location is not a file this registry produced (a plain `.rs`
    /// module or a dependency crate): return it exactly as rust-analyzer
    /// gave it.
    Unchanged,
    /// The location is inside a known generated file, but the specific
    /// span has no source (synthesized code, e.g. a recovery placeholder
    /// or the `rsx!` wrapper itself).
    Unmapped,
}

/// Reverse-maps one `(uri, range)` pair rust-analyzer returned, per ADR
/// 0007: "if a `.rsx` file produced the location, map back to it.
/// Otherwise return the Rust location untouched."
///
/// Prefers [`narrow_single_source`]'s proportional sub-range over the
/// registry's own [`outou_sourcemap::SourceMap::map_range`], for the same
/// reason [`rsx_position_to_generated`] does not just jump to a mapping's
/// start: `map_range` is designed for the many-to-many case (a diagnostic
/// or "find references" query, where the full source span of a matching
/// mapping is exactly what is wanted) and returns that whole span even
/// when the *generated* query was a single short identifier inside a much
/// larger verbatim-copied mapping — which would make a hover range span
/// several lines of the `.rsx` file for a one-word query. Falling back to
/// `map_range` when a mapping has more than one source (a JSX element
/// whose name came from both its opening and closing tag, ADR 0007's own
/// example) keeps that case's existing, deliberately coarse behavior.
pub fn generated_location_to_source(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    range: lsp_types::Range,
) -> MappedLocation {
    let generated = uri::to_outou(generated_uri);
    let Some(map) = workspace.registry.map_for_generated(&generated) else {
        return MappedLocation::Unchanged;
    };
    let Some(unit) = workspace.generated.get(generated.as_str()) else {
        return MappedLocation::Unchanged;
    };
    let query = unit.line_index.range_to_span(to_outou_range(range));

    match narrow_single_source(workspace, map, query) {
        NarrowOutcome::Exact { uri, range } => return MappedLocation::Source { uri, range },
        // S1 (issue #9 Gate 3 review): the one mapping this query falls
        // inside has an unambiguous single source, but the two spans
        // differ in length — a transformed mapping, not a verbatim copy —
        // so a proportional guess is confidently wrong more often than it
        // is right (`type` -> `r#type`; `key={tag.clone()}` was the original
        // HIGH-7 example, gone since issue #26 copies key expressions verbatim).
        // Reported as unmapped outright, never falling through to the
        // coarser `reverse()` full-span answer below.
        //
        // Except the exact `r#` + name shape of a keyword-named attribute
        // (ledger row 61), which translates by a fixed prefix offset.
        NarrowOutcome::LengthMismatch => {
            return match raw_prefixed_location(workspace, map, &unit.text, query) {
                Some((uri, range)) => MappedLocation::Source { uri, range },
                None => MappedLocation::Unmapped,
            }
        }
        NarrowOutcome::NotApplicable => {}
    }

    let Some(mut resolved) = workspace.registry.reverse(&generated, query) else {
        return MappedLocation::Unchanged;
    };
    if resolved.is_empty() {
        return MappedLocation::Unmapped;
    }
    // Accepted behavior (ADR 0007, issue #24): a
    // mapping with several sources (e.g. an element name coming from both
    // its opening and closing tag, ADR 0007's own example) always uses
    // only the *first* one here; a diagnostic whose generated span
    // legitimately belongs to more than one `.rsx` file — or more than
    // one position in the same file — is only ever shown at the first.
    // ADR 0007's reverse-mapping rule ("if a `.rsx` file produced the
    // location, map back to it") asks for one location, not a union of
    // all candidates, so showing the diagnostic at the first source is the
    // intended behavior (accepted in issue #24, not a pending item).
    let (source_uri, source_span) = resolved.remove(0);
    let Some(source_doc) = workspace.rsx.get(source_uri.as_str()) else {
        return MappedLocation::Unmapped;
    };
    let source_range = source_doc.line_index.span_to_range(source_span);
    MappedLocation::Source {
        uri: uri::to_lsp(&source_uri),
        range: to_lsp_range(source_range),
    }
}

/// [`narrow_raw_prefixed`] resolved to an LSP URI and range.
fn raw_prefixed_location(
    workspace: &Workspace,
    map: &outou_sourcemap::SourceMap,
    generated_text: &str,
    query: Span,
) -> Option<(lsp_types::Uri, lsp_types::Range)> {
    let source = narrow_raw_prefixed(workspace, map, generated_text, query)?;
    let source_uri = map.source_uri(source.source)?;
    let doc = workspace.rsx.get(source_uri.as_str())?;
    Some((
        uri::to_lsp(source_uri),
        to_lsp_range(doc.line_index.span_to_range(source.span)),
    ))
}

/// Result of [`narrow_single_source`].
enum NarrowOutcome {
    /// Narrowed to an exact sub-range.
    Exact {
        /// The `.rsx` file's URI.
        uri: lsp_types::Uri,
        /// The narrowed range within it.
        range: lsp_types::Range,
    },
    /// The containing mapping has exactly one source, but its span and
    /// the generated span differ in length (S1): the caller must report
    /// this as unmapped, not fall back to a coarser answer.
    LengthMismatch,
    /// Zero or several sources, no containing mapping, or a source file
    /// this server does not have text for: the caller should fall back
    /// to the registry's coarser full-span mapping.
    NotApplicable,
}

/// Narrows a generated query span to the exact sub-range of the one
/// `.rsx` source it came from, when the containing mapping has exactly
/// one source span — the unambiguous case. See [`NarrowOutcome`].
///
/// A thin LSP-specific wrapper (URI/range translation, and the
/// `Workspace`-only "no text for this source" case) around the shared
/// `outou_sourcemap::SourceMap::narrow`, which owns the actual proportional
/// narrowing and the `LengthMismatch` decision (issue #12 corpus review,
/// F4) — `outou-cli`'s UI test harness calls the same function directly.
fn narrow_single_source(
    workspace: &Workspace,
    map: &outou_sourcemap::SourceMap,
    query: Span,
) -> NarrowOutcome {
    match map.narrow(query) {
        outou_sourcemap::NarrowOutcome::NotApplicable => NarrowOutcome::NotApplicable,
        outou_sourcemap::NarrowOutcome::LengthMismatch => NarrowOutcome::LengthMismatch,
        outou_sourcemap::NarrowOutcome::Exact { source, span } => {
            let Some(source_uri) = map.source_uri(source) else {
                return NarrowOutcome::NotApplicable;
            };
            let Some(doc) = workspace.rsx.get(source_uri.as_str()) else {
                return NarrowOutcome::NotApplicable;
            };
            let range = doc.line_index.span_to_range(span);
            NarrowOutcome::Exact {
                uri: uri::to_lsp(source_uri),
                range: to_lsp_range(range),
            }
        }
    }
}

/// Reverse-maps one generated `(uri, range)` to **every** `.rsx` location
/// it came from, rather than [`generated_location_to_source`]'s single
/// (first-source) answer: used by rename and references
/// (`crate::rename`, `crate::references`).
///
/// Issue #14 review (CRITICAL-1): this must **not** simply flatten every
/// source of every mapping the query overlaps
/// (`outou_sourcemap::SourceMap::map_range`/`Registry::reverse`, which is
/// designed for a diagnostic or a coarse "what does this span come from"
/// query, and deliberately returns a *containing* mapping's whole source
/// span). Reproduced live: a rename of `Greeting` sends rust-analyzer an
/// 8-byte generated identifier query that falls *inside* the 22-byte
/// verbatim-copied `fn Greeting(x: i32) {}` mapping; the old
/// `Registry::reverse`-based implementation answered with that whole
/// 22-byte span, so the rename replaced the entire signature.
///
/// The rule (structural, not by [`outou_sourcemap::MappingKind`]):
///
/// 1. [`outou_sourcemap::SourceMap::narrow`] first — the same primitive
///    hover/definition already use (`narrow_single_source`,
///    [`generated_location_to_source`]). `NarrowOutcome::Exact` narrows to
///    that one offset-preserved sub-range: the common case, a verbatim
///    (single-source, equal-length) mapping containing the query.
///    `NarrowOutcome::LengthMismatch` (a source shorter or longer than its
///    generated counterpart) is unmappable: empty, never a proportional
///    guess. The one exception is a keyword-named attribute, `type`
///    lowered to `r#type` (ledger rows 17 and 61, ADR 0013 amendment):
///    when the generated text is exactly `r#` + the source text, the
///    fixed two-byte prefix is stripped exactly ([`narrow_raw_prefixed`]).
/// 2. Only when `narrow` finds no single unambiguous source
///    (`NarrowOutcome::NotApplicable` — no containing mapping, or one
///    with zero or several sources) does a whole-span answer apply, and
///    only for a mapping whose generated span **exactly equals** the
///    query: the genuine N:M case (a JSX element name mapped from both
///    its opening and closing tag, ADR 0007) is always written as exactly
///    the identifier's own bytes, nothing more, so an *exact* match is
///    the only way a multi/zero-source mapping can be the right answer. A
///    query that is merely a *sub-range* of such a mapping cannot be
///    disambiguated and returns nothing, rather than guessing which
///    source it partially belongs to.
///
/// Returns `None` when `generated_uri` is not a file this registry
/// produced at all (an ordinary `.rs` module or a dependency crate): the
/// caller must pass the location through unchanged. Returns `Some(vec![])`
/// when the location is inside a known generated file but resolves to no
/// source at all (synthesized code, a length-mismatched mapping, or an
/// undisambiguatable sub-range) — callers that must not silently drop
/// such an edit (rename) treat this as a reason to refuse; callers for
/// which dropping is safe (references, read-only) simply skip it.
pub fn generated_range_to_all_sources(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    range: lsp_types::Range,
) -> Option<Vec<(lsp_types::Uri, lsp_types::Range)>> {
    let targets = generated_range_to_source_targets(workspace, generated_uri, range)?;
    Some(
        targets
            .into_iter()
            .map(|target| (target.uri, target.range))
            .collect(),
    )
}

/// One `.rsx` location a generated range came from, with whether it is a
/// JSX attribute name (see [`generated_range_to_source_targets`]).
pub struct SourceTarget {
    /// The `.rsx` file's URI.
    pub uri: lsp_types::Uri,
    /// The range within it.
    pub range: lsp_types::Range,
    /// Whether the mapping this came from is a JSX attribute name
    /// (`MappingKind::Attribute`). A rename edit's new text is written for
    /// Rust (`r#type`); in `.rsx` an attribute name is plain (`type`), see
    /// `crate::rename`.
    pub is_attribute_name: bool,
}

/// [`generated_range_to_all_sources`], also reporting whether each source
/// is a JSX attribute name. Same contract and same `None` / `Some(empty)`
/// meaning.
///
/// Keyword-named props (ledger rows 17 and 61, ADR 0013 amendment): the
/// length-mismatched `type` -> `r#type` attribute mapping is no longer
/// empty when the generated text is exactly `r#` + the source text
/// ([`narrow_raw_prefixed`]); every other length mismatch still is.
pub fn generated_range_to_source_targets(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    range: lsp_types::Range,
) -> Option<Vec<SourceTarget>> {
    let generated = uri::to_outou(generated_uri);
    // Issue #14 review (SHOULD-LAND-11): the registry, not
    // `Workspace::generated`, is the authoritative "is this a generated
    // file at all" answer. Checking `workspace.generated` first would
    // treat a URI the registry itself knows as generated — but for which
    // this server currently has no live `GeneratedUnit` (a transient or
    // inconsistent state) — the same as an ordinary `.rs` file, and a
    // caller (`crate::references`) would pass its `.generated/…` location
    // straight through to the editor instead of dropping it.
    let map = workspace.registry.map_for_generated(&generated)?;
    let Some(unit) = workspace.generated.get(generated.as_str()) else {
        return Some(Vec::new());
    };
    let query = unit.line_index.range_to_span(to_outou_range(range));

    let (sources, kind): (Vec<outou_sourcemap::SourceSpan>, Option<MappingKind>) =
        match map.narrow(query) {
            outou_sourcemap::NarrowOutcome::Exact { source, span } => (
                vec![outou_sourcemap::SourceSpan::new(source, span)],
                smallest_containing_kind(map, query),
            ),
            // A transformed mapping is refused rather than guessed at
            // (ADR 0013), except the exact `r#` + name shape of a
            // keyword-named attribute (ledger row 61).
            outou_sourcemap::NarrowOutcome::LengthMismatch => {
                match narrow_raw_prefixed(workspace, map, &unit.text, query) {
                    Some(source) => (vec![source], Some(MappingKind::Attribute)),
                    None => (Vec::new(), None),
                }
            }
            outou_sourcemap::NarrowOutcome::NotApplicable => map
                .mappings
                .iter()
                .find(|mapping| mapping.generated == query)
                .map(|mapping| (mapping.sources.clone(), Some(mapping.kind)))
                .unwrap_or_default(),
        };
    let is_attribute_name = kind == Some(MappingKind::Attribute);

    Some(
        sources
            .into_iter()
            .filter_map(|source| {
                let source_uri = map.source_uri(source.source)?;
                let doc = workspace.rsx.get(source_uri.as_str())?;
                Some(SourceTarget {
                    uri: crate::uri::to_lsp(source_uri),
                    range: to_lsp_range(doc.line_index.span_to_range(source.span)),
                    is_attribute_name,
                })
            })
            .collect(),
    )
}

/// The kind of the smallest mapping whose generated span contains `query`
/// — the mapping [`outou_sourcemap::SourceMap::narrow`] resolves it with.
fn smallest_containing_kind(map: &outou_sourcemap::SourceMap, query: Span) -> Option<MappingKind> {
    map.mappings
        .iter()
        .filter(|m| m.generated.contains(query))
        .min_by_key(|m| span_len(m.generated))
        .map(|m| m.kind)
}

/// Best-effort fallback position for a generated-Rust diagnostic whose
/// exact range has no source mapping at all (synthesized code, most often
/// inside an `rsx!` macro expansion the backend generated): the nearest
/// mapped source span in the unit's own `.rsx` file, or that file's very
/// first position if the map has no mapping with a source at all.
///
/// Used only for a rust-analyzer/flycheck diagnostic with `severity ==
/// ERROR` (issue #9 Gate 3 review, M5): a hard compile error must never
/// be silently dropped just because it points at a span with no direct
/// source, even though the position shown is therefore approximate.
pub fn nearest_source_position(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    query: lsp_types::Range,
) -> lsp_types::Range {
    let fallback = lsp_types::Range::new(
        lsp_types::Position::new(0, 0),
        lsp_types::Position::new(0, 0),
    );
    let generated = uri::to_outou(generated_uri);
    let Some(map) = workspace.registry.map_for_generated(&generated) else {
        return fallback;
    };
    let Some(unit) = workspace.generated.get(generated.as_str()) else {
        return fallback;
    };
    let Some(doc) = workspace.rsx.get(unit.rsx_uri.as_str()) else {
        return fallback;
    };
    let query_span = unit.line_index.range_to_span(to_outou_range(query));

    let Some(nearest) = map
        .mappings
        .iter()
        .filter(|mapping| !mapping.sources.is_empty())
        .min_by_key(|mapping| generated_distance(mapping.generated, query_span))
    else {
        return fallback;
    };
    let source_span = nearest.sources[0].span;
    to_lsp_range(doc.line_index.span_to_range(source_span))
}

/// Byte distance between two spans: `0` when they overlap or touch,
/// otherwise the gap between the closer pair of endpoints.
fn generated_distance(a: Span, b: Span) -> u32 {
    if a.end <= b.start {
        b.start.saturating_sub(a.end)
    } else {
        a.start.saturating_sub(b.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outou_sourcemap::{file_uri, Mapping, SourceMap, SourceSpan};
    use std::path::Path;

    // Several test modules in this crate hand-build a similar
    // `Workspace { .. }` literal; this one is no longer a verbatim copy of
    // another, and each test states its own sources.
    fn sample_workspace() -> (Workspace, lsp_types::Uri, lsp_types::Uri) {
        // Build a minimal registry/document pair by hand rather than
        // going through `Workspace::load` (which needs real files on
        // disk): `let user = load_user();` at bytes 4..8 for `user`
        // mapping to generated bytes 30..34.
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/crate-root.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let mut registry = outou_sourcemap::Registry::new();
        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(30, 34),
                vec![SourceSpan::new(SourceId(0), Span::new(4, 8))],
                MappingKind::Identifier,
            ),
        );
        registry = registry.with_map(map);

        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new("let user = load_user();\n".to_string(), 0),
        );
        let generated_text = format!("{}user{}", "x".repeat(30), "y".repeat(10));
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );

        (
            workspace,
            uri::to_lsp(&rsx_uri),
            uri::to_lsp(&generated_uri),
        )
    }

    #[test]
    fn maps_an_rsx_position_forward_to_the_generated_position() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        // Position (0, 4) is the very start of "user" (source span 4..8),
        // so it lands exactly on the generated span's own start (30..34).
        let mapped =
            rsx_position_to_generated(&workspace, &rsx_uri, lsp_types::Position::new(0, 4))
                .expect("mapped");
        assert_eq!(mapped.generated_uri, generated_uri);
        assert_eq!(mapped.position, lsp_types::Position::new(0, 30));
    }

    #[test]
    fn maps_a_position_partway_through_a_span_proportionally() {
        let (workspace, rsx_uri, _generated_uri) = sample_workspace();
        // Position (0, 5) is one byte into "user" (source span 4..8,
        // generated span 30..34, both length 4): the generated position
        // must move by the same one byte, not snap back to the span's
        // start (this was HIGH: a naive "always the span's start"
        // implementation put every hover/definition inside a multi-line
        // verbatim Rust block back at that block's very first line).
        let mapped =
            rsx_position_to_generated(&workspace, &rsx_uri, lsp_types::Position::new(0, 5))
                .expect("mapped");
        assert_eq!(mapped.position, lsp_types::Position::new(0, 31));
    }

    #[test]
    fn maps_a_generated_location_back_to_the_rsx_source() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let mapped = generated_location_to_source(
            &workspace,
            &generated_uri,
            lsp_types::Range::new(
                lsp_types::Position::new(0, 30),
                lsp_types::Position::new(0, 34),
            ),
        );
        match mapped {
            MappedLocation::Source { uri, range } => {
                assert_eq!(uri, rsx_uri);
                assert_eq!(range.start, lsp_types::Position::new(0, 4));
                assert_eq!(range.end, lsp_types::Position::new(0, 8));
            }
            _ => panic!("expected a mapped source location"),
        }
    }

    #[test]
    fn nearest_source_position_falls_back_to_the_closest_mapping() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        // The one mapping is generated bytes 30..34; a query well past it
        // (60..64, inside the synthesized `rsx!` wrapper) has no mapping
        // of its own but should still resolve to the nearest one rather
        // than an arbitrary (0, 0).
        let query = lsp_types::Range::new(
            lsp_types::Position::new(0, 60),
            lsp_types::Position::new(0, 64),
        );
        let range = nearest_source_position(&workspace, &generated_uri, query);
        assert_eq!(range.start, lsp_types::Position::new(0, 4));
        assert_eq!(range.end, lsp_types::Position::new(0, 8));
        let _ = rsx_uri;
    }

    /// S1 (issue #9 Gate 3 review, HIGH-7): a transformed mapping — source
    /// 4 bytes (`tag.`, deliberately not the same length as the generated
    /// side) mapping to a *longer* generated span (quotes added, as
    /// `key={tag.clone()}` -> `key: "{tag.clone()}"` does in practice) —
    /// must map to nothing, not a proportionally-scaled wrong position.
    #[test]
    fn a_length_mismatched_mapping_maps_forward_to_nothing() {
        let (workspace, rsx_uri, _generated_uri) = sample_workspace();
        let mut workspace = workspace;
        // Overwrite the sample mapping with one whose generated span is
        // longer than its source span.
        let rsx_path = std::path::Path::new("/app/src/main.rsx");
        let generated_path = std::path::Path::new("/app/src/.generated/crate-root.rs");
        let rsx_uri_owned = file_uri(rsx_path);
        let generated_uri_owned = file_uri(generated_path);
        let map = SourceMap::new(generated_uri_owned.clone(), vec![rsx_uri_owned.clone()])
            .with_mapping(Mapping::new(
                Span::new(30, 36),                                   // 6 bytes generated
                vec![SourceSpan::new(SourceId(0), Span::new(4, 8))], // 4 bytes source
                MappingKind::Expression,
            ));
        workspace.registry = outou_sourcemap::Registry::new().with_map(map);

        let mapped =
            rsx_position_to_generated(&workspace, &rsx_uri, lsp_types::Position::new(0, 5));
        assert!(
            mapped.is_none(),
            "a length-mismatched mapping must not translate"
        );
    }

    /// Same fix, reverse direction: hovering inside the generated span of
    /// a length-mismatched mapping must report `Unmapped`, not a
    /// proportionally-scaled (confidently wrong) source range.
    #[test]
    fn a_length_mismatched_mapping_maps_backward_to_unmapped() {
        let rsx_path = std::path::Path::new("/app/src/main.rsx");
        let generated_path = std::path::Path::new("/app/src/.generated/crate-root.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);
        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(30, 36),
                vec![SourceSpan::new(SourceId(0), Span::new(4, 8))],
                MappingKind::Expression,
            ),
        );
        let registry = outou_sourcemap::Registry::new().with_map(map);
        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new("let tag = String::new();\n".to_string(), 0),
        );
        let generated_text = format!("{}\"{{tag}}\"{}", "x".repeat(30), "y".repeat(10));
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        let generated_uri_lsp = uri::to_lsp(&generated_uri);

        let mapped = generated_location_to_source(
            &workspace,
            &generated_uri_lsp,
            lsp_types::Range::new(
                lsp_types::Position::new(0, 31),
                lsp_types::Position::new(0, 32),
            ),
        );
        assert!(matches!(mapped, MappedLocation::Unmapped));
    }

    #[test]
    fn a_dependency_location_passes_through_unchanged() {
        let (workspace, _rsx_uri, _generated_uri) = sample_workspace();
        let dep_uri: lsp_types::Uri = "file:///home/.cargo/registry/src/foo/lib.rs"
            .parse()
            .unwrap();
        let mapped = generated_location_to_source(
            &workspace,
            &dep_uri,
            lsp_types::Range::new(
                lsp_types::Position::new(0, 0),
                lsp_types::Position::new(0, 1),
            ),
        );
        assert!(matches!(mapped, MappedLocation::Unchanged));
    }

    // --- `generated_range_to_all_sources` (issue #14 review, CRITICAL-1):
    // must narrow to the exact identifier inside a coarse verbatim
    // mapping, never replace the mapping's whole (much larger) span.

    /// A workspace with three mappings sharing one `.rsx` file:
    ///
    /// - A **verbatim, single-source, equal-length** `Expression` mapping
    ///   for a whole (deliberately not-really-valid-Rust, but
    ///   byte-identical) function signature `fn Greeting(x: i32) {}`,
    ///   standing in for the real corpus case ("`Registry::reverse`
    ///   returns the WHOLE 37-byte signature for a `Greeting` rename").
    /// - A **two-source** `Identifier` mapping whose generated span is
    ///   *exactly* one JSX element name occurrence (`element.rs`'s
    ///   `lower_element_body`: one generated identifier, sources at both
    ///   the opening and closing tag).
    /// - A **length-mismatched, single-source** `Attribute` mapping (6
    ///   generated bytes standing in for `r#type`, 4 source bytes for
    ///   `type`), standing in for the real `r#type` prop case.
    fn narrow_fixture_workspace() -> (
        Workspace,
        lsp_types::Uri,
        lsp_types::Uri,
        String,           // rsx source text, for asserting extracted substrings
        lsp_types::Range, // generated range: "Greeting" inside the verbatim signature
        lsp_types::Range, // generated range: the identifier mapping (both tags)
        lsp_types::Range, // generated range: the length-mismatched attribute mapping
    ) {
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/main.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let rsx_source = "fn Greeting(x: i32) {} <Greeting>hi</Greeting> type";
        let sig_end = rsx_source.find(" <Greeting>").unwrap();
        let sig_text = &rsx_source[0..sig_end];
        let occurrences: Vec<usize> = rsx_source
            .match_indices("Greeting")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            occurrences.len(),
            3,
            "fixture must have exactly 3 occurrences"
        );
        let sig_name_offset = occurrences[0] as u32; // "Greeting" inside the signature
        let open_start = occurrences[1] as u32;
        let close_start = occurrences[2] as u32;
        let type_start = rsx_source.rfind("type").unwrap() as u32;

        let prefix_len: u32 = 10;
        let sig_len = sig_text.len() as u32;
        let mid_len: u32 = 10;
        let ident_len: u32 = 8; // "Greeting"
        let mid2_len: u32 = 10;

        let generated_text = format!(
            "{}{}{}{}{}{}",
            "a".repeat(prefix_len as usize),
            sig_text,
            "b".repeat(mid_len as usize),
            "Greeting",
            "c".repeat(mid2_len as usize),
            "ABCDEF", // stands in for generated `r#type` (6 bytes)
        );

        let sig_greeting_generated_start = prefix_len + sig_name_offset;
        let ident_generated_start = prefix_len + sig_len + mid_len;
        let type_generated_start = prefix_len + sig_len + mid_len + ident_len + mid2_len;

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()])
            .with_mapping(Mapping::new(
                Span::new(prefix_len, prefix_len + sig_len),
                vec![SourceSpan::new(SourceId(0), Span::new(0, sig_len))],
                MappingKind::Expression,
            ))
            .with_mapping(Mapping::new(
                Span::new(ident_generated_start, ident_generated_start + ident_len),
                vec![
                    SourceSpan::new(SourceId(0), Span::new(open_start, open_start + 8)),
                    SourceSpan::new(SourceId(0), Span::new(close_start, close_start + 8)),
                ],
                MappingKind::Identifier,
            ))
            .with_mapping(Mapping::new(
                Span::new(type_generated_start, type_generated_start + 6),
                vec![SourceSpan::new(
                    SourceId(0),
                    Span::new(type_start, type_start + 4),
                )],
                MappingKind::Attribute,
            ));
        let registry = outou_sourcemap::Registry::new().with_map(map);

        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );

        let unit = workspace
            .generated
            .get(generated_uri.as_str())
            .expect("just inserted");
        let sig_greeting_range = to_lsp_range(unit.line_index.span_to_range(Span::new(
            sig_greeting_generated_start,
            sig_greeting_generated_start + 8,
        )));
        let ident_range = to_lsp_range(unit.line_index.span_to_range(Span::new(
            ident_generated_start,
            ident_generated_start + ident_len,
        )));
        let type_range = to_lsp_range(
            unit.line_index
                .span_to_range(Span::new(type_generated_start, type_generated_start + 6)),
        );

        (
            workspace,
            uri::to_lsp(&rsx_uri),
            uri::to_lsp(&generated_uri),
            rsx_source.to_string(),
            sig_greeting_range,
            ident_range,
            type_range,
        )
    }

    // --- Keyword-named props (ledger rows 17 and 61, ADR 0013 amendment):
    // `type` lowered to `r#type` translates by a fixed two-byte prefix.

    const RAW_SOURCE: &str = "<Field type=\"text\" />";
    const RAW_GENERATED: &str = "Field(r#type: \"text\")";

    /// One `Attribute` mapping, `type` (source 7..11) -> generated
    /// `RAW_GENERATED[6..12]`, with the given kind and texts.
    fn raw_prefix_workspace(
        kind: MappingKind,
        rsx_source: &str,
        generated_text: &str,
    ) -> (Workspace, lsp_types::Uri, lsp_types::Uri) {
        let rsx_uri = file_uri(Path::new("/app/src/main.rsx"));
        let generated_uri = file_uri(Path::new("/app/src/.generated/main.rs"));
        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(6, 12),
                vec![SourceSpan::new(SourceId(0), Span::new(7, 11))],
                kind,
            ),
        );
        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new().with_map(map),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        (
            workspace,
            uri::to_lsp(&rsx_uri),
            uri::to_lsp(&generated_uri),
        )
    }

    fn cols(line: u32, start: u32, end: u32) -> lsp_types::Range {
        lsp_types::Range::new(
            lsp_types::Position::new(line, start),
            lsp_types::Position::new(line, end),
        )
    }

    fn range_text(text: &str, range: lsp_types::Range) -> &str {
        &text[range.start.character as usize..range.end.character as usize]
    }

    #[test]
    fn a_raw_prefixed_attribute_maps_forward_past_the_prefix() {
        let (workspace, rsx_uri, _) =
            raw_prefix_workspace(MappingKind::Attribute, RAW_SOURCE, RAW_GENERATED);
        // Cursor at the start, in the middle and at the end of `type`
        // (7, 9, 11) lands 2 bytes past `r#`, offset-for-offset: 8, 10, 12.
        for (source_column, generated_column) in [(7, 8), (9, 10), (11, 12)] {
            let mapped = rsx_position_to_generated(
                &workspace,
                &rsx_uri,
                lsp_types::Position::new(0, source_column),
            )
            .expect("mapped");
            assert_eq!(
                mapped.position,
                lsp_types::Position::new(0, generated_column)
            );
        }
    }

    #[test]
    fn a_raw_prefix_translates_only_when_the_generated_text_is_exactly_r_hash_plus_the_source() {
        // Same lengths, wrong generated text: still refused.
        let (workspace, rsx_uri, generated_uri) = raw_prefix_workspace(
            MappingKind::Attribute,
            RAW_SOURCE,
            "Field(ABCDEF: \"text\")",
        );
        assert!(
            rsx_position_to_generated(&workspace, &rsx_uri, lsp_types::Position::new(0, 9))
                .is_none()
        );
        assert_eq!(
            generated_range_to_all_sources(&workspace, &generated_uri, cols(0, 6, 12)),
            Some(Vec::new())
        );
        // Right text, wrong name (`r#kind` for a source `type`).
        let (workspace, rsx_uri, _) = raw_prefix_workspace(
            MappingKind::Attribute,
            RAW_SOURCE,
            "Field(r#kind: \"text\")",
        );
        assert!(
            rsx_position_to_generated(&workspace, &rsx_uri, lsp_types::Position::new(0, 9))
                .is_none()
        );
    }

    #[test]
    fn a_raw_prefix_translates_only_for_an_attribute_mapping() {
        let (workspace, rsx_uri, generated_uri) =
            raw_prefix_workspace(MappingKind::Expression, RAW_SOURCE, RAW_GENERATED);
        assert!(
            rsx_position_to_generated(&workspace, &rsx_uri, lsp_types::Position::new(0, 9))
                .is_none()
        );
        assert_eq!(
            generated_range_to_all_sources(&workspace, &generated_uri, cols(0, 6, 12)),
            Some(Vec::new())
        );
        assert!(matches!(
            generated_location_to_source(&workspace, &generated_uri, cols(0, 6, 12)),
            MappedLocation::Unmapped
        ));
    }

    #[test]
    fn the_whole_raw_prefixed_name_maps_back_to_the_plain_name() {
        let (workspace, rsx_uri, generated_uri) =
            raw_prefix_workspace(MappingKind::Attribute, RAW_SOURCE, RAW_GENERATED);
        let sources = generated_range_to_all_sources(&workspace, &generated_uri, cols(0, 6, 12))
            .expect("a known generated file");
        assert_eq!(sources, vec![(rsx_uri.clone(), cols(0, 7, 11))]);
        assert_eq!(range_text(RAW_SOURCE, sources[0].1), "type");
        match generated_location_to_source(&workspace, &generated_uri, cols(0, 6, 12)) {
            MappedLocation::Source { uri, range } => {
                assert_eq!(uri, rsx_uri);
                assert_eq!(range, cols(0, 7, 11));
            }
            _ => panic!("expected a mapped source location"),
        }
    }

    #[test]
    fn a_raw_prefixed_name_without_its_prefix_maps_back_at_a_fixed_offset() {
        let (workspace, _, generated_uri) =
            raw_prefix_workspace(MappingKind::Attribute, RAW_SOURCE, RAW_GENERATED);
        // `type` alone (8..12) and `yp` (9..11) inside it.
        for (generated, expected) in [
            (cols(0, 8, 12), cols(0, 7, 11)),
            (cols(0, 9, 11), cols(0, 8, 10)),
        ] {
            let sources = generated_range_to_all_sources(&workspace, &generated_uri, generated)
                .expect("a known generated file");
            assert_eq!(sources.len(), 1, "{sources:#?}");
            assert_eq!(sources[0].1, expected);
        }
    }

    #[test]
    fn a_query_touching_the_raw_prefix_but_not_the_whole_token_has_no_source() {
        let (workspace, _, generated_uri) =
            raw_prefix_workspace(MappingKind::Attribute, RAW_SOURCE, RAW_GENERATED);
        for generated in [cols(0, 6, 8), cols(0, 6, 10), cols(0, 7, 12)] {
            let sources = generated_range_to_all_sources(&workspace, &generated_uri, generated)
                .expect("a known generated file");
            assert!(sources.is_empty(), "{generated:?}: {sources:#?}");
            assert!(matches!(
                generated_location_to_source(&workspace, &generated_uri, generated),
                MappedLocation::Unmapped
            ));
        }
    }

    #[test]
    fn source_targets_flag_an_attribute_name() {
        let (workspace, _, generated_uri) =
            raw_prefix_workspace(MappingKind::Attribute, RAW_SOURCE, RAW_GENERATED);
        let targets = generated_range_to_source_targets(&workspace, &generated_uri, cols(0, 6, 12))
            .expect("a known generated file");
        assert_eq!(targets.len(), 1);
        assert!(targets[0].is_attribute_name);
        // An equal-length identifier mapping is not an attribute name.
        let (workspace, _, generated_uri) = sample_workspace();
        let targets =
            generated_range_to_source_targets(&workspace, &generated_uri, cols(0, 30, 34))
                .expect("a known generated file");
        assert_eq!(targets.len(), 1);
        assert!(!targets[0].is_attribute_name);
    }

    /// Real backend output: the attribute mapping of `<Field type="text" />`
    /// really is `r#type` over `type`, so the translation holds against what
    /// codegen emits, not just a hand-built map.
    #[test]
    fn a_keyword_attribute_maps_both_ways_against_real_generated_code() {
        use outou_codegen::{Backend, GenerateOptions, Mode};

        let rsx_source = "fn f() { <Field type=\"text\" r#for=\"x\" />; }";
        let rsx_uri = file_uri(Path::new("/app/src/main.rsx"));
        let generated_uri = file_uri(Path::new("/app/src/.generated/main.rs"));
        let parsed = outou_syntax::parse(rsx_source);
        let generated = outou_backend_dioxus::DioxusBackend
            .generate(
                &parsed,
                rsx_source,
                Mode::Strict,
                &GenerateOptions::new(generated_uri.clone(), rsx_uri.clone()),
            )
            .expect("generation succeeds");
        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new().with_map(generated.source_map.clone()),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated.rust),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        let rsx_lsp = uri::to_lsp(&rsx_uri);
        let generated_lsp = uri::to_lsp(&generated_uri);
        let unit = workspace.generated.get(generated_uri.as_str()).unwrap();

        // `type` is plain-typed `r#type`; `r#for` (written raw in the
        // source, 5 bytes) lowers to `r#for` too: equal length, exact.
        for (written, generated_name) in [("type", "r#type"), ("r#for", "r#for")] {
            let source_start = rsx_source.find(written).unwrap() as u32;
            let generated_start = generated.rust.find(generated_name).unwrap() as u32;
            let source_position = workspace
                .rsx
                .get(rsx_lsp.as_str())
                .unwrap()
                .line_index
                .offset_to_position(source_start + 1);
            let forward =
                rsx_position_to_generated(&workspace, &rsx_lsp, to_lsp_position(source_position))
                    .expect("forward mapping");
            let forward_offset = unit
                .line_index
                .position_to_offset(to_outou_position(forward.position));
            let name_len = written.len() as u32;
            let prefix = generated_name.len() as u32 - name_len;
            assert_eq!(forward_offset, generated_start + prefix + 1, "{written}");

            let generated_range = to_lsp_range(unit.line_index.span_to_range(Span::new(
                generated_start,
                generated_start + generated_name.len() as u32,
            )));
            let sources =
                generated_range_to_all_sources(&workspace, &generated_lsp, generated_range)
                    .expect("a known generated file");
            assert_eq!(sources.len(), 1, "{written}: {sources:#?}");
            let doc = workspace.rsx.get(rsx_lsp.as_str()).unwrap();
            let span = doc.line_index.range_to_span(to_outou_range(sources[0].1));
            assert_eq!(&rsx_source[span.start as usize..span.end as usize], written);
        }
    }

    /// Issue #26: a `key` expression is copied verbatim into a synthesized
    /// binding, so position requests inside it work in both directions
    /// (hover, references and rename need exactly this); they used to be
    /// refused because `key={tag}` became `key: "{tag}"`. Uses the real
    /// backend output, not a hand-built map.
    #[test]
    fn a_key_expression_maps_both_ways_against_real_generated_code() {
        use outou_codegen::{Backend, GenerateOptions, Mode};

        let rsx_source = "fn f() { <li key={tag.clone()}></li>; }";
        let rsx_uri = file_uri(Path::new("/app/src/main.rsx"));
        let generated_uri = file_uri(Path::new("/app/src/.generated/main.rs"));
        let parsed = outou_syntax::parse(rsx_source);
        let generated = outou_backend_dioxus::DioxusBackend
            .generate(
                &parsed,
                rsx_source,
                Mode::Strict,
                &GenerateOptions::new(generated_uri.clone(), rsx_uri.clone()),
            )
            .expect("generation succeeds");
        let key_start = rsx_source.find("tag.clone()").unwrap() as u32;
        let generated_start = generated.rust.find("tag.clone()").unwrap() as u32;

        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new().with_map(generated.source_map.clone()),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated.rust),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        let rsx_lsp = uri::to_lsp(&rsx_uri);
        let generated_lsp = uri::to_lsp(&generated_uri);

        // Forward: a cursor on `tag` (as hover, rename and references do)
        // lands on the `tag` of the binding, byte for byte.
        let unit = workspace.generated.get(generated_uri.as_str()).unwrap();
        let expected = to_lsp_range(
            unit.line_index
                .span_to_range(Span::new(generated_start + 1, generated_start + 1)),
        )
        .start;
        let mapped = rsx_position_to_generated(
            &workspace,
            &rsx_lsp,
            lsp_types::Position::new(0, key_start + 1),
        )
        .expect("a position inside a key expression maps forward");
        assert_eq!(mapped.generated_uri, generated_lsp);
        assert_eq!(mapped.position, expected);

        // Reverse: the identifier in the generated binding maps back to
        // exactly the `tag` written in the `.rsx` file.
        let range = to_lsp_range(
            workspace
                .generated
                .get(generated_uri.as_str())
                .unwrap()
                .line_index
                .span_to_range(Span::new(generated_start, generated_start + 3)),
        );
        let sources = generated_range_to_all_sources(&workspace, &generated_lsp, range)
            .expect("a known generated file");
        assert_eq!(sources.len(), 1, "{sources:#?}");
        let doc = workspace.rsx.get(rsx_lsp.as_str()).unwrap();
        let span = doc.line_index.range_to_span(to_outou_range(sources[0].1));
        assert_eq!(&rsx_source[span.start as usize..span.end as usize], "tag");
    }

    /// Issue #27: a child expression is wrapped in `child(..)` but copied
    /// verbatim inside it, so position requests inside it still map both
    /// ways (and the synthesized `child(` maps to nothing). Real backend
    /// output.
    #[test]
    fn a_child_expression_maps_both_ways_against_real_generated_code() {
        use outou_codegen::{Backend, GenerateOptions, Mode};

        let rsx_source = "fn f() { <p>{count.clone()}</p>; }";
        let rsx_uri = file_uri(Path::new("/app/src/main.rsx"));
        let generated_uri = file_uri(Path::new("/app/src/.generated/main.rs"));
        let parsed = outou_syntax::parse(rsx_source);
        let generated = outou_backend_dioxus::DioxusBackend
            .generate(
                &parsed,
                rsx_source,
                Mode::Strict,
                &GenerateOptions::new(generated_uri.clone(), rsx_uri.clone()),
            )
            .expect("generation succeeds");
        assert!(
            generated.rust.contains("child(count.clone())"),
            "{}",
            generated.rust
        );
        let child_start = rsx_source.find("count.clone()").unwrap() as u32;
        let generated_start = generated.rust.find("count.clone()").unwrap() as u32;

        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new().with_map(generated.source_map.clone()),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated.rust),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );
        let rsx_lsp = uri::to_lsp(&rsx_uri);
        let generated_lsp = uri::to_lsp(&generated_uri);

        // Forward: a cursor inside `count` lands inside the generated call.
        let unit = workspace.generated.get(generated_uri.as_str()).unwrap();
        let expected = to_lsp_range(
            unit.line_index
                .span_to_range(Span::new(generated_start + 2, generated_start + 2)),
        )
        .start;
        let mapped = rsx_position_to_generated(
            &workspace,
            &rsx_lsp,
            lsp_types::Position::new(0, child_start + 2),
        )
        .expect("a position inside a child expression maps forward");
        assert_eq!(mapped.generated_uri, generated_lsp);
        assert_eq!(mapped.position, expected);

        // Reverse: the identifier inside the generated call maps back to
        // exactly the `count` written in the `.rsx` file.
        let range = to_lsp_range(
            workspace
                .generated
                .get(generated_uri.as_str())
                .unwrap()
                .line_index
                .span_to_range(Span::new(generated_start, generated_start + 5)),
        );
        let sources = generated_range_to_all_sources(&workspace, &generated_lsp, range)
            .expect("a known generated file");
        assert_eq!(sources.len(), 1, "{sources:#?}");
        let doc = workspace.rsx.get(rsx_lsp.as_str()).unwrap();
        let span = doc.line_index.range_to_span(to_outou_range(sources[0].1));
        assert_eq!(&rsx_source[span.start as usize..span.end as usize], "count");
    }

    /// CRITICAL-1: querying the 8 generated bytes of the identifier
    /// `Greeting` *inside* a much larger verbatim (single-source,
    /// equal-length) mapping must narrow to exactly those 8 `.rsx` bytes —
    /// never the whole 22-byte signature the old `Registry::reverse`-based
    /// implementation returned.
    #[test]
    fn generated_range_to_all_sources_narrows_inside_a_verbatim_mapping() {
        let (workspace, rsx_uri, generated_uri, rsx_source, sig_greeting_range, _, _) =
            narrow_fixture_workspace();
        let sources =
            generated_range_to_all_sources(&workspace, &generated_uri, sig_greeting_range)
                .expect("a known generated file");
        assert_eq!(sources.len(), 1, "{sources:#?}");
        let (uri, range) = &sources[0];
        assert_eq!(*uri, rsx_uri);
        let doc = workspace.rsx.get(rsx_uri.as_str()).unwrap();
        let span = doc.line_index.range_to_span(to_outou_range(*range));
        assert_eq!(
            &rsx_source[span.start as usize..span.end as usize],
            "Greeting"
        );
    }

    /// A query whose generated span **exactly equals** a genuine N:M
    /// identifier mapping's own generated span returns every one of its
    /// full sources (both the opening and closing tag) — the one case
    /// where a whole-span answer is correct.
    #[test]
    fn generated_range_to_all_sources_returns_both_tags_for_an_exact_identifier_mapping() {
        let (workspace, rsx_uri, generated_uri, rsx_source, _, ident_range, _) =
            narrow_fixture_workspace();
        let sources = generated_range_to_all_sources(&workspace, &generated_uri, ident_range)
            .expect("a known generated file");
        assert_eq!(sources.len(), 2, "{sources:#?}");
        let doc = workspace.rsx.get(rsx_uri.as_str()).unwrap();
        for (uri, range) in &sources {
            assert_eq!(*uri, rsx_uri);
            let span = doc.line_index.range_to_span(to_outou_range(*range));
            assert_eq!(
                &rsx_source[span.start as usize..span.end as usize],
                "Greeting"
            );
        }
    }

    /// Issue #14 review (SHOULD-LAND-11): a URI the registry itself
    /// considers generated (`Registry::is_generated`/`map_for_generated`)
    /// must never be treated the same as an ordinary `.rs` file just
    /// because this server currently has no live `GeneratedUnit` for it
    /// (a transient/inconsistent state) — that would make a caller
    /// (`crate::references`) pass a `.generated/…` location straight
    /// through to the editor, exactly the leak `Registry::reverse`'s
    /// `None` return is supposed to mean "not generated at all," not
    /// "generated but temporarily unavailable." The authoritative check
    /// must be the registry, not `Workspace::generated`.
    #[test]
    fn generated_range_to_all_sources_never_passes_through_a_known_generated_uri() {
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/main.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(0, 4),
                vec![SourceSpan::new(SourceId(0), Span::new(0, 4))],
                MappingKind::Expression,
            ),
        );
        let registry = outou_sourcemap::Registry::new().with_map(map);

        // Deliberately no `workspace.generated` entry for this URI, even
        // though the registry above knows it as a generated file.
        let workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
            plan_error: None,
        };

        let generated_uri_lsp = uri::to_lsp(&generated_uri);
        let range = lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(0, 4),
        );
        let result = generated_range_to_all_sources(&workspace, &generated_uri_lsp, range);
        assert_eq!(
            result,
            Some(Vec::new()),
            "a known-generated URI must resolve to Some(empty), never None (pass-through)"
        );
    }

    /// A sub-range query against a length-mismatched mapping (the
    /// `r#type`/`type` case) must return nothing at all, never a
    /// proportionally-scaled guess and never the whole mismatched span.
    #[test]
    fn generated_range_to_all_sources_is_empty_for_a_length_mismatched_mapping() {
        let (workspace, _rsx_uri, generated_uri, _rsx_source, _, _, type_range) =
            narrow_fixture_workspace();
        let sources = generated_range_to_all_sources(&workspace, &generated_uri, type_range)
            .expect("a known generated file");
        assert!(sources.is_empty(), "{sources:#?}");
    }
}
