---
title: "[Week 6] cargo check / test / clippy, workspaces, nested modules, cfg, doc comments"
milestone: "Phase 0"
week: "Week 6"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (feeds Gate 4) |
| **Droppable** | No |

- [x] `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, `cargo publish --dry-run` on the example and fixtures — `crates/outou-cli/tests/matrix.rs`. `cargo package --list -p ui-kit` ships `src/.generated/` without any `[package] include` (the repository's git-aware default file list plus the root `.gitignore`'s negation for this path already cover it); `cargo publish --dry-run` genuinely fails for the library fixture (`ui-kit`) for an unrelated reason — its `outou` dependency is an unpublished local `path` dependency, a registry-level constraint every crate with an unpublished `path` dependency hits, not a dot-directory or JSX-specific one — and cannot even run for the example app (a `[[bin]]`-only crate never publishes); both are recorded findings, not gaps — see `crates/outou-cli/README.md`'s "The Cargo command matrix" section.
- [x] `#[cfg(test)] mod tests` inside `.rsx` runs under `cargo test` — already true for `examples/phase0-app/src/components.rsx` before this issue; `matrix.rs`'s `example_app_cargo_matrix` now asserts the test name appears in `cargo test`'s own output. `tests/fixtures/workspace/ui-kit`'s own `#[cfg(test)] mod tests` is asserted the same way for a library crate.
- [x] plain Rust doc tests in `.rsx` files still run; doc tests containing JSX are not required — true for a crate with a `[lib]` target (`tests/fixtures/workspace/ui-kit`'s `add_one`, asserted running under `cargo test --workspace`/`cargo test --doc -p ui-kit`); **not** true for a `[[bin]]`-only crate at all, regardless of JSX — a genuine Cargo-level finding, not an Outou gap, recorded in `crates/outou-cli/README.md` and exercised as `example_app_doc_tests_cannot_run_because_there_is_no_library_target`.
- [x] workspace with several crates, a dependency crate written in `.rsx`, nested modules, `#[cfg]`-gated and `#[path]` modules — `tests/fixtures/workspace/{ui-kit,app}`: `ui-kit` is a `.rsx` library (nested module `widgets` + `widgets/button`, a `#[cfg(feature = "extra")]` module, a `#[path = "shapes/circle.rsx"]` module, a plain `.rs` sibling), depended on by path from the `app` binary. `outou build --manifest-dir <workspace root>` now builds every member with an `.rsx` crate root in one pass (`crates/outou-cli/src/build/workspace.rs`, reading `[workspace] members`); `cargo build/test/clippy --workspace` and `cargo build -p ui-kit --features extra` all pass. `ui-kit`'s generated Rust is committed (ADR 0008) and its regeneration is checked byte-identical (`matrix.rs`) and added to `cargo xtask determinism`'s target list.
