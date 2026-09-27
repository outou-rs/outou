---
title: "[A5] Runtime decision: ADR superseding ADR 0003, and alpha-results.md"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, decision]
---

| | |
|---|---|
| **Gate** | A5 (final gate) |
| **Droppable** | No |

The runtime decision Phase 0 deferred on purpose (`docs/adr/0003-dioxus-as-temporary-backend.md`: "After the front end is proven, a runtime decision is made from that ledger: keep Dioxus, or build a native runtime"). This issue is the alpha's final gate; it does not start until A1–A4 have produced their evidence, or have been cut or recorded under `docs/alpha.md`'s "If a gate cannot be met".

- [ ] Weigh `docs/backend-leakage.md`'s full ledger against the criteria in its closing paragraph ("Rows are appended, never rewritten..."): how many rows make Outou semantics unnatural, weighed against interop freedom, renderer control, performance, bundle size, API stability and upgrade cost.
- [ ] Fold in the A4 dogfood evidence (friction log, new leakage rows).
- [ ] Fold in the A1 measurements (rust-analyzer resource cost, latency) as part of "performance" in the weighing above.
- [ ] Write a new ADR in `docs/adr/` recording the decision, explicitly marked as superseding ADR 0003 (`docs/adr/0003-dioxus-as-temporary-backend.md`'s Status becomes Superseded).
- [ ] If the decision changes the facade boundary, update ADR 0010 (`docs/adr/0010-facade-crate-as-backend-boundary.md`) to match.
- [ ] Write `docs/alpha-results.md`, covering at minimum: A1 measurements and thresholds, A2 recovery fixture results, A3 budget-miss evidence, A4 dogfood log and leakage additions, and this decision with its ADR.
- [ ] Add `docs/alpha-results.md` to `docs/README.md`'s table.
