# outou-sourcemap

Many-to-many source maps between `.rsx` files and generated Rust, and a workspace-wide registry (generated URI → map → original URIs).
Reverse-mapping rule: if a `.rsx` file produced the location, map back to it; otherwise return the Rust location untouched.

Phase 0: Week 4.

## Overview

- [`Span`] is a half-open UTF-8 byte range `[start, end)` inside one file. [`SourceSpan`] pairs a `Span` with a [`SourceId`], an index into a [`SourceMap`]'s source files.
- [`Mapping`] is one entry of a map: a generated `Span`, zero or more source `SourceSpan`s (empty means synthesized code), a [`MappingKind`], and an optional human-readable `label`.
- [`SourceMap`] holds every `Mapping` for one generated Rust file, plus the generated file's `Uri` and the `Uri`s of its source files. [`SourceMapBuilder`] builds one incrementally during codegen.
- [`Registry`] indexes every generated file's `SourceMap`, workspace-wide, and implements the [ADR 0007](../../docs/adr/0007-source-map-many-to-many.md) reverse-mapping rule via [`Registry::reverse`].
- [`LineIndex`] converts between byte offsets and LSP `{line, character}` [`Position`]s for one file's text.

## Position semantics

A [`Position`] is 0-based, matching the LSP `Position` type: `line` counts line terminators (`\n`, `\r\n` or a lone `\r`), and `character` counts **UTF-16 code units** within the line — not bytes, and not Unicode scalar values. A character outside the Basic Multilingual Plane (most emoji) therefore advances `character` by two, matching how LSP clients count it.

[`LineIndex`] is built once from a file's full text and converts in both directions:

- A byte offset past the end of the text clamps to the end.
- A `Position` whose `character` is past a line's content clamps to that line's content end (before its line terminator).
- A `Position` whose `line` is past the last line clamps to the last line.
- A byte offset that lands inside a line terminator (for example, between the `\r` and `\n` of a CRLF pair) normalises to that line's content end.
- A `character` that would bisect a UTF-16 surrogate pair floors to the start of the character it bisects, rather than overshooting into the next one.
- Round-tripping (`offset -> Position -> offset`) is exact only for offsets that are UTF-8 character boundaries and are not inside a line terminator.

## JSON format

[`SourceMapJson`] is the on-disk shape a compiler-written source map uses, compatible with the hand-written spike format the language-server spike client reads (`spikes/rust-analyzer/source-map.json`, `spikes/rust-analyzer/client/source-map.mjs`):

```json
{
  "version": 1,
  "generated": "src/.generated/App.rs",
  "sources": ["src/App.rsx"],
  "mappings": [
    {
      "kind": "identifier",
      "what": "user declaration",
      "generated": { "start": { "line": 8, "character": 8 }, "end": { "line": 8, "character": 12 } },
      "sources": [
        { "file": 0, "start": { "line": 4, "character": 8 }, "end": { "line": 4, "character": 12 } }
      ]
    }
  ]
}
```

- `version` is always `1`.
- `generated` and `sources[]` are paths or URIs, as written by the compiler.
- Each mapping's `generated` range and each of its `sources[]` entries use 0-based `{line, character}` positions (see "Position semantics" above), not byte offsets.
- `sources[]` may be empty (synthesized code) or hold several entries (many-to-many).
- `what` is optional and purely informational.

[`SourceMap::to_json`] converts an in-memory map (byte `Span`s) to this JSON shape, given the generated file's text and each source file's text (to build the `LineIndex`es); it returns [`ToJsonError`] if a mapping references a source id beyond the map's declared `sources`, or one with no matching entry in `source_texts`. [`SourceMapJson::into_source_map`] converts back, returning [`FromJsonError`] for an unsupported `version`, an inverted `start`/`end` range, a `sources[].file` index absent from the declared `sources` list, or an out-of-range `sources[].file` index (no matching text). Neither function performs file I/O; callers serialize/deserialize `SourceMapJson` with `serde_json` themselves.

[`SourceMapBuilder::try_with_mapping`] builds mappings the same way: it returns `Result<_, BuildError>`, rejecting an inverted generated span, an inverted source span, or a source span whose id was never registered with `with_source`, rather than accepting silently-corrupt data.

## Reverse mapping for the LSP

[`SourceMap::map_range`] answers "what source spans apply to this generated span", with the same precedence rule as the spike client: mappings whose generated span *contains* the query take precedence over ones that merely overlap it. The overlap fallback uses the spike's strict rule (`overlapsRange`): a zero-width mapping never overlaps a non-empty query, only one that contains (or equals) it. Source spans from every matching mapping are flattened, in mapping order, and deduplicated by resolved URI (falling back to the source id when it does not resolve) plus span. The result's `unmapped` flag is `true` only when nothing intersects the query at all — a matched mapping with an empty `sources` list (synthesized code) is a normal, non-`unmapped` empty result.

[`Registry::reverse`] wraps this for a whole workspace: `None` means `generated_uri` is not a file this registry produced, so the caller must return the location untouched (a plain `.rs` module or a dependency crate). `Some(spans)` — including an empty vector — means the location was recognized as generated.

[`Span`]: src/span.rs
[`SourceSpan`]: src/span.rs
[`SourceId`]: src/span.rs
[`Mapping`]: src/lib.rs
[`SourceMap`]: src/lib.rs
[`SourceMapBuilder`]: src/builder.rs
[`Registry`]: src/registry.rs
[`Registry::reverse`]: src/registry.rs
[`LineIndex`]: src/line_index.rs
[`Position`]: src/line_index.rs
[`SourceMapJson`]: src/json.rs
[`SourceMap::to_json`]: src/json.rs
[`SourceMapJson::into_source_map`]: src/json.rs
[`FromJsonError`]: src/json.rs
[`SourceMap::map_range`]: src/lib.rs
[`MappingKind`]: src/lib.rs
[`ToJsonError`]: src/json.rs
[`SourceMapBuilder::try_with_mapping`]: src/builder.rs
[`BuildError`]: src/builder.rs
