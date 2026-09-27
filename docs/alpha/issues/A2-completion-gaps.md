---
title: "[A2] Completion: fix the <div class= sibling-swallowing gap"
milestone: "Alpha"
gate: A2
droppable: no
labels: [alpha, gate-a2, must-keep, lsp]
---

| | |
|---|---|
| **Gate** | A2 |
| **Droppable** | No |

`crates/outou-lsp/src/complete.rs` (`TODO(phase0)`, issue #9 Gate 3 review, "Recovery quality for `<div class=`" SKIP item): the recovery parser folds a following sibling element into a broken tag's own attributes (`<div class=` followed by `<Greeting name="Outou" />` recovers as `div { Greeting: true, name: "Outou" }`), so `classify` has nothing meaningful to find at that cursor and falls through to `Cursor::Expression`. `crate::response`'s cursor-containment filter (M4) currently keeps this from corrupting the buffer, but the completion itself is still missing.

- [ ] Decide and implement a fix for the underlying recovery shape (parser/codegen, `crates/outou-syntax`), so a sibling element after an unterminated attribute value is not folded into the broken tag's attribute list.
- [ ] Confirm `crates/outou-lsp/src/complete.rs`'s `classify` produces a meaningful `Cursor` for this position once the recovery shape is fixed, and update or remove the local classifier workaround accordingly.
- [ ] Add a regression test reproducing `<div class=` followed by a sibling element, asserting a correct (non-fallback) completion result.
- [ ] Remove the `TODO(phase0)` at `crates/outou-lsp/src/complete.rs:94` once fixed. The marker at :432 (the L15 re-parse) belongs to the triage issue.
