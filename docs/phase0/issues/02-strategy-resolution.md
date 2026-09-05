---
title: "[Week 2] Strategy resolution and ADR 0009"
milestone: "Phase 0"
week: "Week 2"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (follows Gate 0) |
| **Droppable** | No |

Using the spike results:

- [ ] fix the rust-analyzer strategy and the generated-source layout
- [ ] update `docs/adr/0009-generated-source-location.md` to **Accepted** with the chosen layout, or
- [ ] if Strategy A failed: spike **Strategy B** (shadow Cargo project under `.outou/lsp/`, reusing `cargo metadata`) and record it in `docs/ra-spike-results.md`
- [ ] remove the losing layout from the spike fixture and from `docs/phase0.md`
