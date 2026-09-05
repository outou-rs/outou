# modules fixtures

`mixed/` is the minimum tree Phase 0 must handle:

```text
src/
├── main.rsx
├── components.rsx
└── components/
    ├── user.rsx
    └── button.rs
```

`ambiguous/` has both `widgets.rs` and `widgets.rsx`; resolving it must fail with `ambiguous Outou module `widgets``. `expected.txt` holds the exact rendered error text (`error: ` prefix, then the `Display` of `ModuleError::Ambiguous`).

`path-attr/` exercises `#[path = "…"]`:

```text
src/
├── main.rsx              # #[path = "elsewhere/thing.rsx"] mod thing;
│                          # #[path = "somewhere/other.rs"] mod other;
├── elsewhere/
│   └── thing.rsx
└── somewhere/
    ├── other.rs           # mod nested_from_rust; (no #[path], candidate search)
    └── nested_from_rust.rsx
```

A file reached via `#[path]` is mod-rs-like (`crates/outou-modules/src/scope.rs`'s `DirScope` model): its own un-annotated children look up in *its own* directory, not in one named after the declaring module. So `other.rs`'s plain `mod nested_from_rust;` (no `#[path]`, ordinary four-candidate search) finds `nested_from_rust.rsx` as a sibling of `other.rs` itself, in `src/somewhere/` — this is also the fixture that exercises the `.rs → .rsx` transition.

`path-dirs/` is the rustc-validated directory-ownership tree: built to exactly reproduce rustc 1.98.1's file loading for every `DirScope` rule (plain-file vs. mod-rs-style candidates, `#[path]` on a file module, `#[path]` on an inline module, a `#[path]`-loaded file's own un-annotated children, and combinations of these two levels deep). Every non-decoy `.rs`/`.rsx` file that isn't itself a leaf component exists purely to declare further `mod`s; every leaf is a trivial component. Every location a naive (non-rustc-matching) resolver could plausibly pick instead has a matching decoy file marked `// decoy: must never be loaded` — the fixture test asserts both that every expected file is present and that no decoy ever appears in the resolved graph.

`rs-to-rsx/` is a second, simpler `.rs → .rsx` transition: an ordinary (no `#[path]`) `mod plain;` resolves to `plain.rs`, whose own ordinary `mod deep;` resolves to `plain/deep.rsx`.

`cfg/` has `#[cfg(feature = "x")] mod optional;` with `optional.rsx` present, and `#[ cfg(feature = "y") ]` (trivia inside the brackets) `mod spaced;` with `spaced.rsx` present; both must resolve successfully (the attribute is preserved, never evaluated — ADR 0006) even though neither feature is enabled for this test run.

`not-found/` is a plain-Rust crate root (`src/main.rs`) with `mod missing;` and no `missing.rs`/`missing.rsx`/`missing/mod.rs`/`missing/mod.rsx`; resolving it must fail with `ModuleError::NotFound`, naming `src/main.rs` as `declared_in` with a span that slices exactly `mod missing;` out of it.

`inline/` has an inline module containing a nested file module (`mod shell { mod panel; }`, with `shell/panel.rsx` on disk), to exercise the directory rule for modules declared inside an inline block.

`cycle-self/` (`#[path = "main.rsx"] mod again;`, pointing at itself) and `cycle-chain/` (`main.rsx` `#[path]`-loads `b.rsx`, which `#[path]`-loads `main.rsx` back) must both fail with `ModuleError::Circular` and the exact chain in `expected.txt`, and — this is the point of the fixture — must do so without aborting the process: before the fix, both examples overflowed the stack (SIGABRT) instead of returning an error.

`raw-ident/` has `mod r#type;`; `ModuleNode::path` keeps the raw spelling (`["r#type"]`) but the file (`type.rsx`), its children's directory (`type/`), and the generated path (`.generated/type.rs`) all use the unraw'd name, matching rustc.

`cfg-attr-path/` has `#[cfg_attr(unix, path = "unix.rsx")] mod platform;` with **both** `unix.rsx` and `platform.rsx` present. Phase 0 does not evaluate `cfg`, so it cannot choose between the conditional path's possible targets; this must fail with `ModuleError::ConditionalPath` rather than silently resolving `platform.rsx` (which is what today's un-annotated four-candidate search would otherwise do, and is not what a real build using `unix` would compile).

`cfg-duplicate/` has two `cfg`-exclusive `mod imp;` declarations (`#[cfg(unix)]` / `#[cfg(windows)]`, each with its own `#[path]`) — a std-style idiom rustc accepts (`#[cfg]` is never evaluated here either). Both must resolve, as two distinct nodes sharing the logical name `imp`, with distinct generated paths (`imp.rs`, `imp-1.rs`).

`root-name/` has `mod main { ... }` at the crate root, to confirm the root's own generated path (the reserved `crate-root` stem) never collides with a child module that happens to share the root file's name.
