# phase0-app

The representative Phase 0 application. It exercises the parser, module resolution, component lowering, props, Rust expression islands, diagnostics, the IDE and `cargo build` at once.

## Building

`src/.generated/` is gitignored for applications (see `CONTRIBUTING.md`), so it does not exist in a fresh checkout. Generate it once with `outou build`, then use `cargo` normally:

```bash
cargo run -p outou-cli -- build --manifest-dir examples/phase0-app
cd examples/phase0-app
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

`outou build` resolves the module graph starting at `src/main.rsx`, generates `src/.generated/crate-root.rs` and one file per module (`src/.generated/components.rs` for `mod components;`), and writes a `<name>.rs.map.json` source map next to each. The package's `[[bin]]` target points straight at `src/.generated/crate-root.rs` (ADR 0009, layout (b)): there is no build script, and once `outou build` has run, `cargo build` alone builds the crate — the workflow the Gate 2 success criterion asks for is `outou build && cargo build`, run once per change to a `.rsx` file.

Re-run `outou build` whenever a `.rsx` file under `src/` changes; it is idempotent and removes any generated file that is no longer produced (a stale module's leftover `.rs`/`.rs.map.json`).
