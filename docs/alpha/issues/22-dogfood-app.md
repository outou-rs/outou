---
title: "[A4] Dogfood application: build a real internal multi-module app in .rsx"
milestone: "Alpha"
gate: A4
droppable: yes
labels: [alpha, gate-a4, droppable, dogfood]
---

| | |
|---|---|
| **Gate** | A4 |
| **Droppable** | Yes (see `docs/alpha.md`, "What is cut first") |

`examples/phase0-app` was built to exercise Gate 4, not to be lived in. A4 builds something larger and interactive — an app that runs in a browser — and logs every friction point encountered while using Outou for real work.

- [ ] Design and build an internal, interactive multi-module app in `.rsx` that runs in a browser: forms, lists, several components split across files, and at least one workspace library crate the app depends on.
- [ ] It may re-export the temporary backend's existing state, event and launch APIs through `outou::prelude` (today's prelude publicly exports only `component` and `Element`, `crates/outou/src/lib.rs`). Record each such re-export under ADR 0010 and as a new row in `docs/backend-leakage.md`.
- [ ] Put the app under `examples/` as a non-workspace member, and ensure it builds with `outou build && cargo build` and passes CI: a job that runs `outou build` then `cargo build` and clippy on it, mirroring the `example-app` job (`.github/workflows/ci.yml`).
- [ ] Add a "Friction log" section to the app's README, logging every friction point encountered while building it — not only backend leakage.
- [ ] For each friction point that is backend-specific, append a new, numbered row to `docs/backend-leakage.md` (rows are appended, never rewritten), following its existing classification scheme.
- [ ] For each friction point, record a disposition: fixed during the alpha, deferred with an owning `TODO(alpha)`/issue, or accepted.
- [ ] Exercise the app's interactive flows with a documented manual walkthrough in the app's README (or a headless test, if one can be added without inventing new infrastructure).
- [ ] Summarize the app, the friction log and its dispositions in `docs/alpha-results.md`.
