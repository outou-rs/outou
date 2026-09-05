# 0009. Where generated Rust lives

Status: Accepted

## Context

Under Strategy A (ADR 0005) the same generated Rust serves `cargo build` and rust-analyzer. Two layouts were possible, and the choice affects the build script, publishing, and how rust-analyzer sees the file.

## Options

### (a) `OUT_DIR` + `include!`

A `build.rs` (`outou-build`) generates into `$OUT_DIR/outou/` and the crate includes it. This is Cargo's convention for generated code. The hashed `OUT_DIR` path must be read from `cargo check --message-format=json` (`build-script-executed`), and rust-analyzer has to run build scripts to know it. `build.rs` never writes into `src/`.

### (b) fixed path `src/.generated/` + `#[path]`

The compiler writes `src/.generated/<module>.rs` and the module declaration points at it with `#[path]`. No build script. To rust-analyzer it is an ordinary source file. The development layout and the published layout (ADR 0008) are identical, so `outou package` only generates and includes.

## Decision

Layout (b), fixed path `src/.generated/` + `#[path]`, is adopted. In the Week 1 spike, (b) satisfied all seven Strategy A criteria; (a) failed criterion 4 (completion), specifically inside the generated `rsx!` macro call and under incomplete input — exactly the shape of Outou's everyday generated code, and one of Phase 0's non-droppable items. Full evidence in [`docs/ra-spike-results.md`](../ra-spike-results.md). Consequently, `OUT_DIR` generation is dropped along with `outou-build`.

## Consequences

- Exactly one layout ships. `OUT_DIR` generation, `crates/outou-build`, and the `build.rs` step are removed, not kept as an option.
- Development and published crates share one layout (ADR 0008): `outou package` only has to generate `.generated/` files and include them, with no separate build-time generation path.
- `.outou/lsp/` is not involved; it exists only for Strategy B, which was not needed.

## Alternatives considered

- **Keep IDE output separate from build output regardless of strategy.** Withdrawn: it would mean two generation paths and a second source of staleness.
- **Layout (a), `OUT_DIR` + `include!`.** Rejected: fails completion inside the generated macro call and under incomplete input (see Decision above).
