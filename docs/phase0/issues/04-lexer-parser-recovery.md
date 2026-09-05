---
title: "[Week 3 / Gate 1] Mode-aware lexer, parser, error recovery and JSX diagnostics"
milestone: "Phase 0"
week: "Week 3"
gate: "Gate 1"
droppable: no
labels: [phase0, gate-1, must-keep]
---

| | |
|---|---|
| **Gate** | 1 |
| **Droppable** | No |
| **Crate** | `outou-syntax` |

- [ ] mode-aware lexer (`Rust` / `JsxTag` / `JsxText`), opaque macro and attribute token trees
- [ ] JSX AST with Rust expression islands; text and expressions as separate child nodes
- [ ] recovery nodes: `ErrorNode`, `MissingToken`, `IncompleteTag`
- [ ] recovery for `tests/fixtures/incomplete/`: `<div cl`, `<User name={`, unclosed nested tag — an AST is produced and parsing continues after the broken region
- [ ] Outou diagnostics in Outou vocabulary, e.g. `closing tag `</span>` does not match opening tag `<div>`` (`tests/fixtures/diagnostics/`)
- [ ] `outou_syntax::parse` never panics on any input from the corpus fixtures

**Gate 1:** Rust + JSX cannot be parsed and recovered practically → STOP.
