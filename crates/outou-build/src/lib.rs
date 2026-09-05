//! `build.rs` helper for the OUT_DIR generation layout.
//!
//! Two layouts for generated Rust are under evaluation:
//!
//! - (a) `build.rs` generates into `OUT_DIR` and the crate `include!`s it.
//!   This crate is the helper for that layout.
//! - (b) The compiler writes `src/.generated/` and the crate references it
//!   with `#[path]`. No build script is involved and this crate is unused.
//!
//! Which layout ships is decided after the rust-analyzer spike (see
//! `docs/adr/0009-generated-source-location.md`). Until then this crate is
//! a placeholder. Whatever the outcome, `build.rs` never writes into `src/`.

use std::path::PathBuf;

/// Errors from [`generate_to_out_dir`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `OUT_DIR` was not set; the function was not called from `build.rs`.
    #[error("OUT_DIR is not set; call this from a build script")]
    NoOutDir,
    /// Module resolution failed.
    #[error(transparent)]
    Modules(#[from] outou_modules::ModuleError),
    /// Code generation failed.
    #[error(transparent)]
    Codegen(#[from] outou_codegen::Error),
}

/// Generates Rust for every `.rsx` module of the calling crate into
/// `$OUT_DIR/outou/` and emits the `cargo:rerun-if-changed` lines.
///
/// Returns the directory that was written.
pub fn generate_to_out_dir() -> Result<PathBuf, Error> {
    let out_dir = std::env::var_os("OUT_DIR").ok_or(Error::NoOutDir)?;
    let _ = PathBuf::from(out_dir).join("outou");
    todo!("outou-build: implemented only if the OUT_DIR layout is adopted (ADR 0009)")
}
