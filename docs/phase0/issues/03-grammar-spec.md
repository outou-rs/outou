---
title: "[Week 2] Grammar specification"
milestone: "Phase 0"
week: "Week 2"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (input to Gate 1) |
| **Droppable** | No |

Turn `docs/grammar.md` from a draft into the normative grammar. Every `TODO(phase0)` in it is resolved or explicitly deferred.

- [ ] lexer modes and transitions, including nested expression islands
- [ ] macro and attribute opacity
- [ ] the `<` rule: expression position, lookahead, qualified-path recognition, bounded speculative parse
- [ ] whitespace rules with one golden fixture per rule under `tests/fixtures/formatting/`, each paired with the React/JSX equivalent
- [ ] list of reserved syntax rejected in Phase 0
