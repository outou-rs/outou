//! Issue #26: the `key` shapes that used to fail in the backend
//! (`key="new"`, `key={format!("edit-{id}")}`) and the ones that already
//! worked compile and render, and a key of an unsupported type is reported
//! in Outou's words.
//!
//! Ignored by default (it builds the Dioxus dependency tree). Run with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test key_compile -- --ignored --nocapture
//! ```

mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

const ACCEPTED: &str = r#"use outou::prelude::*;

#[component]
fn Row(label: String) -> Element {
    <li>{label}</li>
}

#[component]
fn App() -> Element {
    let id = 7u32;
    let name = String::from("edit");
    let labels = vec![String::from("a"), String::from("b")];
    <ul key="root">
        <Row key="new" label={name.clone()} />
        <Row key={format!("edit-{id}")} label={name.clone()} />
        <Row key={id} label={name.clone()} />
        <Row key={String::from("x")} label={name.clone()} />
        <Row key={name} label={String::from("moved after the key")} />
        {labels.iter().map(|label| <Row key={label.as_str()} label={label.clone()} />)}
    </ul>
}

fn main() {
    let mut dom = outou::prelude::dioxus_core::VirtualDom::new(App);
    dom.rebuild_in_place();
}
"#;

const REJECTED: &str = r#"use outou::prelude::*;

struct Money(u32);

#[component]
fn App() -> Element {
    let price = Money(3);
    <ul>
        <li key={price}>price</li>
        <li key={true}>flag</li>
    </ul>
}

fn main() {
    let _ = App;
}
"#;

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

fn run(dir_name: &str, source: &str, subcommand: &str) -> Output {
    let root = support::repo_root();
    let temp_dir = root.join("target").join(dir_name);
    fs::create_dir_all(temp_dir.join("src")).unwrap();
    fs::write(temp_dir.join("src/main.rs"), generate(source)).unwrap();
    write_manifest(&temp_dir, &root, dir_name);
    Command::new(env!("CARGO"))
        .arg(subcommand)
        .arg("--color=never")
        .current_dir(&temp_dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .expect("running cargo")
}

fn write_manifest(dir: &Path, root: &Path, name: &str) {
    let outou_path = root.join("crates/outou");
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [workspace]\n\n[dependencies]\noutou = {{ path = {outou_path:?} }}\n"
    );
    fs::write(dir.join("Cargo.toml"), manifest).unwrap();
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn static_expression_integer_and_formatted_keys_compile_and_render() {
    let output = run("outou-key-accepted", ACCEPTED, "run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn a_key_of_an_unsupported_type_is_reported_in_outou_words() {
    let output = run("outou-key-rejected", REJECTED, "check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("`Money` cannot be used as a JSX `key`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("`bool` cannot be used as a JSX `key`"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("Key must not be a static string"),
        "{stderr}"
    );
}
