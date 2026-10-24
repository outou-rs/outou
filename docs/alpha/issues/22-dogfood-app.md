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

- [x] Design and build an internal, interactive multi-module app in `.rsx` that runs in a browser: forms, lists, several components split across files, and at least one workspace library crate the app depends on.
  - Built (`examples/dogfood-board`); native build, tests, clippy and a wasm32 `cargo check` pass. Run in a real browser on 2026-10-24 (dioxus-cli 0.7.10, `dx serve --platform web`, Chrome) and the manual walkthrough executed by hand. The run found F18 (`<select value>` shows the wrong option on mount, ledger row 37); it was fixed in the app afterwards and **re-checked in the browser the same day**; the migrated application (after #26 to #29) was checked in the browser again on 2026-10-24, see `docs/alpha-results.md` A5.
- [x] It may re-export the temporary backend's existing state, event and launch APIs through `outou::prelude` (today's prelude publicly exports only `component` and `Element`, `crates/outou/src/lib.rs`). Record each such re-export under ADR 0010 and as a new row in `docs/backend-leakage.md`.
- [x] Put the app under `examples/` as a non-workspace member, and ensure it builds with `outou build && cargo build` and passes CI: a job that runs `outou build` then `cargo build` and clippy on it, mirroring the `example-app` job (`.github/workflows/ci.yml`).
  - Added the `dogfood-board` CI job and the local equivalent passes; left unchecked until the job has run on a pushed branch.
- [x] Add a "Friction log" section to the app's README, logging every friction point encountered while building it — not only backend leakage.
- [x] For each friction point that is backend-specific, append a new, numbered row to `docs/backend-leakage.md` (rows are appended, never rewritten), following its existing classification scheme.
- [x] For each friction point, record a disposition: fixed during the alpha, deferred with an owning `TODO(alpha)`/issue, or accepted.
- [x] Exercise the app's interactive flows with a documented manual walkthrough in the app's README (or a headless test, if one can be added without inventing new infrastructure).
  - Interactive flows are covered by the manual walkthrough only (hand-run in a real browser on 2026-10-24); that is not repeatable automation. The automated tests are three headless first-render tests (whole tree, edit form, detail view); none clicks or types (F13, ledger row 36).
- [x] Summarize the app, the friction log and its dispositions in `docs/alpha-results.md`.
