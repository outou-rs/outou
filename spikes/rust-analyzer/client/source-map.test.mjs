// Tests for the pure source-map lookup used by ra-client.mjs.
//
// Run with: node --test spikes/rust-analyzer/client/*.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { mapRange } from "./source-map.mjs";

const sourceMap = {
  version: 1,
  generated: "fixture/src/.generated/App.rs",
  sources: ["fixture/src/App.rsx"],
  mappings: [
    {
      kind: "identifier",
      what: "user declaration",
      generated: { start: { line: 8, character: 8 }, end: { line: 8, character: 12 } },
      sources: [{ file: 0, start: { line: 4, character: 8 }, end: { line: 4, character: 12 } }],
    },
    {
      kind: "other",
      what: "rsx! wrapper: synthesized, no source span",
      generated: { start: { line: 10, character: 4 }, end: { line: 10, character: 10 } },
      sources: [],
    },
  ],
};

test("a range fully contained in a mapping's generated range resolves its sources", () => {
  const range = { start: { line: 8, character: 9 }, end: { line: 8, character: 11 } };
  const result = mapRange(sourceMap, range);
  assert.equal(result.unmapped, false);
  assert.deepEqual(result.sources, [
    { file: "fixture/src/App.rsx", start: { line: 4, character: 8 }, end: { line: 4, character: 12 } },
  ]);
});

test("a range that only overlaps a mapping's generated range still resolves it", () => {
  // Starts inside mapping[0] (8,8)-(8,12) but extends past its end.
  const range = { start: { line: 8, character: 10 }, end: { line: 8, character: 14 } };
  const result = mapRange(sourceMap, range);
  assert.equal(result.unmapped, false);
  assert.deepEqual(result.sources, [
    { file: "fixture/src/App.rsx", start: { line: 4, character: 8 }, end: { line: 4, character: 12 } },
  ]);
});

test("a range with no intersecting mapping is reported as unmapped", () => {
  const range = { start: { line: 20, character: 0 }, end: { line: 20, character: 5 } };
  const result = mapRange(sourceMap, range);
  assert.equal(result.unmapped, true);
  assert.deepEqual(result.sources, []);
});

test("a matched mapping with an empty sources list is not unmapped", () => {
  const range = { start: { line: 10, character: 4 }, end: { line: 10, character: 10 } };
  const result = mapRange(sourceMap, range);
  assert.equal(result.unmapped, false);
  assert.deepEqual(result.sources, []);
});

// Local fixture with two mappings whose `generated` ranges overlap each
// other (neither contains the query range), used to exercise the
// filter()-based, multi-match behavior of mapRange().
const spanningSourceMap = {
  version: 1,
  generated: "fixture/src/.generated/App.rs",
  sources: ["fixture/src/App.rsx"],
  mappings: [
    {
      kind: "identifier",
      what: "first half",
      generated: { start: { line: 8, character: 8 }, end: { line: 8, character: 14 } },
      sources: [{ file: 0, start: { line: 4, character: 8 }, end: { line: 4, character: 12 } }],
    },
    {
      kind: "identifier",
      what: "second half",
      generated: { start: { line: 8, character: 13 }, end: { line: 8, character: 20 } },
      sources: [{ file: 0, start: { line: 5, character: 0 }, end: { line: 5, character: 7 } }],
    },
  ],
};

test("a range spanning two mappings resolves both source spans, in order", () => {
  // Overlaps both mappings' generated ranges; neither contains it.
  const range = { start: { line: 8, character: 10 }, end: { line: 8, character: 16 } };
  const result = mapRange(spanningSourceMap, range);
  assert.equal(result.unmapped, false);
  assert.deepEqual(result.sources, [
    { file: "fixture/src/App.rsx", start: { line: 4, character: 8 }, end: { line: 4, character: 12 } },
    { file: "fixture/src/App.rsx", start: { line: 5, character: 0 }, end: { line: 5, character: 7 } },
  ]);
});

test("duplicate source spans across two matched mappings are deduped", () => {
  const duplicateSourceMap = {
    version: 1,
    generated: "fixture/src/.generated/App.rs",
    sources: ["fixture/src/App.rsx"],
    mappings: [
      {
        kind: "identifier",
        what: "first half",
        generated: { start: { line: 8, character: 8 }, end: { line: 8, character: 14 } },
        sources: [{ file: 0, start: { line: 4, character: 8 }, end: { line: 4, character: 12 } }],
      },
      {
        kind: "identifier",
        what: "second half, same source span as the first",
        generated: { start: { line: 8, character: 13 }, end: { line: 8, character: 20 } },
        sources: [{ file: 0, start: { line: 4, character: 8 }, end: { line: 4, character: 12 } }],
      },
    ],
  };
  const range = { start: { line: 8, character: 10 }, end: { line: 8, character: 16 } };
  const result = mapRange(duplicateSourceMap, range);
  assert.equal(result.unmapped, false);
  assert.deepEqual(result.sources, [
    { file: "fixture/src/App.rsx", start: { line: 4, character: 8 }, end: { line: 4, character: 12 } },
  ]);
});
