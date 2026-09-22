---
title: "[Droppable] Semantic tokens and rename / references"
milestone: "Phase 0"
week: "Week 7 if time allows"
gate: "—"
droppable: yes
labels: [phase0, droppable]
---

| | |
|---|---|
| **Gate** | none |
| **Droppable** | **Yes — semantic tokens are cut third (a TextMate grammar stands in), rename/references second** |

- [x] TextMate grammar for `outou-rsx` in `packages/vscode-outou/` (the stand-in)
- [x] semantic tokens: Rust keyword/type/variable from rust-analyzer mapped back, plus component, HTML element, attribute, event, JSX text
- [x] rename updates both the opening and the closing tag through the N:M source map
- [x] references

See `crates/outou-lsp/README.md`'s "Semantic tokens, rename, references"
section, `docs/adr/0012-semantic-tokens-legend-and-overlay.md` and
`docs/adr/0013-rename-translation-and-refusal.md` for the design and its
known gaps (`docs/backend-leakage.md` row 29: a keyword-named prop has no
rename/references answer at all; `prepareRename` on a closing tag returns
the opening tag's range).

An independent review of the first pass (issue #14 review) found the
initial implementation unmergeable: the core translation primitive used
`outou_sourcemap::Registry::reverse` directly, which returns a
*containing* mapping's whole source span rather than an offset-preserved
sub-range — reproduced live, renaming the component `Greeting` replaced
its entire 22-byte `fn Greeting(x: i32) {}` signature with the new name,
not just the 8-byte identifier. Fixed by rebuilding
`crate::mapping::generated_range_to_all_sources` on
`outou_sourcemap::SourceMap::narrow` (ADR 0013's own "Consequences"
section). The review also found the rust-analyzer legend this server
assumed to be a superset of Outou's five overlay types is not, in a real
rust-analyzer 1.98.1 (no `class`/`event`); fixed by
`Legend::extended_with_outou_types` (ADR 0012).

Every one of these fixes, plus the review's other findings (in-flight
staleness on document version, TextMate grammar false positives on Rust
generics, an intrinsic-tag-name rename refusing locally, rust-analyzer
error sanitization, integer-overflow safety on untrusted token deltas),
is now covered by:

- Unit and dispatch-level tests (`cargo test -p outou-lsp`, fake
  rust-analyzer responses) for every translation/refusal/overflow case.
- Three real, `--ignored` rust-analyzer probes added to `tests/gate3.rs`
  and `spikes/rust-analyzer/client/outou-lsp-client.mjs` — `rename-component`
  (renaming `Greeting` -> `Welcome` in `examples/phase0-app`: >= 3
  edits, each exactly 8 columns wide, no duplicates, no `.generated/…`
  URI), `semantic-tokens-full` (sorted, non-overlapping, includes a
  `class`-typed token even though rust-analyzer's own legend has none),
  and `references-component` (every reference exactly 8 columns wide, no
  `.generated/…` URI) — run via `cargo test -p outou-lsp --test gate3 --
  --ignored` (real rust-analyzer 1.98.1; verbatim pass/fail reported in
  the PR/session notes, not duplicated here since this file is not a
  running log).
- Behavioral regex tests on the TextMate grammar's own `begin`/`end`
  patterns (`node --test packages/vscode-outou/syntaxes/*.test.mjs`).

`examples/phase0-app/src/main.rsx`'s `<Greeting name="Outou" />` was
changed to `<Greeting name="Outou"></Greeting>` (self-closing ->
open/closing tag pair, semantically identical) specifically so
`rename-component`/`references-component` can exercise "both tags," not
just one occurrence; `crates/outou-backend-dioxus/tests/golden/main/`
was re-blessed accordingly (the only diff: the `Greeting` identifier
mapping gained its closing tag as a second source, exactly as ADR 0007
already models).
