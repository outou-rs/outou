---
title: "[Week 4] outou-sourcemap: many-to-many maps and the registry"
milestone: "Phase 0"
week: "Week 4"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (required by Gate 3) |
| **Droppable** | No |
| **Crate** | `outou-sourcemap` |

- [ ] `SourceMap` with N:M entries (1 source → N generated, 1 generated → N source, zero sources for synthesized code)
- [ ] `Registry`: generated URI → `SourceMap` → original URIs, workspace-wide
- [ ] reverse-mapping rule: `.rsx` if one produced the location, otherwise the Rust location untouched
- [ ] serialization compatible with `spikes/rust-analyzer/source-map.json`
- [ ] tests: opening + closing tag → one identifier; one expression → several generated spans; dependency locations pass through
