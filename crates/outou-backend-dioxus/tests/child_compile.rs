//! Issue #27: every accepted shape of JSX child compiles without a warning
//! (the generated `child(..)` call adds no braces of its own, so Rust's
//! `unused_braces` stays quiet) and renders, and a child of an unsupported
//! type is reported in Outou's words.
//!
//! Ignored by default (it builds the Dioxus dependency tree). Run with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test child_compile -- --ignored --nocapture
//! ```

mod compiler_output;
mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

use compiler_output::has_compiler_warning;

const ACCEPTED: &str = r##"use outou::prelude::*;

unsafe fn raw_three() -> usize {
    3
}

#[component]
fn Row(label: String) -> Element {
    <li>{label}</li>
}

#[component]
fn App() -> Element {
    let count: usize = 3;
    let ratio = 3.5;
    let labels = vec![String::from("a"), String::from("b")];
    let maybe: Option<String> = Some(String::from("some"));
    let open = count > 1;
    let debug = false;
    let ready = false;
    let cond = false;
    let value = 1;
    let maybe_n: Option<usize> = Some(2);
    <div>
        {count}
        {ratio}
        {-2}
        {'c'}
        {"literal"}
        {maybe}
        {labels.iter().map(|label| <Row key={label.as_str()} label={label.clone()} />)}
        {if open { <b>{count}</b> } else { <i>closed</i> }}
        {match count { 0 => None, _ => Some(<u>{count}</u>) }}
        {let shown = open.then(|| <u>shown</u>); shown}
        {<span>direct</span>}
        // Statements that need no `;` keep their braces, one expression
        // does not.
        {if debug { println!("render") } "x"}
        {if ready {} "done"}
        {while cond {} "done"}
        {loop { break } "done"}
        {match value { _ => {} } "done"}
        {fn h() -> &'static str { "x" } h()}
        {"a;{}"}
        {';'}
        {r#"x;}"#}
        {'a: { if open { break 'a 1 } 2 }}
        {unsafe { raw_three() }}
        {loop { break 4 }}
        {if let Some(n) = maybe_n { n } else { 0 }}
        {(|x: usize| { x + 1 })(count)}
        {vec![1; 3].len()}
        {format!("{};", count)}
        {{ let inner = count; inner }}
        <Row label={String::from("nested")} />
    </div>
}

fn main() {
    let mut dom = outou::prelude::dioxus_core::VirtualDom::new(App);
    dom.rebuild_in_place();
}
"##;

const REJECTED: &str = r#"use outou::prelude::*;

struct Price(u32);

#[component]
fn App() -> Element {
    let price = Price(3);
    let open = true;
    <ul>
        <li>{price}</li>
        <li>{open}</li>
        <li>{open && 1 > 0}</li>
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
        .env("CARGO_TARGET_DIR", target_dir(&root))
        .output()
        .expect("running cargo")
}

/// The shared target directory; `CARGO_TARGET_DIR` overrides it so a cold CI
/// build can be reproduced without touching the warm one.
fn target_dir(root: &Path) -> std::path::PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
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
fn every_accepted_child_shape_compiles_without_warnings_and_renders() {
    let output = run("outou-child-accepted", ACCEPTED, "run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!has_compiler_warning(&stderr), "{stderr}");
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn a_child_of_an_unsupported_type_is_reported_in_outou_words() {
    let output = run("outou-child-rejected", REJECTED, "check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("`Price` cannot be rendered as a JSX child"),
        "{stderr}"
    );
    assert!(
        stderr.contains("`bool` cannot be rendered as a JSX child"),
        "{stderr}"
    );
}
