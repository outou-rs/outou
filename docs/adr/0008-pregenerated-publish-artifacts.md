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

## Alternatives considered

- **Consumer-time generation via `build.rs`.** Pulls the whole Outou compiler into every consumer's build and makes IDE behavior depend on build scripts.
- **Both, selectable.** Two publishing modes double the test matrix for no user-visible gain.
