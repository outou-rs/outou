# AGENTS.md

Guidance for AI coding agents working in this repository. This file is the single source of truth; `CLAUDE.md` only points here.

## What this project is

Outou compiles JSX written in standalone `.rsx` files to Rust. Phase 0, a feasibility spike, is complete (`docs/phase0-results.md`); the project is now in the **internal front-end alpha**. Read `docs/alpha.md` before planning any work: the order of work, the gates and what may be cut are fixed there.

The alpha's purpose is internal quality, not a public release, and its final gate is the runtime decision. Work that does not move the gates forward (a native runtime, Outou-owned hooks or signals, a router, production CSS, React interop) is out of scope. `docs/phase0.md` and `docs/phase0-results.md` remain as history: they record how the front end was proven feasible in the first place.

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo +1.85 check --workspace                 # MSRV
cargo xtask --help                            # corpus fetch/test, determinism, dist
```

The rust-analyzer spike is a separate Cargo project; run it from its own directory:

```bash
cd spikes/rust-analyzer/fixture
cargo check                             # src/.generated/ + #[path] (ADR 0009)
node ../client/ra-client.mjs --help
```

All four workspace checks must pass before a change is considered done. Do not weaken lints or add `allow` attributes to make them pass.

## Layout

| Path | Contents |
|---|---|
| `crates/outou` | Public facade. The only crate applications depend on. `prelude` is public API; `__private` is for generated code only. |
| `crates/outou-syntax` | Lexer, parser, AST, error recovery, Outou diagnostics. |
| `crates/outou-sourcemap` | Many-to-many source maps and the workspace registry. |
| `crates/outou-codegen` | `Backend` trait, `Strict` / `Recovery` modes. |
| `crates/outou-backend-dioxus` | Temporary backend. Emits text; does **not** depend on Dioxus. |
| `crates/outou-modules` | Module resolver for mixed `.rs` / `.rsx` crates. |
| `crates/outou-fmt` | Formatter: placeholder JSX regions, `rustfmt` for the Rust, then a JSX pretty printer. This is the only crate that spawns `rustfmt`; `outou-syntax` never shells out (`outou-lsp` separately spawns rust-analyzer, unrelated to formatting). |
| `crates/outou-lsp`, `crates/outou-cli` | Binaries `outou-lsp` and `outou`. |
| `xtask/` | Repository automation. |
| `spikes/rust-analyzer/` | Week 1 experiment. Not a workspace member. No framework code. |
| `tests/` | Shared fixtures and UI tests (data only, not a crate). |
| `examples/phase0-app/` | Phase 0's representative app in `.rsx`. Not a workspace member; CI builds it with `outou build` then `cargo build`. |
| `packages/` | npm packages. `vite-plugin-outou` is still a placeholder; `vscode-outou` has a working TextMate grammar and language configuration, standing in for semantic tokens until the extension is wired to `outou-lsp`. |
| `docs/` | Design, grammar, plan, backend leakage ledger, ADRs, issue mirrors (Phase 0 and alpha). |

## Rules

### Language and naming

- The syntax is "Outou JSX" or "JSX". Never call the syntax "RSX"; `.rsx` is only the file extension. See ADR 0002.
- Public documentation, code comments, commit messages and diagnostics are in English.
- `use outou::prelude::*` is the only import users write. Never show `use dioxus::prelude::*` in user-facing code, examples or docs.

### Backend boundary

- Generated code reaches the runtime only through `::outou::__private::*`. A user crate's `Cargo.toml` never lists the backend. See ADR 0010.
- Backend vocabulary (`rsx! macro`, `PropsBuilder`, `dioxus_rsx`, `GeneratedNode`, and similar) in any user-facing diagnostic is a bug. Translate or hide it.
- Any new constraint that comes from the backend gets a row in `docs/backend-leakage.md` with a classification. Rows are appended, not rewritten.
- `crates/outou` is the only crate allowed to depend on `dioxus`.

### Grammar and parser

- The only extension to Rust syntax is the JSX expression. Do not add other syntax. See ADR 0004 and `docs/grammar.md`.
- The parser never panics and always returns an AST. Every parser change ships with a fixture under `tests/fixtures/` or `tests/ui/`.
- Whitespace follows React/JSX. Each rule has a golden test paired with the equivalent React input.
- Outou diagnoses JSX structure itself, in Outou vocabulary, at the `.rsx` position.

### Modules, codegen, determinism

- Ambiguous modules are errors, never resolved by priority. `#[path]` is honored; `#[cfg]` is preserved, not evaluated. See ADR 0006.
- Source maps are many-to-many; a generated span may have zero or several sources. See ADR 0007.
- There is one compiler. `cargo build` and the language server must call the same front end and codegen; only the mode differs.
- Generated Rust is deterministic. Never edit files under `.generated/` by hand.

### Decisions and documents

- Architectural changes get an ADR in `docs/adr/` (Context / Decision / Consequences / Alternatives considered, one page).
- ADR 0009 (where generated Rust lives) is Accepted: layout (b), `src/.generated/` + `#[path]`, per the rust-analyzer spike in `docs/ra-spike-results.md`.
- Unresolved points are written as `TODO(alpha)` in the document where they belong, not silently decided. Existing `TODO(phase0)` markers stay valid until triaged per `docs/alpha.md`'s cross-cutting triage.
- Do not add competitor comparisons, popularity claims or project-continuation judgments to public documents.

### Scope discipline

- Follow the gate order in `docs/alpha.md`. Do not start a later gate to avoid a blocked earlier one, and do not implement items listed under "what is cut first" there before the gated work is done.
- Do not build a native runtime, hooks, signals, a router, SSR, HMR or React interop during the alpha. The alpha's job is to *decide* the runtime, not build one. Using the temporary backend's existing state and event APIs in A4 is not building them; any facade re-export this needs follows ADR 0010 and gets a `docs/backend-leakage.md` row.
- Alpha work items are GitHub issues under milestone "Alpha", mirrored in `docs/alpha/issues/`. Keep both in sync when scope changes.
- Phase 0 work items remain GitHub issues #1–#16 under milestone "Phase 0", mirrored in `docs/phase0/issues/`, kept as history now that Phase 0 is complete.

## Git

- Commit messages: `type: description`, with `type` one of `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.
- Do not push, tag, or force-push unless explicitly asked.
- `Cargo.lock` is committed. `spikes/rust-analyzer/fixture/Cargo.lock` is committed too.
- Generated Rust under `.generated/` is ignored for applications and committed for libraries and for the spike fixture.

## Verification before claiming completion

Run the four workspace checks and, if the spike fixture was touched, `cargo check` in `spikes/rust-analyzer/fixture`; if the spike client was touched, `node --test spikes/rust-analyzer/client/*.test.mjs`. Report failures verbatim; do not describe a change as working without having run them.
