# 0009. Where generated Rust lives

Status: **Proposed** (decided after the rust-analyzer spike)

## Context

Under Strategy A (ADR 0005) the same generated Rust serves `cargo build` and rust-analyzer. Two layouts are possible, and the choice affects the build script, publishing, and how rust-analyzer sees the file.

## Options

### (a) `OUT_DIR` + `include!`

A `build.rs` (`outou-build`) generates into `$OUT_DIR/outou/` and the crate includes it. This is Cargo's convention for generated code. The hashed `OUT_DIR` path must be read from `cargo check --message-format=json` (`build-script-executed`), and rust-analyzer has to run build scripts to know it. `build.rs` never writes into `src/`.

### (b) fixed path `src/.generated/` + `#[path]`

The compiler writes `src/.generated/<module>.rs` and the module declaration points at it with `#[path]`. No build script. To rust-analyzer it is an ordinary source file. The development layout and the published layout (ADR 0008) are identical, so `outou package` only generates and includes.

## Decision

Deferred. Both layouts are exercised in the spike. If (b) satisfies all seven Strategy A criteria, (b) is adopted and `OUT_DIR` generation is dropped along with `outou-build`. If only (a) works, `outou-build` becomes real and publishing carries the generated files separately.

## Consequences (either way)

- Exactly one layout ships; the other is removed, not kept as an option.
- `.outou/lsp/` is not involved; it exists only for Strategy B.

## Alternatives considered

- **Keep IDE output separate from build output regardless of strategy.** Withdrawn: it would mean two generation paths and a second source of staleness.
