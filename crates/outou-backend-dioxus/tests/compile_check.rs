//! Compile-check test (issue #6, deliverable 5b): generates
//! `examples/phase0-app`'s two `.rsx` files, writes them into a throwaway
//! crate under `target/` with `outou` as a path dependency, and runs
//! `cargo check` on it. This is the strongest available proof that the
//! Dioxus lowering actually produces code the real backend accepts, not
//! just text that looks plausible.
//!
//! Ignored by default: building `dioxus` from a cold `target/` can take
//! well over 30s. Run explicitly with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test compile_check -- --ignored --nocapture
//! ```

mod support;

use std::fs;
use std::process::Command;

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn generated_example_app_passes_cargo_check() {
    let root = support::repo_root();

    let main_source = fs::read_to_string(root.join("examples/phase0-app/src/main.rsx")).unwrap();
    let components_source =
        fs::read_to_string(root.join("examples/phase0-app/src/components.rsx")).unwrap();

    let main_generated = DioxusBackend
        .generate(
            &outou_syntax::parse(&main_source),
            &main_source,
            Mode::Strict,
            &GenerateOptions::new(
                Uri::new("file:///gen/main.rs"),
                Uri::new("file:///src/main.rsx"),
            )
            .with_module_path("components", "components.rs"),
        )
        .expect("main.rsx generates");
    let components_generated = DioxusBackend
        .generate(
            &outou_syntax::parse(&components_source),
            &components_source,
            Mode::Strict,
            &GenerateOptions::new(
                Uri::new("file:///gen/components.rs"),
                Uri::new("file:///src/components.rsx"),
            ),
        )
        .expect("components.rsx generates");

    let temp_dir = root.join("target/outou-compile-check");
    let src_dir = temp_dir.join("src");
    fs::create_dir_all(&src_dir).expect("creating temp crate src dir");

    // `main.rs` doubles as the binary entry point: `main.rsx` already
    // defines `fn main()`.
    fs::write(src_dir.join("main.rs"), &main_generated.rust).unwrap();
    fs::write(src_dir.join("components.rs"), &components_generated.rust).unwrap();

    let outou_path = root.join("crates/outou");
    let dependency_line = format!("outou = {{ path = {outou_path:?} }}");
    let manifest = format!(
        "[package]\n\
         name = \"outou-compile-check\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [workspace]\n\
         \n\
         [dependencies]\n\
         {dependency_line}\n"
    );
    fs::write(temp_dir.join("Cargo.toml"), manifest).unwrap();

    let status = Command::new(env!("CARGO"))
        .arg("check")
        .current_dir(&temp_dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .status()
        .expect("running `cargo check` on the generated example app");

    assert!(
        status.success(),
        "cargo check failed for the generated example app (see output above)"
    );
}
