# outou-modules

Module graph resolver for crates mixing `.rs` and `.rsx`. Candidates for `mod foo;` are `foo.rsx`, `foo.rs`, `foo/mod.rsx`, `foo/mod.rs`; more than one existing candidate is an error. `#[path]` is honored and `#[cfg]` is preserved, not evaluated.

Phase 0: Week 4 (Gate 2).
