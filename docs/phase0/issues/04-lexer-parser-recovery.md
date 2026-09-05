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

- [x] mode-aware lexer (`Rust` / `JsxTag` / `JsxText`) driven directly by the recovering parser (`crate::parser`), opaque macro and attribute token trees — the free-standing, context-free `tokenize` tried early in Phase 0 was removed (decision D5): nothing called it, and it could not perform the parser's own closing-tag matching, so it inevitably drifted from the parser's real decisions
- [x] JSX AST with Rust expression islands (`ast::Island`, exactly partitioning their content — no JSX node inside an island is ever collapsed away, H6); text and expressions as separate child nodes
- [x] recovery nodes: `ErrorNode`, `MissingToken`, `IncompleteTag`, and `JsxElement::errors` for a stray byte skipped inside a tag (M8)
- [x] recovery for `tests/fixtures/incomplete/`: `<div cl`, `<User name={`, unclosed nested tag — an AST is produced and parsing continues after the broken region
- [x] Outou diagnostics in Outou vocabulary, e.g. `closing tag `</span>` does not match opening tag `<div>`` (`tests/fixtures/diagnostics/`), each at the exact span grammar §9 specifies (missing-closing-tag diagnostics for ancestors report at each ancestor's own opening tag, not the closing tag that revealed the mismatch, M4)
- [x] `outou_syntax::parse` never panics **on any input**, not just the corpus fixtures — grammar §9's actual requirement: verified by `tests/never_panics.rs` (fixtures, every byte-boundary prefix of every fixture, adversarial inputs, the H1 trailing-backslash regression, and the H7 nesting-depth regression) and by a 46-symbol-alphabet, fixed-seed random-string fuzz test in the same crate asserting no panic and every span within bounds over tens of thousands of generated inputs

**Gate 1:** Rust + JSX cannot be parsed and recovered practically → STOP.
