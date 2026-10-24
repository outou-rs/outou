//! Issue #28: `<select value={V}>` selects the matching `<option>`, checked on
//! a headless render of generated code, and a value of an unsupported type is
//! reported once, in Outou's words.
//!
//! Ignored by default (it builds the Dioxus dependency tree). Run with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test select_compile -- --ignored --nocapture
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

const ACCEPTED: &str = r#"use outou::prelude::dioxus_core::{AttributeValue, Mutation, VirtualDom};
use outou::prelude::*;

/// The `selected` flags of one fresh render, in the order the backend sets
/// them. Each component below holds one scenario, so that order is the
/// document order of its options.
fn flags(root: fn() -> Element) -> Vec<bool> {
    let mut dom = VirtualDom::new(root);
    selected_flags(dom.rebuild_to_vec().edits)
}

#[component]
fn Mapped() -> Element {
    let current = String::from("High");
    <select value={current}>
        {["Low", "Medium", "High"].into_iter().map(|level| <option key={level} value={level}>{level}</option>)}
    </select>
}

#[component]
fn Literal() -> Element {
    <select value={2u8}>
        <option value="1">One</option>
        <option value={2u8}>Two</option>
        <option value="2" selected={false}>Explicit selected wins</option>
        <option value="3" selected>Explicit selected, bare</option>
    </select>
}

#[component]
fn InGroup() -> Element {
    let levels = [1u8, 2, 3];
    <select value={2u8}>
        <optgroup label="More">
            {levels.iter().map(move |level| <option value={*level}>{*level}</option>)}
        </optgroup>
    </select>
}

#[component]
fn StaticValue() -> Element {
    <select value="static">
        <option value="static">Static</option>
        <option value="other">Other</option>
    </select>
}

#[component]
fn BlockValue() -> Element {
    <select value={ let base = 1u8; base + 1 }>
        <option value="2">Two</option>
    </select>
}

#[component]
fn NoMatch() -> Element {
    <select value="High">
        <option value="Nothing">Matches nothing</option>
    </select>
}

/// `value={*level}` becomes `option_value(&(*level))`, which Clippy's
/// `borrow_deref_ref` flags unless the generated `let` allows it.
#[component]
fn Derefs() -> Element {
    let levels = [1u8, 2, 3];
    let current = &2u8;
    <select value={*current}>
        {levels.iter().map(|level| <option value={*level}>{*level}</option>)}
    </select>
}

#[component]
fn MultipleFalse() -> Element {
    <select multiple={false} value="b">
        <option value="a">A</option>
        <option value="b">B</option>
    </select>
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Signal<String>>> =
        const { std::cell::RefCell::new(None) };
}

#[component]
fn Controlled() -> Element {
    let current = use_signal(|| String::from("a"));
    CURRENT.with(|cell| *cell.borrow_mut() = Some(current));
    <select value={current()}>
        <option value="a">A</option>
        <option value="b">B</option>
    </select>
}

/// The `selected` attributes set by `edits`, in order.
fn selected_flags(edits: Vec<Mutation>) -> Vec<bool> {
    edits
        .into_iter()
        .filter_map(|edit| match edit {
            Mutation::SetAttribute {
                name: "selected",
                value: AttributeValue::Bool(flag),
                ..
            } => Some(flag),
            _ => None,
        })
        .collect()
}

/// Renders `Controlled`, changes its state from "a" to "b" and renders again:
/// the `selected` flags must move with the state.
fn rerender_after_a_state_change() -> (Vec<bool>, Vec<bool>) {
    let mut dom = VirtualDom::new(Controlled);
    let first = selected_flags(dom.rebuild_to_vec().edits);
    let signal = CURRENT.with(|cell| cell.borrow().expect("the component ran"));
    dom.in_runtime(|| {
        let mut signal = signal;
        signal.set(String::from("b"));
    });
    dom.mark_dirty(outou::prelude::dioxus_core::ScopeId::APP);
    let second = selected_flags(dom.render_immediate_to_vec().edits);
    (first, second)
}

#[component]
fn Multiple() -> Element {
    <select multiple value="High">
        <option value="High">Not rewritten</option>
    </select>
}

#[component]
fn Nested() -> Element {
    let levels = [1u8, 2, 3];
    <div>
        {levels.iter().map(|level| <select value={*level}>
            {levels.iter().map(|other| <option value={*other}>{*other}</option>)}
        </select>)}
    </div>
}

fn main() {
    assert_eq!(flags(Mapped), [false, false, true], "options in a map");
    assert_eq!(
        flags(Literal),
        [false, true, false, true],
        "literal options and an explicit selected"
    );
    assert_eq!(flags(InGroup), [false, true, false], "options in an optgroup");
    assert_eq!(flags(StaticValue), [true, false], "a static value");
    assert_eq!(flags(BlockValue), [true], "a value written as a block");
    assert_eq!(flags(NoMatch), [false], "no option matches");
    assert_eq!(flags(Derefs), [false, true, false], "dereferenced values");
    assert_eq!(flags(Multiple), Vec::<bool>::new(), "multiple is not rewritten");
    assert_eq!(flags(MultipleFalse), [false, true], "a literal false for multiple is a single select");
    let (first, second) = rerender_after_a_state_change();
    assert_eq!(first, [true, false], "first render of the controlled select");
    assert_eq!(second, [false, true], "the flags follow the state on a re-render");
    assert_eq!(
        flags(Nested),
        [true, false, false, false, true, false, false, false, true],
        "a select in an island"
    );
}
"#;

const REJECTED: &str = r#"use outou::prelude::*;

struct Level(u8);
struct Price(u8);

#[component]
fn App() -> Element {
    <select value={Level(1)}>
        <option value={true}>flag</option>
        <option value={Price(2)}>price</option>
        {[1.5f32].into_iter().map(|n| <option value={n}>{n}</option>)}
    </select>
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
fn the_matching_option_is_selected_on_a_fresh_render() {
    let output = run("outou-select-accepted", ACCEPTED, "run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!has_compiler_warning(&stderr), "{stderr}");
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn the_accepted_shapes_pass_clippy_without_warnings() {
    let output = run_with(
        "outou-select-accepted",
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
fn a_value_of_an_unsupported_type_is_reported_once_in_outou_words() {
    let output = run("outou-select-rejected", REJECTED, "check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    for type_name in ["Level", "bool", "Price", "f32"] {
        let message =
            format!("`{type_name}` cannot be used as the value of a `<select>` or `<option>`");
        assert_eq!(
            stderr.matches(&message).count(),
            1,
            "`{type_name}` must be reported exactly once:\n{stderr}"
        );
    }
}
