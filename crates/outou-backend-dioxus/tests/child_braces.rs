//! Issue #27: when does a child expression keep its braces?
//!
//! `{::outou::__private::child(EXPR)}` takes a single expression as written
//! (no braces of its own, so Rust's `unused_braces` stays quiet and temporaries
//! live to the end of the enclosing statement, 2024 edition). Anything Rust
//! parses as more than one statement, though `;`-free, must keep its braces:
//! `child({ … })`.

use outou_backend_dioxus::DioxusBackend;
use outou_codegen::{Backend, GenerateOptions, Mode};
use outou_sourcemap::Uri;

fn child_call(island: &str) -> String {
    let source = format!("fn f() -> Element {{\n    <p>{{{island}}}</p>\n}}\n");
    let parsed = outou_syntax::parse(&source);
    let rust = DioxusBackend
        .generate(
            &parsed,
            &source,
            Mode::Strict,
            &GenerateOptions::new(
                Uri::new("file:///gen/main.rs"),
                Uri::new("file:///src/main.rsx"),
            ),
        )
        .expect("generation succeeds")
        .rust;
    let start = rust.find("child(").expect("a child call") + "child(".len();
    let end = rust.rfind(")}").expect("the end of the call");
    rust[start..end].to_string()
}

fn is_braced(island: &str) -> bool {
    child_call(island) == format!("{{{island}}}")
}

#[test]
fn statement_shapes_without_a_semicolon_keep_their_braces() {
    let cases = [
        r#" if debug { println!("render") } "x" "#,
        r#"if ready {} "done""#,
        r#"while c {} "done""#,
        r#"loop { break } "done""#,
        r#"match v { _ => {} } "done""#,
        r#"fn h() -> &'static str { "x" } h()"#,
        r#"unsafe { f() } "done""#,
        r#"for i in 0..2 {} "done""#,
        r#"{ 1 } "done""#,
        r#"'a: { break 'a } "done""#,
        r#"if c { 1 } else { 2 } "done""#,
        r#"if c {} -1"#,
        r#"struct S; "x""#,
        r#"const N: usize = 1; N"#,
        r#"const fn k() -> usize { 1 } k()"#,
        r#"use std::fmt::Write; "x""#,
    ];
    for island in cases {
        assert!(is_braced(island), "{island} should keep its braces");
    }
}

#[test]
fn single_expressions_are_passed_as_written() {
    let cases = [
        r#""a;{}""#,
        r#"';'"#,
        r##"r#"x;}"#"##,
        "1 /* ; { */",
        "1 // ; {\n",
        "'a: { if open { break 'a 1 } 2 }",
        "unsafe { f() }",
        "loop { break 4 }",
        "match count { _ => 5 }",
        "if let Some(n) = m { n } else { 0 }",
        "(|x: usize| { x + 1 })(count)",
        "|x: usize| { x + 1 }",
        "vec![1; 3]",
        r#"format!("{};", count)"#,
        "#[allow(unused_variables)] count",
        "if c { <a/> } else { <b/> }",
        "match v { 0 => <a/>, _ => <b/> }",
        "if c { 1 } else { 2 }.max(3)",
        "match v { _ => 1 }.to_string()",
        "if c { 1 } else { 2 } as u8",
        "S { a: 1 }",
        "S { a: 1 } == S { a: 1 } && c",
        "a == match v { _ => 1 }",
        "if let Some(n) = m { n } else { 0 }?",
        "{ 1 }.to_string()",
        "count",
        "const { 1 }",
        "{ if c { let a = 1; a } else { 2 } }.to_string()",
    ];
    for island in cases {
        assert!(!is_braced(island), "{island} should be passed as written");
    }
}

#[test]
fn a_semicolon_keeps_the_braces() {
    for island in ["let a = 1; a", "1;", "if c { 1 } else { 2 }; 3"] {
        assert!(is_braced(island), "{island:?} should keep its braces");
    }
}
