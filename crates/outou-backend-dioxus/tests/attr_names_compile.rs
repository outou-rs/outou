//! Every attribute name the language server offers for an HTML element
//! (`crates/outou-lsp/src/html_attrs.rs`) compiles on that element, so
//! attribute-name completion never suggests a name the backend rejects.
//!
//! The table is included from the language server's source instead of
//! duplicated, so this test cannot drift from what completion offers.
//!
//! Ignored by default (it builds the Dioxus dependency tree). Run with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test attr_names_compile -- --ignored --nocapture
//! ```

mod support;

#[path = "../../outou-lsp/src/html_attrs.rs"]
mod html_attrs;

use std::fs;
use std::process::{Command, Output};

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

const PROJECT: &str = "outou-attr-names";

/// Elements without a table row; they get only the global names.
const GLOBAL_ONLY_ELEMENTS: &[&str] = &["div", "section", "span", "p", "ul"];

/// One self-closing element per tag, carrying every offered name. Event
/// handlers take a closure, everything else a string.
fn source() -> String {
    let mut tags: Vec<&str> = html_attrs::elements_with_own_attributes().collect();
    tags.extend(GLOBAL_ONLY_ELEMENTS);
    let mut body = String::new();
    for tag in tags {
        body.push_str(&format!("        <{tag}\n"));
        for name in html_attrs::attribute_names(tag) {
            let value = if name.starts_with("on") {
                "{move |_| {}}"
            } else {
                "\"x\""
            };
            body.push_str(&format!("            {name}={value}\n"));
        }
        body.push_str("        />\n");
    }
    format!(
        "use outou::prelude::*;\n\n#[component]\nfn App() -> Element {{\n    <div>\n{body}    </div>\n}}\n\nfn main() {{\n    let _ = App;\n}}\n"
    )
}

fn generate(source: &str) -> String {
    let parsed = outou_syntax::parse(source);
    DioxusBackend
        .generate(
            &parsed,
            source,
            Mode::Strict,
            &GenerateOptions::new(
                Uri::new("file:///gen/main.rs"),
                Uri::new("file:///src/main.rsx"),
            ),
        )
        .expect("generation succeeds")
        .rust
}

fn cargo_check(source: &str) -> Output {
    let root = support::repo_root();
    let dir = root.join("target").join(PROJECT);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/main.rs"), generate(source)).unwrap();
    let outou_path = root.join("crates/outou");
    let manifest = format!(
        "[package]\nname = \"{PROJECT}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [workspace]\n\n[dependencies]\noutou = {{ path = {outou_path:?} }}\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    Command::new(env!("CARGO"))
        .args(["check", "--color=never"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("running cargo")
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn every_offered_attribute_name_compiles_on_its_element() {
    let source = source();
    let output = cargo_check(&source);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{stderr}\n--- source ---\n{source}"
    );
}
