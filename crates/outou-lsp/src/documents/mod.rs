//! In-memory state: every `.rsx` document this server knows about and the
//! generated Rust unit overlaid onto rust-analyzer for it.
//!
//! There is one [`Workspace`] per `initialize`d root. It owns the plan
//! (`crate::plan`), the `Registry` every position/location mapping goes
//! through, and generates every unit in `Mode::Recovery` — never
//! `Mode::Strict`: the language server must keep working while the file
//! is half-typed, which is exactly what recovery mode is for.
//!
//! Split into [`units`] (the per-document/per-generated-unit types) and
//! [`workspace`] (the [`Workspace`] that owns and plans them).

mod units;
mod workspace;

#[cfg(test)]
pub(crate) use units::test_generated_unit;
pub use units::RsxDocument;
pub use workspace::{LoadOutcome, Workspace};
