//! Library surface for the `outou` command line tool.
//!
//! `outou build`'s pipeline is exposed as a plain library function
//! ([`build::build`]) rather than being reachable only by spawning the
//! `outou` binary, so `cargo xtask determinism` and this crate's own test
//! suite can call it directly. "There is one compiler" (`AGENTS.md`): the
//! binary (`src/main.rs`) is a thin argument-parsing wrapper around the
//! same [`build::build`] this library exposes.

pub mod build;
pub mod check;
