# 0006. An ambiguous module is an error

Status: Accepted

## Context

`mod components;` in a `.rsx` or `.rs` file may be satisfied by `components.rsx`, `components.rs`, `components/mod.rsx` or `components/mod.rs`. rustc has its own rules for the `.rs` candidates and knows nothing about `.rsx`. Outou resolves the module graph before rustc sees anything.

## Decision

If more than one candidate exists, resolution fails:

```text
ambiguous Outou module `components`
```

There is no implicit priority between `.rs` and `.rsx`, or between `foo.rsx` and `foo/mod.rsx`. The user removes one file.

Related rules: `#[path = "…"]` is honored and decides by extension; `#[cfg(…)]` is not evaluated, every module is generated and the attribute is preserved on the generated declaration.

## Consequences

- "Which file did it pick?" can never be asked.
- Migrating a module from `.rs` to `.rsx` is a rename, not a shadowing.
- `cfg`-gated modules that do not exist for the current target still have to parse.

## Alternatives considered

- **Prefer `.rsx`.** Silent when a stale `.rs` is left behind.
- **Prefer `.rs`, matching rustc.** Silent when a new `.rsx` is added and never used.
