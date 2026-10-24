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

mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

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

/// Removes ANSI escape sequences (`\x1b[1m\x1b[33m`): CI sets
/// `CARGO_TERM_COLOR=always`, which colours diagnostics.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        }
    }
    out
}

/// True when cargo's stderr holds a rustc warning diagnostic: a line that
/// starts with `warning:` or `warning[`. Cargo's progress lines
/// (`Compiling warnings v0.2.1`) merely mention the word, and the summary
/// `warning: build failed` is a warning line too, which is fine since the
/// caller has already asserted success.
fn has_compiler_warning(stderr: &str) -> bool {
    strip_ansi(stderr).lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("warning:") || line.starts_with("warning[")
    })
}

#[test]
fn a_progress_line_naming_the_warnings_crate_is_not_a_warning() {
    assert!(!has_compiler_warning("   Compiling warnings v0.2.1\n"));
    assert!(!has_compiler_warning(
        "\x1b[1m\x1b[92m   Compiling\x1b[0m warnings v0.2.1\n    Checking error-code v1.0.0\n"
    ));
}

#[test]
fn coloured_and_plain_warning_diagnostics_are_detected() {
    assert!(has_compiler_warning(
        "\x1b[1m\x1b[33mwarning\x1b[0m\x1b[1m: unused variable: `x`\x1b[0m\n"
    ));
    assert!(has_compiler_warning("warning: unused variable: `x`\n"));
    assert!(has_compiler_warning("warning[E0123]: something\n"));
    assert!(has_compiler_warning("  warning: indented\n"));
    assert!(has_compiler_warning(
        "   Compiling a v1\nwarning: build failed, waiting\n"
    ));
}

#[test]
fn clean_output_has_no_warning() {
    assert!(!has_compiler_warning(""));
    assert!(!has_compiler_warning(
        "   Compiling x v0.0.0\n    Finished `dev` profile in 1s\n     Running `target/debug/x`\n"
    ));
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
