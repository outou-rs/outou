---
title: "[Week 4] Module resolver for mixed .rs / .rsx crates"
milestone: "Phase 0"
week: "Week 4"
gate: "—"
droppable: no
labels: [phase0, must-keep]
---

| | |
|---|---|
| **Gate** | none (required by Gate 2) |
| **Droppable** | No |
| **Crate** | `outou-modules` |

- [x] candidates `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs`; more than one existing candidate → `ambiguous Outou module `foo`` declared in its own file (`tests/fixtures/modules/ambiguous/`); a stray directory of the same name is not counted as a candidate (`existing_file` checks `is_file()`, not mere existence)
- [x] `#[path = "…"]` honored; directory ownership for both implicit `mod name;` candidates and `#[path]` targets matches rustc's own rules exactly, validated directly against rustc 1.98.1 (`crates/outou-modules/src/scope.rs`'s `DirScope` model; `tests/fixtures/modules/path-attr/`, `tests/fixtures/modules/path-dirs/`). Extension still decides `.rsx` vs plain Rust.
- [x] `#[cfg(…)]` not evaluated: all modules generated, attribute preserved on the generated declaration (`ModuleNode::cfg`/`attributes`; `tests/fixtures/modules/cfg/`), including `#[ cfg(...) ]` with trivia inside the brackets (`tests/fixtures/modules/cfg/src/spaced.rsx`)
- [x] explicit generated module graph (no reliance on rustc's implicit `.rs` lookup for generated files) — `ModuleGraph`/`ModuleNode`, `ModuleNode::generated_path` per ADR 0009 layout (b); the crate root always uses the reserved `crate-root` stem so a child module named `main` cannot collide with it, and sibling declarations sharing one logical name (the `cfg`-exclusive `mod imp;` idiom) get disambiguated generated paths (`tests/fixtures/modules/root-name/`, `tests/fixtures/modules/cfg-duplicate/`)
- [x] `tests/fixtures/modules/mixed/` resolves: `.rsx → .rsx`, `.rsx → .rs`, nested `components/user.rsx` and `components/button.rs`. The `.rs → .rsx` transition is covered by `tests/fixtures/modules/path-attr/` (`other.rs`'s plain `mod nested_from_rust;` resolves to `nested_from_rust.rsx`) and by `tests/fixtures/modules/rs-to-rsx/`.
- [x] a `#[path]` chain that re-opens a file already open higher up (directly or through further nesting) is reported as `ModuleError::Circular` instead of overflowing the stack (previously a SIGABRT); module nesting is additionally capped at `MAX_MODULE_DEPTH` (128) with `ModuleError::TooDeep`, since acyclic depth is not bounded by cycle detection alone (`tests/fixtures/modules/cycle-self/`, `tests/fixtures/modules/cycle-chain/`)

Documented Phase 0 limitations (explicit diagnostics, not silently-wrong resolution):

- `#[cfg_attr(condition, path = "…")]` is rejected with `ModuleError::ConditionalPath`: Phase 0 does not evaluate `cfg`, so it cannot choose which of a conditional path's targets to resolve (`tests/fixtures/modules/cfg-attr-path/`).
- An explicit `#[path]` that resolves outside the crate root (an absolute path, or enough `..` segments to escape it) is rejected with `ModuleError::OutsideCrate`: Phase 0 requires every module file to live under the crate root.
