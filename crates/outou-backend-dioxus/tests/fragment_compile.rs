//! Fragments (`<>…</>`, ADR 0015), checked on generated code that is
//! compiled against the real Dioxus 0.7 and rendered headlessly: a component
//! whose body is a fragment has several roots, a fragment child is spliced
//! into its parent's children, an empty fragment renders nothing, and
//! `key`, `<select value>` and `.map` work through a fragment.
//!
//! Ignored by default (it builds the Dioxus dependency tree). Run with:
//!
//! ```sh
//! cargo test -p outou-backend-dioxus --test fragment_compile -- --ignored --nocapture
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

const ACCEPTED: &str = r#"use outou::prelude::dioxus_core::{
    AttributeValue, DynamicNode, Mutation, ScopeId, TemplateNode, VNode, VirtualDom,
};
use outou::prelude::*;

/// A component with several roots: the rows of a table body.
#[component]
fn TwoRoots() -> Element {
    <>
        <tr><td>a</td></tr>
        <tr><td>b</td></tr>
    </>
}

#[component]
fn SingleChild() -> Element {
    <><p>x</p></>
}

#[component]
fn Empty() -> Element {
    <></>
}

/// Text and an expression directly in the fragment are roots too.
#[component]
fn Mixed() -> Element {
    let n = 7;
    <>
        <b>x</b>
        tail {n}
    </>
}

#[component]
fn Spliced() -> Element {
    <ul>
        <><li>a</li><li>b</li></>
        <li>c</li>
        <><></></>
        <><><li>d</li></></>
    </ul>
}

#[component]
fn InMap() -> Element {
    let items = [1u8, 2];
    <ul>
        {items.iter().map(|n| <><li>{*n}</li><li>!</li></>)}
    </ul>
}

#[component]
fn InAttributeIsland() -> Element {
    let rows = [<><i>p</i><i>q</i></>];
    <div>{rows.into_iter()}</div>
}

#[component]
fn Rows() -> Element {
    <>
        <tr><td>1</td></tr>
        <tr><td>2</td></tr>
    </>
}

#[component]
fn Wrap(content: Element) -> Element {
    <div>{content}</div>
}

/// A fragment in an attribute island: a component prop of type `Element`.
#[component]
fn InProp() -> Element {
    <Wrap content={<><b>p</b><i>q</i></>} />
}

#[component]
fn Table() -> Element {
    <table><tbody><Rows /><Rows /></tbody></table>
}

/// Keys on the roots of a fragment and on its items in a map.
#[component]
fn Keyed() -> Element {
    let items = [("a", 1u8), ("b", 2)];
    <>
        <h1 key="title">Items</h1>
        {items.iter().map(|(id, n)| <><dt key={*id}>{*id}</dt><dd>{*n}</dd></>)}
    </>
}

/// A key after the first root is not given to the backend, which would warn
/// about it; Outou warns instead, and the generated code stays clean.
#[component]
fn KeyIgnored() -> Element {
    let items = [("a", 1u8)];
    <>
        <h1>Items</h1>
        {items.iter().map(|(id, n)| <><dt>{*id}</dt><dd key={format!("v-{id}")}>{*n}</dd></>)}
    </>
}

#[component]
fn SelectInFragment() -> Element {
    let levels = [1u8, 2, 3];
    <select value={2u8}>
        <>
            <option value="1">one</option>
            <option value={2u8}>two</option>
        </>
        <optgroup label="more">
            <>{levels.iter().map(|level| <><option value={*level}>{*level}</option></>)}</>
        </optgroup>
    </select>
}

#[component]
fn SelectMapOfFragments() -> Element {
    let levels = [1u8, 2, 3];
    <select value={3u8}>
        {levels.iter().map(|level| <><option value={*level}>{*level}</option></>)}
    </select>
}

/// The static part of a rendered node as an HTML-like string; a dynamic
/// text is its value, a dynamic list its nodes in brackets.
fn show(node: &VNode) -> String {
    node.template
        .roots
        .iter()
        .map(|root| show_template(node, root))
        .collect()
}

fn show_template(node: &VNode, template: &TemplateNode) -> String {
    match template {
        TemplateNode::Element { tag, children, .. } => {
            let inner: String = children.iter().map(|c| show_template(node, c)).collect();
            format!("<{tag}>{inner}</{tag}>")
        }
        TemplateNode::Text { text } => text.to_string(),
        TemplateNode::Dynamic { id } => match &node.dynamic_nodes[*id] {
            DynamicNode::Text(text) => format!("{{{}}}", text.value),
            DynamicNode::Fragment(nodes) => {
                let inner: Vec<String> = nodes.iter().map(|n| format!("[{}]", show(n))).collect();
                inner.join("")
            }
            DynamicNode::Component(component) => format!("<{}/>", component.name.rsplit("::").next().unwrap_or_default()),
            DynamicNode::Placeholder(_) => "(placeholder)".to_string(),
        },
    }
}

fn rendered(root: fn() -> Element) -> (usize, String) {
    let mut dom = VirtualDom::new(root);
    dom.rebuild_in_place();
    let node = dom.get_scope(ScopeId::APP).expect("root scope").root_node();
    (node.template.roots.len(), show(node))
}

/// The keys of the list items in the first dynamic list of the root node, and
/// the key of the root node itself: a node is keyed by its first root.
fn item_keys(root: fn() -> Element) -> (Option<String>, Vec<Option<String>>) {
    let mut dom = VirtualDom::new(root);
    dom.rebuild_in_place();
    let node = dom.get_scope(ScopeId::APP).expect("root scope").root_node();
    let items = node
        .dynamic_nodes
        .iter()
        .find_map(|dynamic| match dynamic {
            DynamicNode::Fragment(nodes) => Some(nodes.iter().map(|n| n.key.clone()).collect()),
            _ => None,
        })
        .expect("a dynamic list");
    (node.key.clone(), items)
}

fn selected_flags(root: fn() -> Element) -> Vec<bool> {
    let mut dom = VirtualDom::new(root);
    dom.rebuild_to_vec()
        .edits
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

fn main() {
    assert_eq!(
        rendered(TwoRoots),
        (2, "<tr><td>a</td></tr><tr><td>b</td></tr>".to_string()),
        "a top-level fragment is several roots"
    );
    assert_eq!(rendered(SingleChild), (1, "<p>x</p>".to_string()));
    // The backend renders "nothing" as one placeholder node (a comment in
    // the DOM), as it does for any empty call: no element, no text.
    assert_eq!(
        rendered(Empty),
        (1, "(placeholder)".to_string()),
        "an empty fragment renders nothing"
    );
    assert_eq!(rendered(Mixed), (3, "<b>x</b>tail {7}".to_string()), "{:?}", rendered(Mixed));
    assert_eq!(
        rendered(Spliced),
        (1, "<ul><li>a</li><li>b</li><li>c</li><li>d</li></ul>".to_string()),
        "a fragment child adds no node of its own"
    );
    assert_eq!(
        rendered(InMap),
        (1, "<ul>[<li>{1}</li><li>!</li>][<li>{2}</li><li>!</li>]</ul>".to_string()),
        "a fragment as the body of a map closure"
    );
    assert_eq!(
        rendered(InAttributeIsland),
        (1, "<div>[<i>p</i><i>q</i>]</div>".to_string()),
    );
    assert_eq!(
        rendered(Table),
        (1, "<table><tbody><Rows/><Rows/></tbody></table>".to_string()),
    );
    assert_eq!(
        VirtualDom::new(InProp)
            .rebuild_to_vec()
            .edits
            .iter()
            .filter(|edit| matches!(edit, Mutation::LoadTemplate { .. }))
            .count(),
        3,
        "the wrapper div and the prop's two roots"
    );
    assert_eq!(rendered(Rows).0, 2, "a component with several roots");
    let loads = VirtualDom::new(Table)
        .rebuild_to_vec()
        .edits
        .iter()
        .filter(|edit| matches!(edit, Mutation::LoadTemplate { .. }))
        .count();
    assert_eq!(loads, 5, "the table, and both roots of both `Rows`");
    assert_eq!(
        item_keys(Keyed),
        (Some("title".to_string()), vec![Some("a".to_string()), Some("b".to_string())]),
        "an item is keyed by the key of its first root"
    );
    assert_eq!(
        item_keys(KeyIgnored),
        (None, vec![None]),
        "a key on a later root is not the item's key"
    );
    assert_eq!(
        selected_flags(SelectInFragment),
        [false, true, false, true, false],
        "options in a fragment, in a fragment in an optgroup"
    );
    assert_eq!(
        selected_flags(SelectMapOfFragments),
        [false, false, true],
        "a select whose options come from a map of fragments"
    );
}
"#;

const REJECTED: &str = r#"use outou::prelude::*;

#[component]
fn App() -> Element {
    let ok = String::from("a");
    <>
        <p>{ok}</p>
        <p>{vec![1u8]}</p>
    </>
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
fn fragments_render_with_the_expected_roots_and_children() {
    let output = run("outou-fragment-accepted", ACCEPTED, "run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(!has_compiler_warning(&stderr), "{stderr}");
}

#[test]
#[ignore = "builds the full dioxus dependency tree; run explicitly, see module docs"]
fn the_accepted_shapes_pass_clippy_without_warnings() {
    let output = run_with(
        "outou-fragment-accepted",
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
fn a_child_of_an_unsupported_type_inside_a_fragment_is_reported_in_outou_words() {
    let output = run("outou-fragment-rejected", REJECTED, "check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("cannot be rendered as a JSX child"),
        "the typed child message must survive a fragment:\n{stderr}"
    );
}
