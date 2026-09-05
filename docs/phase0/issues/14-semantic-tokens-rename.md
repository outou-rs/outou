---
title: "[Droppable] Semantic tokens and rename / references"
milestone: "Phase 0"
week: "Week 7 if time allows"
gate: "—"
droppable: yes
labels: [phase0, droppable]
---

| | |
|---|---|
| **Gate** | none |
| **Droppable** | **Yes — semantic tokens are cut third (a TextMate grammar stands in), rename/references second** |

- [ ] TextMate grammar for `outou-rsx` in `packages/vscode-outou/` (the stand-in)
- [ ] semantic tokens: Rust keyword/type/variable from rust-analyzer mapped back, plus component, HTML element, attribute, event, JSX text
- [ ] rename updates both the opening and the closing tag through the N:M source map
- [ ] references
