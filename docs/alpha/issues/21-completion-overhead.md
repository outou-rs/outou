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

- [x] Once A1's threshold is fixed, re-measure forwarded-completion overhead using A1's method.
- [x] If over threshold, profile `crate::response`'s leak-scanning/reverse-mapping path (`crates/outou-lsp/src/response.rs`) for the largest cost, and optimize it (e.g. avoid rescanning fields that cannot carry backend vocabulary, cap or short-circuit on candidate-list size).
- [ ] Re-measure after optimizing and confirm the result is within the A1 threshold. N/A: no optimization was needed (the threshold was already met), so there is nothing to re-measure.
- [x] Record before/after numbers in `docs/alpha-results.md`.
- [x] Pin down the trigger of the multi-second `large-enum-path` stall (a rust-analyzer-side event seen in direct sessions too) by reproducing it with `RA_LOG`/`--log-file` on a larger set of sessions (`docs/alpha-results.md`, A1 (d)). Done as far as a correlation: the stall consistently overlaps the initial cache priming; causation was not isolated.
- [ ] Decide whether `outou-lsp` should forward `experimental/serverStatus` or otherwise show readiness (today it forwards only `publishDiagnostics` and `$/progress`).
- [x] Judge any completion-path optimisation on **release** numbers: the debug-profile threshold is loose in release (~31x headroom at 1204 candidates, ~23x at 122) and would not catch a release regression.

Status (2026-10-24, details in `docs/alpha-results.md`, "A3: forwarded-completion overhead (issue #21)"):
the overhead passes the A1 threshold in debug and release (so the profile/optimise items found nothing to
fix: "if over threshold" did not apply and no after-measurement was needed); the stall is characterised
(the first completion that returns the final list consistently overlaps rust-analyzer's initial
cache priming and query computation, in 27 of 27 sessions sampled before quiescence; a correlation,
not an isolated cause). The `experimental/serverStatus` item is
left unchecked: a recommendation (do not forward it) is written, but the decision is the user's.
