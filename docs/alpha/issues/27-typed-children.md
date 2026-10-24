---
title: "[A5] Typed children: IntoChild and numeric children, bool is a compile error"
milestone: "Alpha"
gate: A5
droppable: no
labels: [alpha, gate-a5, must-keep, cross-cutting]
---

| | |
|---|---|
| **Gate** | A5 |
| **Droppable** | No |

Decided in ADR 0014. Child expressions are lowered as bare `{EXPR}`, so Dioxus's `IntoDynNode` decides what may be a child and numbers fail with a backend-vocabulary message (ledger rows 20 and 34, dogfood-board friction F3).

- [x] Codegen wraps every child island as `{::outou::__private::child(EXPR)}` (no extra braces for a single `;`-free expression, matching the existing `bare_rust_expr` rule).
- [x] A sealed `IntoChild<M>` (marker generic, as in the prototype) in the public module. It delegates to Dioxus's `IntoDynNode` and renders integers, floats and `char` as strings.
- [x] `bool` is a compile error. The `on_unimplemented` message points to `if` and `Option` for conditional children (decision recorded 2026-10-24).
- [x] Tests: `usize`, `f64`, `String`, `&str`, `Option<Element>`, iterators of keyed elements and `Element` still render (headless render test). A `bool` child gives the Outou message at the `.rsx` position. Backend vocabulary check on the output. Update `tests/ui/backend-non-into-dyn-node-child` (it becomes an Outou diagnostic) and the vocabulary rewrite rule for the messages that remain.
- [x] Update the F3 workaround in the dogfood-board; append a ledger row resolving rows 34 and (in part) 20.

**Acceptance:** numeric children render without `.to_string()`; a `bool` child fails with an Outou-vocabulary message; nothing names `IntoDynNode` to the user.

**Done** (2026-10-24). Notes: the tests live in `crates/outou/tests/jsx_child.rs` (headless render), `crates/outou-backend-dioxus/tests/child_compile.rs` and `tests/golden/children` (codegen), `tests/ui/jsx-child-unsupported-type` (renamed from `backend-non-into-dyn-node-child`, which could no longer fail: `i32` is a child now) and `crates/outou-cli/tests/cargo_cli_real.rs` (`outou cargo` with a real compiler). The two helps rustc adds under the error are dropped by `outou cargo` (`crates/outou-cli/README.md`, rule 11). Ledger row 42. Known gap: a number nested in an `Option` or a collection is not a child.
