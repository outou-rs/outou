---
title: "[Droppable] outou package: full publish automation"
milestone: "Phase 0"
week: "Week 7 if time allows"
gate: "—"
droppable: yes
labels: [phase0, droppable]
---

| | |
|---|---|
| **Gate** | none |
| **Droppable** | **Yes — cut fourth. The design (ADR 0008) is fixed; only the automation may slip** |

- [ ] `outou package` generates Rust and includes it in the published crate; consumers need only `cargo build`
- [ ] CI job: regenerate from `.rsx`, compare with the committed/packaged Rust, fail on difference
- [ ] `cargo publish --dry-run` on a library fixture written in `.rsx`
