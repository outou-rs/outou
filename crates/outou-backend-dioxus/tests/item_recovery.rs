//! End-to-end contract for item-boundary recovery (issue #18, grammar §9):
//! for every broken-function shape, `Mode::Strict` is refused (the parser
//! diagnosed the shape), `Mode::Recovery` output parses as Rust, and the
//! items after the broken construct survive in it.

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, Error, GenerateOptions, Mode};
use outou_sourcemap::Uri;

const AFTER: &str = "\n#[component]\nfn After() -> Element {\n    <p>after</p>\n}\n";

fn options() -> GenerateOptions {
    GenerateOptions::new(Uri::new("file:///gen.rs"), Uri::new("file:///a.rsx"))
}

fn assert_refused_in_strict_and_recovered(label: &str, source: &str, survivors: &[&str]) {
    let parsed = outou_syntax::parse(source);
    assert!(
        !parsed.diagnostics.is_empty(),
        "{label}: the broken shape must be diagnosed"
    );
    assert!(
        matches!(
            DioxusBackend.generate(&parsed, source, Mode::Strict, &options()),
            Err(Error::SyntaxErrors { .. })
        ),
        "{label}: Strict generation must be refused"
    );
    let generated = DioxusBackend
        .generate(&parsed, source, Mode::Recovery, &options())
        .unwrap_or_else(|e| panic!("{label}: recovery generation must never fail: {e}"));
    syn::parse_file(&generated.rust).unwrap_or_else(|e| {
        panic!(
            "{label}: recovery output does not parse as Rust: {e}\n---\n{}",
            generated.rust
        )
    });
    for survivor in survivors {
        assert!(
            generated.rust.contains(survivor),
            "{label}: {survivor:?} is missing from recovery output:\n{}",
            generated.rust
        );
    }
}

#[test]
fn item_looking_text_in_a_string_raw_string_or_comment_does_not_hide_the_error() {
    let raw = format!(
        "#[component]\nfn Docs() -> Element {{\n    let sample = r#\"\nfn main() {{\n    println!(\"hi\");\n}}\n\"#;\n    <pre class={{\n}}\n{AFTER}"
    );
    let string = format!(
        "#[component]\nfn Docs() -> Element {{\n    let sample = \"\nfn main() {{}}\n\";\n    <pre class={{\n}}\n{AFTER}"
    );
    let comment = format!(
        "#[component]\nfn Docs() -> Element {{\n    /*\nfn old() {{}}\n*/\n    <pre class={{\n}}\n{AFTER}"
    );
    for (label, source) in [("raw", raw), ("string", string), ("comment", comment)] {
        assert_refused_in_strict_and_recovered(
            label,
            &source,
            &["fn Docs()", "fn After() -> Element {"],
        );
    }
}

#[test]
fn signatures_that_stop_at_the_next_item_are_refused_and_recovered() {
    let cases = [
        ("missing body", format!("fn App() -> Element\n{AFTER}")),
        (
            "mod closes an open paren",
            format!("mod m {{\n    fn App(\n}}\n{AFTER}"),
        ),
        (
            "struct and use between",
            format!("fn App(\n\nstruct S;\n\nuse a::b;\n{AFTER}"),
        ),
    ];
    for (label, source) in cases {
        assert_refused_in_strict_and_recovered(label, &source, &["fn After() -> Element {"]);
    }
}

#[test]
fn unclosed_bodies_are_refused_and_keep_the_next_items() {
    let cases = [
        (
            // Only one block is open: the closing `}` codegen synthesizes
            // balances it. A nested unclosed Rust block stays rustc's.
            "no other diagnostic",
            format!("fn App() -> Element {{\n    let x = 1;\n{AFTER}"),
        ),
        (
            "struct and use between",
            format!("fn App() -> Element {{\n    <div>\n\nstruct S;\n\nuse a::b;\n{AFTER}"),
        ),
    ];
    for (label, source) in cases {
        assert_refused_in_strict_and_recovered(label, &source, &["fn After() -> Element {"]);
    }
}

#[test]
fn precise_capturing_use_bound_is_valid_rust_and_undiagnosed() {
    let source = "fn f<'a>(x: &'a u8) -> impl Sized +\nuse<'a> {\n    x\n}\n\nfn g() {}\n";
    let parsed = outou_syntax::parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let generated = DioxusBackend
        .generate(&parsed, source, Mode::Recovery, &options())
        .expect("recovery generation never fails");
    syn::parse_file(&generated.rust).unwrap_or_else(|e| panic!("{e}\n{}", generated.rust));
}

#[test]
fn closing_tags_interrupted_by_a_following_element_stay_valid_rust() {
    let cases = [
        ("stray div", "fn A() -> Element {\n    </div <p/>\n}\n"),
        ("stray bare", "fn A() -> Element {\n    </ <p/>\n}\n"),
        ("fragment", "fn A() -> Element {\n    <></div <p/>\n}\n"),
        ("stray rbrace", "fn A() -> Element {\n    </div\n}\n"),
        ("fragment rbrace", "fn A() -> Element {\n    <></div\n}\n"),
    ];
    for (label, source) in cases {
        assert_refused_in_strict_and_recovered(label, source, &["fn A()"]);
    }
}
