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

## The overlay experiment

The point of the overlay is that the editor buffer, not the file on disk, is what rust-analyzer analyzes. Test it by sending modified content:

```bash
cp src/.generated/App.rs /tmp/overlay.rs
# edit /tmp/overlay.rs: e.g. change `let user = load_user();` to `let user = load_user().name;`
node ../client/ra-client.mjs --root . --file src/.generated/App.rs --overlay /tmp/overlay.rs --line 8 --char 10
```

Hover must report the type from the *overlay* (`String`), not from disk (`User`), and it must do so without `build.rs` running again.

## Positions to probe

Positions are 0-based; see `source-map.json` for the mapping to `App.rsx`.

| Feature | `--line` | `--char` | Expect |
|---|---|---|---|
| hover on `user` | 8 | 9 | type `User` (or overlay type) |
| definition of `load_user` | 8 | 17 | `src/main.rs` |
| completion after `user.` | 8 | — (use an overlay that ends the line with `user.`) | `name`, `age` |
| completion of component | 11 | 9 | `UserCard` |
| completion of prop | 12 | 13 | `user` |
| diagnostic | — | — | introduce a type error in the overlay; expect it in `publishDiagnostics` for the generated URI, then map it through `source-map.json` |

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

## If Strategy A fails

Strategy B: the language server maintains a *shadow Cargo project* (under `.outou/lsp/`, reusing dependencies, features, edition and target from `cargo metadata`) whose sources are the generated Rust. `.outou/lsp/` exists only for Strategy B. Strategy C, synthesizing `rust-project.json`, is the last resort because it makes Outou responsible for the crate graph, sysroot, cfg and proc macros.
