# 0005. rust-analyzer strategy priority

Status: Accepted

## Context

For `.rsx` files to get rust-analyzer features, generated Rust has to reach rust-analyzer's crate graph and the *editor buffer* has to be what it analyzes. There are three ways to arrange that, with very different amounts of Rust project model Outou would have to own.

## Decision

Try the strategies in this order and take the first one that satisfies the seven criteria in `docs/phase0.md`:

1. **Strategy A — Cargo-generated source plus editor overlay.** Cargo builds the crate graph as usual. The language server sends `didOpen`/`didChange` for the generated file with the current buffer. Outou owns nothing of the project model.
2. **Strategy B — shadow Cargo project.** The language server maintains a parallel Cargo project under `.outou/lsp/` whose sources are the generated Rust, reusing dependencies, features, edition and target from `cargo metadata`. `.outou/lsp/` exists only under this strategy.
3. **Strategy C — synthesized `rust-project.json`.** Outou reconstructs crate graph, dependencies, features, proc macros, sysroot and cfg itself.

The decision is made from the results of the hand-written spike, before any parser exists.

## Consequences

- If Strategy A works, the same generated file serves `cargo build` and the IDE; there is no separate IDE generation path.
- The earlier rule that IDE output is always separate from build output is withdrawn; the location depends on the strategy (see ADR 0009).
- Failure of all three is a Gate 0 stop.

## Alternatives considered

- **Strategy B first**, because it isolates the IDE from the build. It duplicates the project model and is more fragile with workspaces and proc macros; it stays the fallback.
- **A rust-analyzer fork or plugin.** Not maintainable by a small project.
