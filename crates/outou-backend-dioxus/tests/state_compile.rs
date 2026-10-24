//! Issue #29: the state API (`State<T>`, `use_state`, ...) in generated code.
//! The accepted shapes compile, run and are clippy-clean; the shapes that
//! needed a guard (`read()`, `write()`, the F4 borrow) do not compile, and
//! the errors name no backend type.
//!
//! Ignored by default (it builds the Dioxus dependency tree). Run with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test state_compile -- --ignored --nocapture
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

const ACCEPTED: &str = r#"use outou::prelude::dioxus_core::VirtualDom;
use outou::prelude::*;

#[derive(Clone, Debug, PartialEq)]
struct Draft {
    title: String,
    notes: String,
}

#[component]
fn Counter(count: State<usize>) -> Element {
    <button onclick={move |_| count.update(|n| *n += 1)}>{count}</button>
}

#[component]
fn Form() -> Element {
    let draft = use_state(|| Draft { title: String::new(), notes: String::new() });
    let shared = provide_context(|| draft);
    let current = draft.get();
    let length = shared.with(|d| d.title.len());
    <form>
        <input value={current.title.clone()} oninput={move |e| draft.update(|d| d.title = e.value())} />
        <input value={current.notes.clone()} oninput={move |e| draft.set(Draft { notes: e.value(), ..draft.get() })} />
        <p>{length}</p>
        <Reader />
    </form>
}

#[component]
fn Reader() -> Element {
    let draft = use_context::<State<Draft>>();
    <p>{draft.get().title}</p>
}

#[component]
fn App() -> Element {
    let count = use_state(|| 0usize);
    <div>
        <Counter count={count} />
        <Form />
    </div>
}

fn main() {
    VirtualDom::new(App).rebuild_in_place();
}
"#;

const REJECTED: &str = r#"use outou::prelude::*;

#[derive(Clone)]
struct Draft {
    title: String,
}

#[component]
fn App() -> Element {
    let draft = use_state(|| Draft { title: String::new() });
    let guard = draft.read();
    draft.write();
    draft.set(Draft { title: String::new(), ..draft.read().clone() });
    let _ = *draft;
    let _ = draft();
    <p>{guard.title.clone()}</p>
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
    run_with(dir_name, source, subcommand, &[])
}

fn run_with(dir_name: &str, source: &str, subcommand: &str, extra: &[&str]) -> Output {
    let root = support::repo_root();
    let temp_dir = root.join("target").join(dir_name);
    fs::create_dir_all(temp_dir.join("src")).unwrap();
    fs::write(temp_dir.join("src/main.rs"), generate(source)).unwrap();
    write_manifest(&temp_dir, &root, dir_name);
    Command::new(env!("CARGO"))
        .arg(subcommand)
        .arg("--color=never")
        .args(extra)
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
fn the_accepted_shapes_compile_and_run_without_warnings() {
    let output = run("outou-state-accepted", ACCEPTED, "run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!has_compiler_warning(&stderr), "{stderr}");
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn the_accepted_shapes_pass_clippy_without_warnings() {
    let output = run_with(
        "outou-state-accepted",
        ACCEPTED,
        "clippy",
        &["--", "-D", "warnings"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!has_compiler_warning(&stderr), "{stderr}");
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn a_guard_cannot_be_taken_and_the_errors_name_no_backend_type() {
    let output = run("outou-state-rejected", REJECTED, "check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    for method in ["read", "write"] {
        let message = format!("no method named `{method}` found for struct `State<");
        assert!(
            stderr.contains(&message),
            "`{method}` must not exist:\n{stderr}"
        );
    }
    assert!(stderr.contains("cannot be dereferenced"), "{stderr}");
    assert!(
        stderr.contains("expected function, found `State<"),
        "{stderr}"
    );
    for leak in [
        "Signal",
        "Generational",
        "generational",
        "dioxus",
        "ReadableExt",
        "WritableExt",
    ] {
        assert!(!stderr.contains(leak), "`{leak}` leaked:\n{stderr}");
    }
}
