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

- [ ] `cargo build`, `cargo check`, `cargo test`, `cargo clippy`, `cargo publish --dry-run` on the example and fixtures
- [ ] `#[cfg(test)] mod tests` inside `.rsx` runs under `cargo test`
- [ ] plain Rust doc tests in `.rsx` files still run; doc tests containing JSX are not required
- [ ] workspace with several crates, a dependency crate written in `.rsx`, nested modules, `#[cfg]`-gated and `#[path]` modules
