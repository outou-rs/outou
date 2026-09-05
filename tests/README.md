# tests

Shared test data. Nothing here is a Cargo crate.

- `fixtures/modules/` — mixed `.rs` / `.rsx` module trees for the module resolver and multi-file codegen.
- `fixtures/diagnostics/` — inputs paired with the exact Outou diagnostic they must produce.
- `fixtures/formatting/` — JSX whitespace golden tests, each paired with the equivalent React/JSX input and its expected output.
- `fixtures/incomplete/` — inputs a user has half-typed. The parser must still produce an AST and recovery-mode codegen must still produce Rust.
- `ui/` — `input.rsx` + expected `.stderr`, in the style of rustc UI tests; exercised by `crates/outou-cli/tests/{ui,leak}.rs` (issue #11) — see `ui/README.md` for the case format and the `BLESS=1` workflow.
