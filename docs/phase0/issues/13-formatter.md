---
title: "[Droppable] Formatter: placeholder → rustfmt → splice"
milestone: "Phase 0"
week: "Week 7 if time allows"
gate: "—"
droppable: yes (cut first)
labels: [phase0, droppable]
---

| | |
|---|---|
| **Gate** | none |
| **Droppable** | **Yes — first to cut** |

- [ ] replace JSX regions with stable placeholders, run rustfmt, restore, then format the JSX
- [ ] `textDocument/formatting` uses the same pipeline
- [ ] no custom Rust formatter
