# Phase 0: feasibility

Phase 0 does not build a framework. It answers one question:

> Can `.rsx` feel like a first-class Rust file inside a real Rust project?

Or, more precisely: can JSX be introduced into a standalone `.rsx` file while keeping Rust's developer experience intact? The biggest risk is not the parser. It is whether integration with rust-analyzer works at a practical level. The order of work follows the risk.

Phase 0 is time-boxed to eight weeks of work. Extending the box to force a result is not an option; the point is to find out.

## Success criteria

Editing this file in an editor:

```rsx
use outou::prelude::*;

#[component]
fn App() -> Element {
    let user = load_user();

    <UserCard user={user} />
}
```

must give:

- go to definition on `load_user()`
- the type of `user` on hover
- Rust completion after `user.`
- component and prop completion
- Rust type errors reported at the right position in the `.rsx` file
- JSX syntax errors reported by Outou itself
- completion that keeps working while the file is half-typed
- go to definition across several `.rsx` files
- a build using `cargo build` and nothing else

Rendering in a browser is not a criterion.

## Order of work and gates

Work proceeds in this order. Each step ends at a gate; failing a gate stops Phase 0 and triggers the fallback described in [design.md](design.md).

| Step | Work | Gate |
|---|---|---|
| 1 | **rust-analyzer feasibility.** No parser. A hand-written `.rsx`, hand-written generated Rust and a hand-written source map, driven through a headless language-server client. | **Gate 0** — if even hand-written generated Rust cannot get rust-analyzer features through a proxy, stop. |
| 2 | **Strategy resolution and grammar.** Fix the rust-analyzer strategy (or spike the fallback strategy). Write the grammar: lexer modes, macro opacity, qualified paths, whitespace fixtures. | — |
| 3 | **Parser and recovery.** Mode-aware lexer, JSX AST, Rust expression islands, recovery nodes, incomplete input, JSX diagnostics. | **Gate 1** — if Rust + JSX cannot be parsed and recovered practically, stop. |
| 4 | **Codegen, modules, Cargo.** Dioxus backend, the `outou` facade, module graph, `#[path]`, `#[cfg]` preservation, multi-file generation, deterministic output, `cargo build`. | **Gate 2** — if a multi-file `.rsx` crate does not build naturally as a Cargo project, stop. |
| 5 | **Integrated language server.** Connect the real parser to the step-1 pipeline: completion, hover, definition, diagnostics, multi-file source maps, incomplete JSX. | **Gate 3** — if completion, hover, definition and diagnostics do not work through the real parser, stop. |
| 6 | **Cargo, tests, robustness.** `cargo build` / `check` / `test` / `clippy`, workspaces, nested modules, mixed `.rs`/`.rsx`, `#[cfg]`, `#[path]` modules, doc comments. | — |
| 7 | **Contingency.** Fix the serious problems found in steps 1–6. No new features. | — |
| 8 | **Integration and decision.** A realistic project; measure IDE quality, recovery, Cargo, modules, diagnostics, latency and backend leakage. Write `phase0-results.md`. | **Gate 4** — all gates passed: GO to a front-end alpha. |

## Step 1: the rust-analyzer spike

Everything in `spikes/rust-analyzer/` is hand-written: `App.rsx`, the generated Rust it would produce, a few source-map entries and a Cargo project. A small headless JSON-RPC client drives rust-analyzer directly; no editor extension is involved until late in Phase 0.

Strategy A is tried first, in two layouts:

- (a) `build.rs` generates into `OUT_DIR` and the crate `include!`s the result. The hashed `OUT_DIR` path is read from `cargo check --message-format=json` (`build-script-executed`).
- (b) the compiler writes `src/.generated/` and the crate references it with `#[path]`. To rust-analyzer it is an ordinary source file.

If (b) is sufficient, `OUT_DIR` is dropped: development and published crates then share one layout, and `outou package` only has to generate and include files.

Strategy A passes when all of the following hold:

1. rust-analyzer loads the Cargo project normally.
2. The generated source is part of the crate graph.
3. Editor changes reach rust-analyzer **without re-running `build.rs`**. This is the decisive item.
4. Completion reflects the latest buffer.
5. Hover reflects the latest buffer.
6. Definition reflects the latest buffer.
7. `cargo check` / flycheck diagnostics can be mapped back.

Results go in [ra-spike-results.md](ra-spike-results.md).

## rust-analyzer strategies, in priority order

1. **Strategy A — Cargo-generated source + editor overlay.** Cargo builds the crate graph; the language server only supplies the current buffer for the generated file. Dependencies, features, proc macros and the sysroot are never reconstructed by Outou.
2. **Strategy B — shadow Cargo project.** Only if A fails. The language server maintains a parallel Cargo project under `.outou/lsp/` whose sources are the generated Rust, reusing as much as possible from `cargo metadata`. Writing files there is not enough on its own; they must reach rust-analyzer's crate graph.
3. **Strategy C — synthesized `rust-project.json`.** Last resort, because Outou would then own the crate graph, dependencies, features, proc macros, sysroot and cfg.

Whichever strategy wins, there is **one compiler**: `cargo build` and the language server call the same front end and codegen. Output is deterministic and CI compares the build path and the language-server path on the same fixtures.

## What is cut first if time runs out

In this order:

1. Formatter
2. Rename / references
3. Semantic tokens (a TextMate grammar stands in)
4. Publish pipeline automation (the design is fixed; the automation can wait)

## What must not be cut

These decide GO/NO-GO and cannot be dropped to fit the time box:

1. rust-analyzer integration
2. completion while the input is incomplete or broken
3. multi-file module resolution
4. `cargo build` succeeding on its own
5. diagnostic and definition mapping through source maps
6. Outou syntax diagnostics

## Working principles

Build the thing most likely to fail first, not the thing easiest to build:

```text
unknown / existential risk        → rust-analyzer integration
parser ambiguity and recovery     → grammar and parser
Cargo and module integration      → codegen, modules, build
known framework engineering       → everything after Phase 0
```

A VDOM, hooks, a router and the rest come after, if at all.

## Performance budget

Measured in step 8, thresholds fixed afterwards. Provisional targets:

- incremental `.rsx` → generated Rust: perceived as instantaneous
- extra proxy overhead on completion: not perceptible
- a single-file edit never regenerates the whole crate

## Decision report

Step 8 produces `phase0-results.md` with at least: parser successes and failures, corpus failures, known ambiguities, recovery quality, module limitations, the Cargo workflow, an IDE feature matrix, latency measurements, diagnostic leakage, backend leakage and publish feasibility. The decision is made from that report, not from impressions.
