//! Variant (a) only: copy the hand-written virtual Rust into OUT_DIR.
//!
//! In the real compiler this is where `outou-build` would generate from
//! `.rsx`. For the spike the "generated" file is hand-written and lives in
//! `../virtual/App.rs`.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../virtual/App.rs");
    println!("cargo:rerun-if-changed=src/App.rsx");

    if env::var_os("CARGO_FEATURE_GEN_OUTDIR").is_none() {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let target_dir = out_dir.join("outou");
    fs::create_dir_all(&target_dir).expect("create OUT_DIR/outou");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let virtual_rs = manifest_dir.join("../virtual/App.rs");
    fs::copy(&virtual_rs, target_dir.join("App.rs")).expect("copy virtual App.rs into OUT_DIR");

    // Printed so the headless client can find the generated file without
    // parsing cargo's JSON output.
    println!("cargo:warning=outou-spike: generated {}", target_dir.join("App.rs").display());
}
