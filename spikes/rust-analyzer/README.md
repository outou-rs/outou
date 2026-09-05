# rust-analyzer feasibility spike

The first thing Outou has to prove is not that JSX can be parsed, but that a `.rsx` file can get real rust-analyzer features **without** Outou re-implementing Rust's project model. This directory is a self-contained experiment for that. It contains no framework code and is not a member of the workspace.

## What is here

```text
fixture/                 Cargo project rust-analyzer loads
  Cargo.toml             depends on the backend directly; two cargo features select the layout
  build.rs               variant (a) only: copies virtual/App.rs into OUT_DIR/outou/App.rs
  src/main.rs            load_user(), UserCard, and the module switch between (a) and (b)
  src/App.rsx            the "source" the user would edit
  src/.generated/App.rs  hand-written generated Rust for variant (b) (committed on purpose)
virtual/App.rs           hand-written generated Rust, the single source for both variants
source-map.json          hand-written many-to-many map between App.rsx and the generated Rust
client/ra-client.mjs     headless JSON-RPC client that drives rust-analyzer
client/source-map.mjs    pure `mapRange()` used to map a generated-file range
                         back to `App.rsx` spans through source-map.json
client/source-map.test.mjs  `node --test` coverage for source-map.mjs
```

There is no parser. `virtual/App.rs` is what the compiler *would* emit for `src/App.rsx`, written by hand.

## Setup

```bash
rustup component add rust-analyzer      # or point --ra at any rust-analyzer binary
cd spikes/rust-analyzer/fixture
cargo check                             # variant (b), the default
cargo check --no-default-features --features gen-outdir   # variant (a)
```

## Strategy A, two layouts

Strategy A means: let Cargo build the crate graph, and have the language server supply *editor overlay* content for the generated file. Two layouts for that generated file are tried.

### (b) fixed path `src/.generated/App.rs` (default feature `gen-src`)

The generated file is an ordinary source file referenced with `#[path = ".generated/App.rs"]`. rust-analyzer sees it like any other module.

```bash
node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 8 --char 10
```

### (a) `OUT_DIR` + `include!` (feature `gen-outdir`)

`build.rs` writes the file under Cargo's hashed `OUT_DIR`. rust-analyzer must run build scripts to learn the path. Find it with:

```bash
cargo check --no-default-features --features gen-outdir --message-format=json \
  | jq -r 'select(.reason == "build-script-executed") | .out_dir'
```

then pass `<out_dir>/outou/App.rs` as `--file`. The build script also prints the path as a `cargo:warning` for convenience.

To exercise layout (a) with `ra-client.mjs` directly, select the feature set on the command line instead of editing `Cargo.toml`:

```bash
node ../client/ra-client.mjs --root . --file <out_dir>/outou/App.rs \
  --no-default-features --cargo-features gen-outdir --line 8 --char 9
```

`--cargo-features <a,b,c>` and `--no-default-features` map straight onto rust-analyzer's `initializationOptions.cargo.features` / `cargo.noDefaultFeatures`, the same knobs the editor extension exposes.

## The overlay experiment

The point of the overlay is that the editor buffer, not the file on disk, is what rust-analyzer analyzes. Test it by sending modified content:

```bash
cp src/.generated/App.rs /tmp/overlay.rs
# edit /tmp/overlay.rs: e.g. change `let user = load_user();` to `let user = load_user().name;`
node ../client/ra-client.mjs --root . --file src/.generated/App.rs --overlay /tmp/overlay.rs --line 8 --char 10
```

Hover must report the type from the *overlay* (`String`), not from disk (`User`), and it must do so without `build.rs` running again.

### A second overlay via `didChange` (`--overlay2`)

`--overlay` above only covers the buffer sent with the initial `textDocument/didOpen`. To prove that a *later* edit also reaches rust-analyzer without any re-run of `build.rs`, `--overlay2 <file>` sends a second buffer through `textDocument/didChange` (a full-document replacement, version 2) after the first hover/completion/definition round, then repeats hover/completion/definition at the same position (or `--line2`/`--char2`) as `hover2`/`completion2`/`definition2`:

```bash
node ../client/ra-client.mjs --root . --file src/.generated/App.rs --line 8 --char 9 \
  --overlay2 /tmp/overlay2.rs --timeout 180000
```

`latencyMs.overlayChangeToHover` measures from the `didChange` notification to the `hover2` response.

## Readiness

Rather than only polling hover blindly, the client waits for rust-analyzer's `$/progress` notifications after `initialized`: it treats the server as ready once the `rustAnalyzer/cachePriming` token (title "Indexing") has reported `end`. Failing that, it falls back to a debounced check: once every progress token observed so far has reported `end` and stays that way for ~2s without a new token starting, the server is treated as ready too (this guards against latching on a short-lived token, such as `rustAnalyzer/Fetching`, ending at ~0.5s while indexing is still running). This is recorded as `ready` (boolean) and `latencyMs.ready` (initialize -> ready). Because rust-analyzer can start further progress tokens (build-script fetch, proc-macro loading, cache priming) shortly after indexing finishes, this readiness check is a best-effort signal, not a guarantee — the existing hover-retry loop (bounded at 30 attempts, 1s apart) remains as a fallback and is what actually gates the first hover answer.

## Mapping diagnostics through the source map (`--source-map`)

`--source-map <source-map.json>` maps diagnostics rust-analyzer publishes for `--file` back to `App.rsx` spans, using the many-to-many format in [`source-map.json`](source-map.json). The result gains `mappedDiagnostics: [{ message, severity, generated, sources, unmapped }]`; a diagnostic whose range does not intersect any mapping gets `sources: []` and `unmapped: true`. The mapping logic lives in the pure, unit-tested `client/source-map.mjs` (`mapRange(sourceMap, range)`); run its tests with:

```bash
node --test spikes/rust-analyzer/client/*.test.mjs
# equivalently: (cd spikes/rust-analyzer/client && node --test)
```

(Passing the directory itself, e.g. `node --test spikes/rust-analyzer/client/`, does not trigger Node's test-file discovery on this project's toolchain version — use one of the two forms above.)

Note that `checkOnSave`/flycheck diagnostics come from a real `cargo check` process reading the file on disk, so they will not reflect `--overlay`/`--overlay2` content; only rust-analyzer's native (non-flycheck) diagnostics see the in-memory buffer.

## Positions to probe

Positions are 0-based; see `source-map.json` for the mapping to `App.rsx`.

| Feature | `--line` | `--char` | Expect |
|---|---|---|---|
| hover on `user` | 8 | 9 | type `User` (or overlay type) |
| definition of `load_user` | 8 | 17 | `src/main.rs` |
| completion after `user.` | 8 | — (use an overlay that ends the line with `user.`) | `name`, `age` |
| completion of component | 11 | 9 | `UserCard` |
| completion of prop | 12 | 13 | `user` |
| diagnostic | — | — | introduce an error in the overlay; expect it in `publishDiagnostics` for the generated URI, then map it through `source-map.json` via `--source-map` |

## Success criteria for Strategy A

All seven must hold:

1. rust-analyzer loads the fixture as an ordinary Cargo project.
2. The generated source is part of the crate graph.
3. Editor-side changes reach rust-analyzer **without re-running `build.rs`**. This is the most important item.
4. Completion reflects the latest buffer.
5. Hover reflects the latest buffer.
6. Definition reflects the latest buffer.
7. `cargo check` / flycheck diagnostics can be mapped back to `App.rsx` through the source map.

Record the outcome in `docs/ra-spike-results.md`. If (b) satisfies all seven, the `OUT_DIR` layout is dropped: development and published crates then share one layout, and `outou package` only has to generate and include files.

## Results

The spike has been run: layout (b) satisfies all seven criteria, layout (a) fails completion (criterion 4) inside the generated macro call and under incomplete input. Full writeup in [`docs/ra-spike-results.md`](../../docs/ra-spike-results.md); raw JSON output for every probe, with the exact command that produced it, is in [`results/`](results/).

## If Strategy A fails

Strategy B: the language server maintains a *shadow Cargo project* (under `.outou/lsp/`, reusing dependencies, features, edition and target from `cargo metadata`) whose sources are the generated Rust. `.outou/lsp/` exists only for Strategy B. Strategy C, synthesizing `rust-project.json`, is the last resort because it makes Outou responsible for the crate graph, sysroot, cfg and proc macros.
