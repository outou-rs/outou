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

- [x] replace JSX regions with stable placeholders, run rustfmt, restore, then format the JSX (`crates/outou-fmt`, `docs/adr/0011-formatter-placeholder-rustfmt-splice.md`)
- [x] `textDocument/formatting` uses the same pipeline (`crates/outou-lsp/src/dispatch/requests.rs`, `dispatch_formatting_request`)
- [x] no custom Rust formatter (`crates/outou-fmt/src/rustfmt_proc.rs` shells out to `rustfmt`)
