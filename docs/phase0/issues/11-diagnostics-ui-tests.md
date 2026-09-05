---
title: "[Week 6] UI test harness for Outou diagnostics and backend-vocabulary leak detection"
milestone: "Phase 0"
week: "Week 6"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (feeds Gate 4) |
| **Droppable** | No |

- [ ] harness for `tests/ui/<case>/{input.rsx, expected.stderr}` (path-normalized, exact match)
- [ ] three diagnostic layers exercised: Outou syntax, Rust semantic (mapped back), backend (translated)
- [ ] a test that fails if any user-facing output contains `rsx! macro`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode` or similar backend vocabulary
- [ ] every row in `docs/backend-leakage.md` that is a diagnostic has a UI case
