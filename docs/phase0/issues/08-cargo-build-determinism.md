---
title: "[Week 4 / Gate 2] Generation + cargo build on its own + determinism check in CI"
milestone: "Phase 0"
week: "Week 4"
gate: "Gate 2"
droppable: no
labels: [phase0, gate-2, must-keep]
---

| | |
|---|---|
| **Gate** | 2 |
| **Droppable** | No |

- [ ] `examples/phase0-app` builds with `cargo build` and nothing else, using the layout chosen in #2
- [ ] multi-file generation through the module graph
- [ ] `cargo xtask determinism`: generate every fixture through the build path and the language-server path, normalize, compare bytes and source maps; make the CI job required (remove `continue-on-error`)
- [ ] generated code does not pollute the user's `cargo clippy`: local `allow`s on mechanical code only, user expressions keep their lints
- [ ] no backend vocabulary (`rsx! macro`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode`) reaches the user in any build error of the example

**Gate 2:** a multi-file `.rsx` crate does not build naturally as a Cargo project → STOP.
