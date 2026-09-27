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

- [x] `outou package` generates Rust and includes it in the published crate; consumers need only `cargo build`
- [x] CI job (`generated-drift`): regenerate from `.rsx` (`outou package --check`), compare with the committed Rust, fail on difference
- [x] `cargo publish --dry-run` on a library fixture written in `.rsx` (`outou_and_ui_kit_publish_dry_run_succeeds_together`, packaging `outou` alongside it — see `docs/adr/0008-pregenerated-publish-artifacts.md`)
