---
title: "[A2] Recovery: swallowed tail and the two jsx/mod.rs recovery gaps"
milestone: "Alpha"
gate: A2
droppable: no
labels: [alpha, gate-a2, must-keep, parser]
---

| | |
|---|---|
| **Gate** | A2 |
| **Droppable** | No |

Four recovery gaps are on record from Phase 0 (`docs/alpha.md`, "A2: recovery, in detail"). This issue covers the three parser-side ones: the swallowed tail, `recover_fragment` and `recover_stray_close`. `A2-completion-gaps.md` covers the `<div class=` sibling-swallowing gap.

- [ ] Fix the swallowed-tail limitation: a body-less function whose parameter list never reaches a closing `)` can swallow following source before Outou's own symbol-preservation recovers (`crates/outou-backend-dioxus/README.md:30`, `LOW-17`).
- [ ] Make `bodyless-fn-mid-file` and `unclosed-island-mid-file` assert that `fn After` survives as its own item and that the truncated construct is diagnosed. Update their `.expected` files and the `recovery.rs` module doc that describes the limitation.
- [ ] Fix `Parser::recover_fragment` in `crates/outou-syntax/src/parser/jsx/mod.rs` (`TODO(phase0)`): a truncated `</` right after a reserved fragment is currently spliced into Recovery output as invalid Rust with no diagnostic pointing at the truncated `</` itself.
- [ ] Fix `Parser::recover_stray_close` in `crates/outou-syntax/src/parser/jsx/mod.rs` (`TODO(phase0)`): a top-level truncated `</div` at end of file currently loses `resolve_closing_tag`'s per-shape diagnostics and the partially-scanned tag name (`ClosingTagShape::name_text` returns `None` for every shape but `Named`).
- [ ] Add regression fixtures for both `recover_fragment` and `recover_stray_close` shapes under `tests/fixtures/incomplete/`.
- [ ] Remove the two `TODO(phase0)` markers once fixed.
- [ ] Verify every fixture under `tests/fixtures/incomplete/` passes both recovery tests, including the `--ignored` cargo-check one (`cargo test -p outou-backend-dioxus --test recovery -- --ignored`).
