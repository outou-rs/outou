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

- [x] `examples/phase0-app` builds with `cargo build` and nothing else, using the layout chosen in #2 — the `[[bin]]` target points straight at `src/.generated/crate-root.rs` (ADR 0009 layout (b)); no build script, no `OUT_DIR`, no extra Cargo flags or env vars. `.generated/` is gitignored for applications (`CONTRIBUTING.md`), so the one step before it, `outou build`, has to run once per `.rsx` change — documented in `examples/phase0-app/README.md` and the CI `example-app` job.
- [x] multi-file generation through the module graph — `crates/outou-cli/src/build/{plan,emit,clean}.rs`, driven by `outou_modules::ModuleGraph::generated_units()`.
- [x] `cargo xtask determinism`: generate every fixture through the build path and the language-server path, normalize, compare bytes and source maps; make the CI job required (remove `continue-on-error`) — `xtask/src/determinism.rs`; `outou-lsp` does not exist yet (issue #9), so the "language-server path" is a hand-written second call to `outou_syntax::parse` + `DioxusBackend::generate` per unit, documented as a `TODO(phase0, issue #9)` to be replaced once the real server exists.
- [x] generated code does not pollute the user's `cargo clippy`: local `allow`s on mechanical code only, most files carry no allow at all and a user expression keeps its own lints in exactly those files — `crates/outou-backend-dioxus`'s `GENERATED_LINT_ALLOWS` (`#![allow(unused_braces)]`) is emitted only when some attribute island in the file actually needed to keep its braces (a nested-JSX island prop, `docs/backend-leakage.md` rows 21 and 23; `bare_rust_expr` strips the braces everywhere else, so a real-world file with no such prop — `examples/phase0-app` included — carries no allow at all); `crates/outou-cli/tests/build.rs`'s `a_users_own_redundant_braces_still_fail_clippy_in_a_file_with_no_lint_allow` probes that a user's own `take({ x })` still fails `cargo clippy --all-targets -- -D warnings` in such a file.
- [x] no backend vocabulary (`rsx! macro`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode`) reaches the user in any build error of the example — `outou build`'s own diagnostics render through `outou_syntax::render`, never the backend's; `crates/outou-cli/tests/build_cli.rs`'s `cli_binary_exits_1_and_prints_only_outou_vocabulary_on_a_syntax_error` asserts it directly against the `outou` binary.

**Gate 2:** a multi-file `.rsx` crate does not build naturally as a Cargo project → STOP. **Not triggered**: `examples/phase0-app` and every required fixture (`mixed`, `cfg`, `cfg-duplicate`, `raw-ident`, `root-name`, `inline`) build through `outou build && cargo build` with no backend leakage in the build output — including `inline`, whose own inline-module `#[path]` rewrite was wrong until issue #8's review fixes (HIGH-1); `crates/outou-cli/tests/build_compile.rs`'s `generated_inline_fixture_passes_cargo_check` is the real-`rustc` probe that would have caught it, and now passes.

**Phase 0 limitation (`TODO(phase0)`):** a plain Rust (`.rs`) file may not declare a `.rsx` child module (`mod y;` in `src/x.rs` where `y.rsx` exists) — codegen never rewrites a `.rs` file's own source, so there is no way to point such a declaration at `y`'s generated output. `outou build` detects the shape and reports an Outou diagnostic (`crates/outou-cli/src/build/plan.rs`, `PlanError::RustDeclaresRsxChild`); see `crates/outou-cli/README.md` for the full explanation. This is a constraint of the generated-file layout (ADR 0009 (b)) and the `module_paths` API, not of the Dioxus backend, so it is not a `docs/backend-leakage.md` row.
