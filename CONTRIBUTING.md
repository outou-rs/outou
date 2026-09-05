# Contributing

Outou is in a feasibility phase. The most useful contributions right now are measurements, failing cases and review of the design documents, not features.

## Setup

```bash
rustup show                     # rust-toolchain.toml selects stable + rustfmt + clippy + rust-analyzer
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`cargo deny check` runs in CI; install `cargo-deny` to run it locally.

## Repository layout

- `crates/` — the workspace: `outou` (public facade), `outou-syntax`, `outou-sourcemap`, `outou-codegen`, `outou-backend-dioxus`, `outou-modules`, `outou-build`, `outou-lsp`, `outou-cli`
- `spikes/` — standalone experiments; not workspace members
- `tests/` — shared fixtures and UI tests
- `examples/` — example applications
- `packages/` — npm packages (Vite plugin, VS Code extension)
- `xtask/` — `cargo xtask corpus fetch`, `cargo xtask determinism`, `cargo xtask dist`
- `docs/` — design, grammar, plan, decisions

## Generated Rust

Generated Rust under `.generated/` directories is ignored by git for **applications**. **Libraries** that publish pre-generated Rust commit it, and CI verifies that regenerating from `.rsx` yields the same files. Never edit generated files by hand.

## Syntax corpora

Large external corpora are not vendored. `corpus.lock` pins them by repository and commit; `cargo xtask corpus fetch` clones them into `.corpus/`.

## Pull requests

- One topic per pull request.
- Every parser change comes with a fixture under `tests/fixtures/` or `tests/ui/`.
- Anything that adds a constraint from the backend gets a row in `docs/backend-leakage.md`.
- Architectural changes get an ADR in `docs/adr/`.
- Commit messages: `type: description` with `type` one of feat, fix, refactor, docs, test, chore, perf, ci.

## Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
