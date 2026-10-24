---
title: "[A5] Close-out: ledger rows, Friction log, alpha-results A5 section"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, documentation]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Close-out of gate A5 after the implementation issues (ADR 0014).

- [x] Append new rows to `docs/backend-leakage.md` that resolve or wrap rows 31, 33, 34, 35, 37 and 38. Never rewrite old rows. Add a row for F5 (diagnostics) and its resolution. Restate the row 4 correction (`Element` is `Result<VNode, RenderError>` in dioxus-core 0.7.10).
- [x] Update every Friction log disposition in `examples/dogfood-board/README.md` (F1 to F6, F18 and so on) to match what the issues above shipped.
- [x] Write the A5 section of `docs/alpha-results.md`: the decision, the recount, the evidence and the signs that would reverse it. Fill the A2 corpus `TODO(alpha)` (false positives on rustc-rejected Rust: 33, none on valid Rust; record the re-run).
- [x] Tick the #23 checklist; mirror the new issues in `docs/alpha/issues/NN-*.md` and update `docs/alpha/issues/README.md`. (Mirrors added in `79e1693`; #23 ticked.)

**Acceptance:** the ledger, the Friction log and `docs/alpha-results.md` agree with the shipped code; `cargo test --workspace` and the four workspace checks pass; gate A5 can be closed under `docs/alpha.md`.

**Done** (2026-10-24). Ledger row 46 (the row 4 and 5 correction and the map of the A5 rows; rows 31, 33, 34, 35, 37 and 38 were already resolved by rows 41 to 44, and F5 by row 39); Friction log updated (F13, F15, F17, F18 and the status of the browser run); `docs/alpha-results.md` has the A5 section, the ledger recount (46 rows) and the A2 corpus figure (33 false positives, 0 on valid Rust, measured on 2026-10-24). Not claimed: any live-editor check, and the length limits and New task then Cancel of the browser walkthrough, are still open and are listed under "Not verified" in the A5 section. (Corrected 2026-10-24: this text first said the browser walkthrough of the migrated application was open; Fable checked it in a browser the same day, see the A5 section.)
