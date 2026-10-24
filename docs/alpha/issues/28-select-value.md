---
title: "[A5] <select value={V}>: select the matching option (F18)"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, cross-cutting]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Decided in ADR 0014. Dioxus applies a `<select value>` without selecting the matching option on mount, unlike React; the dogfood-board showed the wrong option until each `<option>` got a hand-written `selected` (ledger row 37, friction F18).

- [ ] Within a `<select value={V}>` subtree, rewrite each lexically contained `<option>` (including those inside `.map(|p| <option ..>)`). Evaluate V once outside the `rsx!` call as a `String`, then add `selected: option_selected(..)` (string comparison with the option's value). An explicit `selected` attribute on an option wins.
- [ ] Out of scope and documented (in `docs/grammar.md` or the lowering docs): options reached through variables, options returned by components, options without a `value` attribute, and `multiple`. Warn where this can be detected.
- [ ] Tests: codegen goldens, a headless render test for the `selected` flags, and the dogfood-board's `selected={option == current}` workarounds removed. Check that duplicate diagnostics do not appear because the option expression is emitted twice (many-to-many source map).
- [ ] Browser re-check (manual, F13): new-task form, edit form of a High task, and the remount case. Append a ledger row resolving row 37 within the stated limits.

**Acceptance:** `<select value={x}>` shows `x` on mount in a browser for the lexical cases; the limits are documented.
