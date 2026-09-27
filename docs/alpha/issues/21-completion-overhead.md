---
title: "[A3] Bring forwarded-completion overhead within the A1 threshold"
milestone: "Alpha"
gate: A3
droppable: yes
labels: [alpha, gate-a3, droppable, lsp]
---

| | |
|---|---|
| **Gate** | A3 |
| **Droppable** | Yes (see `docs/alpha.md`, "What is cut first") |

`docs/phase0-results.md` §6: forwarded-completion overhead (positions still forwarded to rust-analyzer, not answered locally) measured 10–25ms median, proportional to the forwarded candidate list's size, from leak-scanning and reverse-mapping every item. This was recorded as real and attributable, but not judged against any fixed threshold, since Phase 0 never fixed one. A1 fixes that threshold; this issue is where the overhead is brought under it.

- [ ] Once A1's threshold is fixed, re-measure forwarded-completion overhead using A1's method.
- [ ] If over threshold, profile `crate::response`'s leak-scanning/reverse-mapping path (`crates/outou-lsp/src/response.rs`) for the largest cost, and optimize it (e.g. avoid rescanning fields that cannot carry backend vocabulary, cap or short-circuit on candidate-list size).
- [ ] Re-measure after optimizing and confirm the result is within the A1 threshold.
- [ ] Record before/after numbers in `docs/alpha-results.md`.
