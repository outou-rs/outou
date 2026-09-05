# outou-lsp

Language server for `.rsx` files (language id `outou-rsx`). Proxies
rust-analyzer and maps positions/locations through `outou-sourcemap`'s
`Registry` in both directions, using the exact same parser and codegen
`outou build` uses ("there is one compiler", `AGENTS.md`) — only the mode
differs (`Mode::Recovery`, so the rest of a half-typed file stays
analyzable).

Phase 0: Week 5 (Gate 3) — **PASS**. Full write-up, evidence and known
limitations: [`docs/gate3-results.md`](../../docs/gate3-results.md).

## Running it

```bash
cargo build -p outou-lsp
target/debug/outou-lsp
```

An editor connects over stdio and sends the usual LSP `initialize`
handshake with `workspaceFolders` pointing at the crate root. On startup
this server:

1. Resolves the crate's `.rsx` root (`src/main.rsx`/`lib.rsx`); if none
   exists, it runs in a **degraded mode** that only publishes Outou syntax
   diagnostics (no rust-analyzer is spawned).
2. Otherwise, plans the whole crate (`outou_cli::build::plan`) and
   generates every `.rsx` unit in `Mode::Recovery`.
3. Writes any *missing* generated file to disk once (so Cargo's own
   `[[bin]]`/`[lib]` target exists even before `outou build` has run; an
   existing, possibly stale file is left alone).
4. Spawns rust-analyzer (`OUTOU_RUST_ANALYZER` env var, else the first
   `rust-analyzer` on `PATH`; the process fails fast with an
   Outou-vocabulary message if neither resolves, before ever starting the
   LSP handshake) with the same root, forwarding the editor's own
   capabilities (minus `linkSupport`, so `textDocument/definition`
   responses are always plain `Location`s) and `initializationOptions`,
   plus `checkOnSave: true` and `cargo.buildScripts.enable: true`.
   `didOpen`s every generated unit's current text as rust-analyzer's
   overlay (Strategy A, ADR 0005).

After that, `textDocument/{didOpen,didChange}` for a `.rsx` file
regenerates only that unit (re-planning the whole crate only when the
edited file's own set of `mod` declarations changes) and forwards a
`didChange` to rust-analyzer for its generated file;
`textDocument/{hover,completion,definition}` map the position to the
generated file, forward the request, and map the response back;
`textDocument/publishDiagnostics` from rust-analyzer is mapped back and
merged with Outou's own syntax diagnostics; `textDocument/didSave` writes
the current generated text to disk and forwards the save, so
rust-analyzer's `checkOnSave` flycheck — the only mechanism that reports
semantic (type-mismatch) errors, per the Week 1 spike — has something
fresh to check. Everything else is forwarded to rust-analyzer
transparently.

## Layout

| File | Contents |
|---|---|
| `src/main.rs` | Arg parsing (`--version`), resolves the rust-analyzer binary, fails fast if it cannot be found. |
| `src/server.rs` | The main loop: `initialize`, crate planning, rust-analyzer setup, the client/rust-analyzer select loop. |
| `src/dispatch.rs` | Per-message handling once the handshake is done: request/response mapping, notification handling, single-unit regeneration and re-planning. |
| `src/ra.rs` | Spawns rust-analyzer and speaks JSON-RPC to it (reuses `lsp_server::Message`'s own framing for that side too). |
| `src/documents.rs` | In-memory state: `.rsx` documents, generated units, the registry. |
| `src/plan.rs` | Thin wrapper over `outou_cli::build::plan` (degraded-mode detection, module-declaration-change detection). |
| `src/mapping.rs` | Position/range translation between `.rsx` and generated Rust. |
| `src/response.rs` | Rewrites hover/definition/completion response payloads through `mapping.rs`. |
| `src/diagnostics.rs` | Merges Outou syntax diagnostics with mapped rust-analyzer/flycheck diagnostics. |
| `src/translate.rs` | Backend-vocabulary translation table for diagnostic messages (`docs/backend-leakage.md` row 24). |
| `src/uri.rs` | Conversions between `outou_sourcemap::Uri` and `lsp_types::Uri`. |
| `tests/gate3.rs` | End-to-end Gate 3 test, driving `spikes/rust-analyzer/client/outou-lsp-client.mjs`. Ignored by default (~2 minutes); run explicitly: `cargo test -p outou-lsp --test gate3 -- --ignored --nocapture`. |

## Known limitations

See [`docs/gate3-results.md`](../../docs/gate3-results.md)'s own section:
no `$/progress` forwarding to the editor, semantic diagnostics need a save
(a rust-analyzer limitation, not this server's), and an unclosed
`<Component attr` does not reach rust-analyzer as a props-builder
completion context (a well-formed tag with a partially-typed attribute
*value* does).
