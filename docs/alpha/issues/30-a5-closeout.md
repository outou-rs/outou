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

- [ ] Append new rows to `docs/backend-leakage.md` that resolve or wrap rows 31, 33, 34, 35, 37 and 38. Never rewrite old rows. Add a row for F5 (diagnostics) and its resolution. Restate the row 4 correction (`Element` is `Result<VNode, RenderError>` in dioxus-core 0.7.10).
- [ ] Update every Friction log disposition in `examples/dogfood-board/README.md` (F1 to F6, F18 and so on) to match what the issues above shipped.
- [ ] Write the A5 section of `docs/alpha-results.md`: the decision, the recount, the evidence and the signs that would reverse it. Fill the A2 corpus `TODO(alpha)` (false positives on rustc-rejected Rust: 33, none on valid Rust; record the re-run).
- [ ] Tick the #23 checklist; mirror the new issues in `docs/alpha/issues/NN-*.md` and update `docs/alpha/issues/README.md`.

**Acceptance:** the ledger, the Friction log and `docs/alpha-results.md` agree with the shipped code; `cargo test --workspace` and the four workspace checks pass; gate A5 can be closed under `docs/alpha.md`.
