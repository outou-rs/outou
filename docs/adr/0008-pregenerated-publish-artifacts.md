# 0008. Published crates ship pre-generated Rust

Status: Accepted

## Context

A library written in `.rsx` must be usable with `cargo add` and `cargo build` alone. Two ways to achieve that: generate at the consumer's build time from a `build.rs` inside the published crate, or generate before publishing and ship the Rust.

## Decision

Published crates contain pre-generated Rust. `outou package` generates it and includes it in the package. The `.rsx` files remain the source of truth; CI regenerates from them and fails if the result differs from what is committed or packaged. Consumers need neither `outou` nor a build dependency.

## Consequences

- No build script runs on the consumer's machine; no `outou-build` in their dependency graph.
- Library repositories commit their generated Rust; applications may ignore it (see `.gitignore` and CONTRIBUTING).
- Generated output has to be deterministic, which is required anyway.
- The full automation of `outou package` may be deferred; the design is fixed now.
- **Generated output is crate-relative, not machine-relative (issue #15).** The generated `.rs` header and the `.rs.map.json` sidecar's `generated`/`sources` fields used to embed the generating machine's absolute `file://` path, so a library regenerated on a different machine (or CI) than the one it was committed from produced different bytes even with identical `.rsx` sources — the exact thing determinism requires not to happen for published output. Both now use a path relative to the crate directory (`outou_codegen::GenerateOptions::source_display`; `outou-cli`'s `build::paths::relativize_map_json`); `generated_uri`/`source_uri` themselves stay absolute, since the language server's in-memory registry matches them against rust-analyzer's own (always absolute) URIs. The on-disk `.rs.map.json` format stays at `version: 1`: `outou-sourcemap`'s `json.rs` already documents `generated`/`sources` as "a path or URI", so a crate-relative path is still a valid v1 document, and nothing in this repository reads the file back regardless (confirmed by a repository-wide grep, issue #15).
- Consumers still depend on the `outou` runtime crate as an ordinary dependency (ADR 0010); what they do not need is the Outou compiler/CLI or a build script.
- **`outou package`'s Phase 0 publish blocker is resolved, not deferred.** `outou` being unpublished no longer blocks a library's `cargo publish`: packaging `outou` and the library together in one `cargo package`/`publish` invocation makes Cargo build the lockfile assuming both land on the same registry, resolving the library's local `path` dependency on `outou`. See `crates/outou-cli/README.md`'s `outou package` section and `docs/phase0/issues/15-outou-package.md`.

## Alternatives considered

- **Consumer-time generation via `build.rs`.** Pulls the whole Outou compiler into every consumer's build and makes IDE behavior depend on build scripts.
- **Both, selectable.** Two publishing modes double the test matrix for no user-visible gain.
