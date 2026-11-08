//! How a fragment (`<>…</>`, ADR 0015) is lowered, on the generated text:
//! several roots at the top, spliced children below, nothing for an empty
//! one, and the key and `<select value>` rules reaching through it. The
//! generated code is compiled and rendered by `fragment_compile.rs`.

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

fn lower(body: &str) -> String {
    let source = format!("fn App() -> Element {{\n    {body}\n}}\n");
    let parsed = outou_syntax::parse(&source);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| d.severity == outou_syntax::Severity::Warning),
        "{:?}",
        parsed.diagnostics
    );
    let options = GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"));
    DioxusBackend
        .generate(&parsed, &source, Mode::Strict, &options)
        .expect("strict generation")
        .rust
}

/// The generated `App` body, whitespace-normalised to single spaces.
fn body(rust: &str) -> String {
    let start = rust.find("fn App() -> Element {").unwrap();
    rust[start..]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_top_level_fragment_becomes_several_roots_in_one_call() {
    let rust = lower("<><a/><b>x</b></>");
    assert!(
        body(&rust).contains("rsx! { a { }, b { \"x\", }, } }"),
        "{rust}"
    );
    assert_eq!(rust.matches("rsx!").count(), 1, "{rust}");
}

#[test]
fn text_and_expressions_are_roots_of_a_top_level_fragment() {
    let rust = lower("<>hi {name}</>");
    assert!(
        body(&rust).contains("rsx! { \"hi \", {::outou::__private::child(name)}, } }"),
        "{rust}"
    );
}

#[test]
fn an_empty_fragment_is_an_empty_call_and_adds_nothing_as_a_child() {
    assert!(body(&lower("<></>")).contains("rsx! { } }"));
    let rust = lower("<ul><></><li>a</li><><></></></ul>");
    assert!(body(&rust).contains("ul { li { \"a\", }, } }"), "{rust}");
}

#[test]
fn a_fragment_child_is_spliced_at_any_depth_and_leaves_no_trace() {
    let rust = lower("<ul><><li>a</li><><li>b</li></></><li>c</li></ul>");
    assert!(
        body(&rust).contains("ul { li { \"a\", }, li { \"b\", }, li { \"c\", }, } }"),
        "{rust}"
    );
    assert!(!rust.contains("Fragment"), "{rust}");
}

#[test]
fn a_single_child_fragment_is_its_child() {
    assert!(body(&lower("<><p>x</p></>")).contains("rsx! { p { \"x\", }, } }"));
}

#[test]
fn a_fragment_in_a_map_closure_is_a_call_of_its_own() {
    let rust = lower("<ul>{xs.map(|x| <><li>{x}</li><li>!</li></>)}</ul>");
    assert!(
        body(&rust).contains(
            "|x| ::outou::__private::rsx! { li { {::outou::__private::child(x)}, }, li { \"!\", }, }"
        ),
        "{rust}"
    );
}

#[test]
fn only_the_key_on_the_first_root_is_passed_on() {
    let rust = lower("<><i key=\"a\"/><i key={k}/></>");
    let text = body(&rust);
    assert!(
        text.contains("i { key: \"{__outou_key_0}\", }, i { }, }"),
        "{rust}"
    );
    // The ignored key is still evaluated and checked.
    assert!(
        text.contains("let __outou_key_1 = ::outou::__private::key(&(k));"),
        "{rust}"
    );
    assert!(!text.contains("{__outou_key_1}"), "{rust}");
}

#[test]
fn keys_below_a_root_and_on_spliced_children_follow_the_same_rule() {
    // `<b>` is the second root of the call once the nested fragment is
    // spliced; `<li>`s inside `<ul>` are not roots.
    let rust = lower("<><a key=\"1\"/><><b key=\"2\"/></><ul><li key=\"3\"/></ul></>");
    let text = body(&rust);
    assert!(text.contains("a { key: \"{__outou_key_0}\", }"), "{rust}");
    assert!(!text.contains("{__outou_key_1}"), "{rust}");
    assert!(text.contains("li { key: \"{__outou_key_2}\", }"), "{rust}");
}

#[test]
fn an_option_inside_a_fragment_inside_a_select_is_selected_by_its_value() {
    let rust = lower("<select value={v}><><option value=\"a\">A</option></></select>");
    assert!(rust.contains("selected: __outou_selected_"), "{rust}");
}

#[test]
fn an_option_in_a_map_of_fragments_inside_a_select_is_selected_by_its_value() {
    let rust =
        lower("<select value={v}>{xs.map(|x| <><option value={x}>{x}</option></>)}</select>");
    assert!(rust.contains("selected: __outou_selected_"), "{rust}");
}
