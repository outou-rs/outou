---
title: "[A5] Typed key: evaluate key={expr} outside rsx!, IntoKey"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, cross-cutting]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Decided in ADR 0014. `key={expr}` is currently spliced into a Dioxus format string, so a static string (`key="new"`) and any expression containing `}` (`format!("edit-{id}")`) fail with backend wording (ledger rows 35 and 38, dogfood-board friction F1, F2).

- [ ] Codegen binds each key first (`let __outou_key_N = ::outou::__private::key(&(EXPR));`) and emits `key: "{__outou_key_N}"`. A static string key takes the same path. Document that the key is evaluated once, before the other attributes.
- [ ] A sealed trait `IntoKey` (strings and integers) in a public module (for example `outou::jsx`), with `#[diagnostic::on_unimplemented]` in Outou vocabulary. Confirm it builds on the MSRV (1.85).
- [ ] Regression tests: `key="new"`, `key={format!("edit-{id}")}`, `key={id}` and `key={String::from("x")}` compile. A key of an unsupported type gives the Outou message at the `.rsx` position. Update the goldens and `examples/phase0-app` output; run `cargo xtask determinism`.
- [ ] Check the LSP: hover, rename and references on a key expression, and that the synthesized `__outou_*` identifiers stay out of completion.
- [ ] Update the dogfood-board workarounds and Friction log (F1, F2). Append a ledger row stating that rows 35 and 38 are resolved.

**Acceptance:** the shapes of rows 35 and 38 compile and behave as keys; a wrong key type is diagnosed in Outou vocabulary; the backend `Key must not be a static string` messages can no longer be reached.
