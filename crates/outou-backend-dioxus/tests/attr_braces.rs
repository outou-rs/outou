//! Issue #31: when does an attribute island keep its braces?
//!
//! A single Rust expression is spliced as written; anything Rust reads as
//! more than one statement, even without a `;` (`if c {} "x"`), keeps its
//! braces, so it is the block it is everywhere else in an island (it is a
//! valid block whose value is the last item) and never reaches the backend as
//! a bare, ill-formed expression. Mirrors `child_braces.rs`.

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

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

/// The attribute value as the backend receives it.
fn lowered(island: &str) -> String {
    let source = format!("fn f() -> Element {{\n    <div title={{{island}}} />\n}}\n");
    let rust = generate(&source);
    let start = rust.find("title: ").expect("an attribute") + "title: ".len();
    let end = rust[start..].find(", } }").expect("end of attribute") + start;
    rust[start..end].trim().to_string()
}

#[test]
fn statement_shapes_without_a_semicolon_keep_their_braces() {
    for island in [
        r#"if c {} "x""#,
        r#"while c {} "x""#,
        r#"loop { break } "x""#,
        r#"match v { _ => {} } "x""#,
        r#"unsafe { f() } "x""#,
        r#"for i in 0..2 {} "x""#,
        r#"{ 1 } "x""#,
        r#"fn h() -> &'static str { "x" } h()"#,
    ] {
        assert_eq!(lowered(island), format!("{{{island}}}"), "{island}");
    }
}

#[test]
fn single_expressions_are_still_spliced_without_braces() {
    for island in [
        "x",
        "if c { 1 } else { 2 }",
        "match v { _ => 1 }",
        "if c { 1 } else { 2 } + 3",
        "{ 1 }.max(2)",
        "S { a: 1 }",
        "move |_| { go() }",
    ] {
        assert_eq!(lowered(island), island, "{island}");
    }
}

#[test]
fn a_semicolon_keeps_the_braces() {
    assert_eq!(lowered("let a = 1; a"), "{let a = 1; a}");
}
