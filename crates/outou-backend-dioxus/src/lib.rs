//! Dioxus backend: lowers the Outou AST to `rsx!` invocations.
//!
//! This crate emits *text*. It does not depend on Dioxus; the runtime is
//! reached at compile time of the user's crate through the hidden path in
//! [`PRIVATE_PATH`], which the `outou` facade re-exports. A user crate
//! therefore never lists Dioxus in its `Cargo.toml`.
//!
//! Children are lowered as separate nodes (`"Hello "` and `{name}`), never
//! folded into a single format string, so that the source map can point at
//! each of them.

use outou_codegen::{Backend, Error, Generated, Mode};
use outou_syntax::ast;

/// Root path through which generated code reaches the runtime.
pub const PRIVATE_PATH: &str = "::outou::__private";

/// The Dioxus backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct DioxusBackend;

impl Backend for DioxusBackend {
    fn name(&self) -> &'static str {
        "dioxus"
    }

    fn generate(&self, file: &ast::File, mode: Mode) -> Result<Generated, Error> {
        let _ = (file, mode);
        todo!("outou-backend-dioxus: lowering is implemented in Phase 0, Week 4 (Gate 2)")
    }
}
