// Pure mapping from a generated-file LSP range back to `App.rsx` source spans,
// using the spike's hand-written `source-map.json` format (see
// spikes/rust-analyzer/source-map.json): `mappings[]` entries each carry a
// `generated` range and zero or more `sources[]` ranges (many-to-many,
// 0-based LSP positions). No I/O here; ra-client.mjs reads the JSON file.

/** Compare two LSP positions. Negative if `a` is before `b`. */
function comparePositions(a, b) {
  if (a.line !== b.line) return a.line - b.line;
  return a.character - b.character;
}

/** Whether `outer` fully contains `inner`. */
function containsRange(outer, inner) {
  return comparePositions(outer.start, inner.start) <= 0 && comparePositions(outer.end, inner.end) >= 0;
}

/** Whether two ranges share any span. */
function overlapsRange(a, b) {
  return comparePositions(a.start, b.end) < 0 && comparePositions(b.start, a.end) < 0;
}

/**
 * Find every mapping whose `generated` range contains, or otherwise
 * overlaps, `range`, and resolve their `sources[]` file indices to paths.
 *
 * Precedence: if any mapping *contains* `range`, only the containing
 * mapping(s) are used; otherwise every mapping that merely *overlaps*
 * `range` is used. All matches' `sources[]` are flattened into a single
 * list, in mapping order, and deduped by `file|start|end`.
 *
 * Returns `{ sources, unmapped }`. `unmapped` is true only when no mapping
 * intersects `range` at all; a matched mapping with an empty `sources` list
 * (the many-to-zero case) is not unmapped.
 */
export function mapRange(sourceMap, range) {
  const mappings = sourceMap?.mappings ?? [];
  const contained = mappings.filter((mapping) => containsRange(mapping.generated, range));
  const matches = contained.length > 0 ? contained : mappings.filter((mapping) => overlapsRange(mapping.generated, range));

  if (matches.length === 0) {
    return { sources: [], unmapped: true };
  }

  const seen = new Set();
  const sources = [];
  for (const mapping of matches) {
    for (const source of mapping.sources ?? []) {
      const file = sourceMap.sources?.[source.file] ?? source.file;
      const key = `${file}|${source.start.line},${source.start.character}|${source.end.line},${source.end.character}`;
      if (seen.has(key)) continue;
      seen.add(key);
      sources.push({ file, start: source.start, end: source.end });
    }
  }
  return { sources, unmapped: false };
}
