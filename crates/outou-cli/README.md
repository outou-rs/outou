# outou-cli

The `outou` command: `build`, `check` and `package`. `outou package` generates Rust and includes it in the published crate so that consumers need only `cargo build`.

Phase 0: Weeks 4–6.

The binary (`src/main.rs`) is a thin argument-parsing wrapper. The actual `build` pipeline is a library (`src/lib.rs`, `pub mod build`), so `cargo xtask determinism` and this crate's own tests can call it directly without spawning the `outou` process — "there is one compiler" (`AGENTS.md`).

## `outou build`

```
outou build [--manifest-dir DIR] [--mode strict|recovery]
```

Resolves the module graph at `DIR` (default: the current directory) with `outou-modules`, generates Rust for every `ModuleGraph::generated_units()` node with `outou-backend-dioxus`, and writes each one under `src/.generated/` per ADR 0009 layout (b), plus a `<name>.rs.map.json` source map next to it (the spike-compatible JSON form, `SourceMap::to_json`; `generated`/`sources` are `file://` + absolute path — `cargo xtask determinism` normalizes this prefix away before comparing). Files are written atomically (temporary file, then rename), and any managed file (`*.rs`, `*.rs.map.json`) left over from a module that was renamed or removed is deleted; anything else under `src/.generated/` is left alone.

- No `.rsx` crate root (`src/main.rsx`/`src/lib.rsx`) — including a crate with only a plain `.rs` root — prints a message and exits `0`: there is nothing to do.
- Both a `.rsx` root and its `.rs` counterpart existing side by side is an error.
- Mode defaults to `strict`, the only mode that ever reaches `cargo build`; any Outou syntax error is a build failure, reported with Outou's own rendered diagnostics (`outou_syntax::render`) — never backend vocabulary (`rsx!`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode`, `dioxus`).

**Phase 0 limitation (`TODO(phase0)`, issue #8):** a plain Rust (`.rs`) file may not declare a `.rsx` child module (`mod y;` in `src/x.rs`, where `y.rsx` exists). Codegen only ever rewrites a `mod` declaration's `#[path]` inside a *generated* file (a `.rsx` file's own output); it never touches a `.rs` file's source, so there is no way to point such a declaration at `y`'s generated output. `outou build` detects this shape while planning (a `SourceKind::Rust` module graph node with a non-inline `SourceKind::Rsx` child) and reports it as an Outou diagnostic: `` module `y` is declared from the plain Rust file `src/x.rs`; Phase 0 requires `.rsx` modules to be declared from `.rsx` files or from the crate root ``. `.rsx` modules must be declared from another `.rsx` file, or from the crate root itself. `tests/fixtures/modules/{rs-to-rsx,path-attr}` (built for `outou-modules`' resolver) both contain this exact shape and are the fixtures `crates/outou-cli/src/build/plan.rs`'s own tests check the error against; they are intentionally excluded from the build-pipeline integration suite (`tests/build.rs`), which only builds shapes Phase 0 supports.

## `outou check`

Parses every `.rsx` file under the current crate's `src/` directory (recursively, in sorted order — a dot-directory such as `src/.generated/` is never descended into, and a symlink is never followed) with `outou_syntax::parse` — the same front end `build`'s generation step and the language server use — and prints Outou's own rendered diagnostics for any file that has one: `error: <message>` for an error-severity diagnostic, `warning: <message>` for a warning-severity one, each followed by ` --> <path>:<line>:<col>`. Exits `1` if any file has an error-severity diagnostic, `0` otherwise (warnings alone do not fail the check). `package` remains `todo!()`; it is Week 6's work.

## Tests

- `tests/build.rs`: builds every required build-pipeline fixture (`tests/fixtures/modules/{mixed,cfg,cfg-duplicate,raw-ident,root-name,inline}`) and `examples/phase0-app` through `outou_cli::build::build`, asserting the exact generated file set, `#[path]`/`#[cfg]` fidelity, stale-file cleanup, atomic-write idempotency, and (`#[ignore]`d — a cold `dioxus` build) that the result actually passes `cargo check`/`cargo clippy`, including a probe that a user's own expression keeps its own lints alongside generated code's file-level `#![allow(unused_braces)]` (`docs/backend-leakage.md` row 21). Also asserts the `outou` binary itself exits `1` and prints only Outou vocabulary for a `.rsx` syntax error.
