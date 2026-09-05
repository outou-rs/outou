## What

<!-- One paragraph. What changes and why. -->

## Checklist

- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` pass
- [ ] Parser or codegen changes come with a fixture under `tests/`
- [ ] New backend constraints are recorded in `docs/backend-leakage.md`
- [ ] Architectural changes have an ADR in `docs/adr/`
- [ ] No backend vocabulary in any user-facing diagnostic
