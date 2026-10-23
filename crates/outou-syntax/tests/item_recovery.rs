//! Item-boundary recovery for broken functions (issue #18, gate A2;
//! grammar §2.2 and §9): a function that is cut short must not swallow the
//! items after it, must always be diagnosed, and the cut must be token
//! aware (an item-looking line inside a string, raw string, block comment
//! or JSX text is not an item) and linear in the input size.

use std::time::{Duration, Instant};

use outou_syntax::{ast, Parsed};

const AFTER: &str = "\n#[component]\nfn After() -> Element {\n    <p>after</p>\n}\n";

fn top_level_functions(parsed: &Parsed) -> Vec<&str> {
    parsed
        .file
        .items
        .iter()
        .filter_map(|item| match item {
            ast::Item::Function(f) => Some(f.name.name.as_str()),
            _ => None,
        })
        .collect()
}

fn rust_item_texts<'a>(parsed: &'a Parsed, source: &'a str) -> Vec<&'a str> {
    parsed
        .file
        .items
        .iter()
        .filter_map(|item| match item {
            ast::Item::Rust(r) => Some(&source[r.span.start as usize..r.span.end as usize]),
            _ => None,
        })
        .collect()
}

fn messages(parsed: &Parsed) -> Vec<&str> {
    parsed
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect()
}

fn assert_cut_after_docs(source: &str) {
    let parsed = outou_syntax::parse(source);
    assert_eq!(
        top_level_functions(&parsed),
        ["Docs", "After"],
        "{:#?}",
        parsed.file
    );
    assert!(
        messages(&parsed)
            .iter()
            .any(|m| m.starts_with("unclosed body of function `Docs`")),
        "{:?}",
        messages(&parsed)
    );
    // The first parse's diagnostics survive the cut (the broken `<pre`).
    assert!(parsed.diagnostics.len() >= 2, "{:?}", messages(&parsed));
}

#[test]
fn raw_string_with_item_looking_lines_is_not_a_cut() {
    let source = format!(
        "#[component]\nfn Docs() -> Element {{\n    let sample = r#\"\nfn main() {{\n    println!(\"hi\");\n}}\n\"#;\n    <pre class={{\n}}\n{AFTER}"
    );
    assert_cut_after_docs(&source);
}

#[test]
fn string_with_item_looking_lines_is_not_a_cut() {
    let source = format!(
        "#[component]\nfn Docs() -> Element {{\n    let sample = \"\nfn main() {{}}\n\";\n    <pre class={{\n}}\n{AFTER}"
    );
    assert_cut_after_docs(&source);
}

#[test]
fn block_comment_with_item_looking_lines_is_not_a_cut() {
    let source = format!(
        "#[component]\nfn Docs() -> Element {{\n    /*\nfn old() {{}}\n*/\n    <pre class={{\n}}\n{AFTER}"
    );
    assert_cut_after_docs(&source);
}

#[test]
fn item_text_before_the_first_error_is_not_a_cut() {
    // Column-0 `fn sample` inside well-formed JSX text, then the real error
    // (the first diagnostic comes after the decoy).
    let source = format!(
        "#[component]\nfn Docs() -> Element {{\n    let a = <pre>\nfn sample\n</pre>;\n    <p class={{\n}}\n{AFTER}"
    );
    let parsed = outou_syntax::parse(&source);
    assert_eq!(
        top_level_functions(&parsed),
        ["Docs", "After"],
        "{:#?}",
        parsed.file
    );
}

#[test]
fn missing_body_before_the_next_item_is_diagnosed_and_cut() {
    let source = format!("fn App() -> Element\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["App", "After"]);
    assert_eq!(
        messages(&parsed),
        ["incomplete signature for function `App`, expected a body or `;`"]
    );
}

#[test]
fn mismatched_close_in_a_module_ends_the_signature_without_eating_the_brace() {
    let source = format!("mod m {{\n    fn App(\n}}\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["After"]);
    let Some(ast::Item::Module(module)) = parsed.file.items.first() else {
        panic!("{:#?}", parsed.file);
    };
    let inner = module.items.as_ref().expect("inline module");
    assert!(
        matches!(inner.as_slice(), [ast::Item::Function(f)] if f.name.name == "App"),
        "{inner:#?}"
    );
    assert_eq!(
        messages(&parsed),
        ["incomplete signature for function `App`, expected `)` to close this `(`"]
    );
}

#[test]
fn stray_closing_brace_does_not_stop_item_detection() {
    let source = format!("fn App() {{}}\n}}\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["App", "After"]);
}

#[test]
fn items_between_a_broken_signature_and_the_next_function_survive() {
    let source = format!("fn App(\n\nstruct S;\n\nuse a::b;\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["App", "After"]);
    let runs = rust_item_texts(&parsed, &source).join("|");
    assert!(
        runs.contains("struct S;") && runs.contains("use a::b;"),
        "{runs:?}"
    );
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", messages(&parsed));
}

#[test]
fn items_between_a_broken_body_and_the_next_function_survive() {
    let source = format!("fn App() -> Element {{\n    <div>\n\nstruct S;\n\nuse a::b;\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["App", "After"]);
    let runs = rust_item_texts(&parsed, &source).join("|");
    assert!(
        runs.contains("struct S;") && runs.contains("use a::b;"),
        "{runs:?}"
    );
}

#[test]
fn const_and_impl_heads_cut_a_broken_body_but_not_a_signature() {
    let source = "fn App() -> Element {\n    <div>\n\nconst N: usize = 1;\nimpl S {}\n";
    let parsed = outou_syntax::parse(source);
    assert_eq!(top_level_functions(&parsed), ["App"]);
    let runs = rust_item_texts(&parsed, source).join("|");
    assert!(
        runs.contains("const N") && runs.contains("impl S"),
        "{runs:?}"
    );

    // Valid generics with a column-0 `const` parameter stay one signature.
    let valid = "fn f<\nconst N: usize>() {}\nfn g() {}\n";
    let parsed = outou_syntax::parse(valid);
    assert_eq!(top_level_functions(&parsed), ["f", "g"]);
    assert!(parsed.diagnostics.is_empty(), "{:?}", messages(&parsed));
}

#[test]
fn unclosed_body_without_other_diagnostics_is_cut_and_diagnosed() {
    let source = format!("fn App() -> Element {{\n    if true {{\n        let x = 1;\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(
        top_level_functions(&parsed),
        ["App", "After"],
        "{:#?}",
        parsed.file
    );
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", messages(&parsed));
    let diag = &parsed.diagnostics[0];
    assert_eq!(
        diag.message,
        "unclosed body of function `App`, expected `}` to close this `{`"
    );
    // Points at the body's opening brace.
    assert_eq!(&source[diag.span.start as usize..][..1], "{");
    assert_eq!(diag.span.start as usize, source.find('{').unwrap());
}

#[test]
fn unclosed_last_function_without_a_following_item_is_left_to_rustc() {
    let parsed = outou_syntax::parse("fn App() -> Element {\n    if true {\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", messages(&parsed));

    // Indented item heads are never a boundary.
    let parsed = outou_syntax::parse("fn App() -> Element {\n    if true {\n    fn inner() {}\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", messages(&parsed));
    assert_eq!(top_level_functions(&parsed), ["App"]);
}

#[test]
fn bodyless_declaration_is_not_an_unclosed_body() {
    let parsed = outou_syntax::parse("fn Decl(x: i32);\n\nfn After() {}\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", messages(&parsed));
    assert_eq!(top_level_functions(&parsed), ["Decl", "After"]);
}

#[test]
fn cut_replaces_end_of_file_wording_when_the_file_continues() {
    let source = format!("fn App() -> Element {{\n    <div class={{\n}}\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert!(
        !messages(&parsed).iter().any(|m| m.contains("end of file")),
        "{:?}",
        messages(&parsed)
    );
    let real_eof = outou_syntax::parse("fn App() -> Element {\n    <div class=");
    assert!(
        messages(&real_eof)
            .iter()
            .any(|m| m.contains("end of file")),
        "{:?}",
        messages(&real_eof)
    );
}

#[test]
fn cuts_per_file_are_capped() {
    let source = "fn a() {\n    <p>x</q>\n".repeat(40);
    let parsed = outou_syntax::parse(&source);
    // 16 cuts, then the 17th broken function falls back to swallowing the
    // rest of the file (with its own diagnostics).
    assert_eq!(top_level_functions(&parsed).len(), 17);
    assert!(!parsed.diagnostics.is_empty());
}

fn assert_parses_within(label: &str, source: &str) {
    let start = Instant::now();
    let parsed = outou_syntax::parse(source);
    let elapsed = start.elapsed();
    assert!(!parsed.file.items.is_empty());
    assert!(
        elapsed < Duration::from_secs(3),
        "{label}: {} bytes took {elapsed:?}; expected linear time",
        source.len()
    );
}

#[test]
fn large_runs_after_a_broken_function_parse_in_linear_time() {
    let head = "fn App() -> Element {\n    <div class={\n";
    let tail = "}\n#[component]\nfn After() -> Element {\n    <p/>\n}\n";
    assert_parses_within(
        "blank lines",
        &format!("{head}{}{tail}", "\n".repeat(160_000)),
    );
    assert_parses_within(
        "line comments",
        &format!("{head}{}{tail}", "// x\n".repeat(40_000)),
    );
    assert_parses_within(
        "doc comments",
        &format!("{head}{}{tail}", "/// x\n".repeat(32_000)),
    );
    assert_parses_within(
        "doc comments in a signature",
        &format!("fn App(\n{}{tail}", "/// x\n".repeat(32_000)),
    );
}

#[test]
fn many_broken_functions_parse_in_linear_time() {
    assert_parses_within("broken signatures", &"fn a(\n".repeat(16_000));
    assert_parses_within("broken bodies", &"fn a() {\n    <p>x</q>\n".repeat(4_000));
}

#[test]
fn truncated_top_level_closing_tag_names_the_first_word() {
    let parsed = outou_syntax::parse("fn App() -> Element {\n    </div class");
    assert!(
        messages(&parsed).contains(&"closing tag `</div>` has no matching opening tag"),
        "{:?}",
        messages(&parsed)
    );
}

#[test]
fn comments_between_a_fragment_and_its_truncated_closing_tag_are_skipped() {
    let parsed = outou_syntax::parse("fn App() -> Element {\n    <>/* c */</");
    assert!(
        messages(&parsed).contains(&"unexpected end of file inside closing tag, expected `>`"),
        "{:?}",
        messages(&parsed)
    );
}

#[test]
fn mismatched_closing_tag_diagnostic_survives_the_cut() {
    let source = format!("fn a() -> Element {{\n    <p>x</q>\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["a", "After"]);
    let all = messages(&parsed);
    assert!(
        all.contains(&"closing tag `</q>` does not match opening tag `<p>`"),
        "{all:?}"
    );
    assert!(
        all.iter()
            .any(|m| m.starts_with("unclosed body of function `a`")),
        "{all:?}"
    );
}

#[test]
fn mismatched_closing_tag_then_extra_close_braces_keep_the_next_function() {
    // The mismatch clamps the open-element depth; the stray `}` after the
    // body must not hide the item after it.
    let source = format!("fn a() -> Element {{\n    <div></span>\n}}\n}}\n{AFTER}");
    let parsed = outou_syntax::parse(&source);
    assert_eq!(top_level_functions(&parsed), ["a", "After"]);
    assert!(
        messages(&parsed).contains(&"closing tag `</span>` does not match opening tag `<div>`"),
        "{:?}",
        messages(&parsed)
    );
}

#[test]
fn use_precise_capturing_bound_in_a_signature_is_not_an_item_head() {
    let source = "fn f<'a>(x: &'a u8) -> impl Sized +\nuse<'a> {\n    x\n}\n\nfn g() {}\n";
    let parsed = outou_syntax::parse(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", messages(&parsed));
    assert_eq!(top_level_functions(&parsed), ["f", "g"]);

    // A real import at column 0 still stops a broken signature.
    let parsed = outou_syntax::parse("fn f(\nuse a::b;\nfn g() {}\n");
    assert_eq!(top_level_functions(&parsed), ["f", "g"]);
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", messages(&parsed));
}

#[test]
fn restricted_visibility_forms_still_head_an_item() {
    for vis in [
        "pub(crate)",
        "pub(super)",
        "pub(self)",
        "pub(in a::b)",
        "pub",
    ] {
        let source = format!("fn f(\n{vis} fn g() {{}}\n");
        let parsed = outou_syntax::parse(&source);
        assert_eq!(top_level_functions(&parsed), ["f", "g"], "{vis}");
    }
}

#[test]
fn interrupted_closing_tags_consume_the_following_element() {
    for source in [
        "fn A() -> Element {\n    </div <p/>\n}\n",
        "fn A() -> Element {\n    </ <p/>\n}\n",
        "fn A() -> Element {\n    <></div <p/>\n}\n",
    ] {
        let parsed = outou_syntax::parse(source);
        let Some(ast::Item::Function(f)) = parsed.file.items.first() else {
            panic!("{:#?}", parsed.file);
        };
        // The recovery element spans through `<p/>`; nothing is left as a
        // bare `<` operand for the caller.
        let last_end = f
            .body
            .statements
            .iter()
            .chain(f.body.tail.iter().map(|tail| &**tail))
            .filter_map(|e| match e {
                ast::Expr::Jsx(j) => Some(j.span.end as usize),
                _ => None,
            })
            .max()
            .expect("a recovery element");
        assert_eq!(&source[last_end - 4..last_end], "<p/>", "{source:?}");
    }
}

#[test]
fn many_unclosed_visibility_groups_parse_in_linear_time() {
    let groups = "pub(\n".repeat(20_000);
    assert_parses_within("signature then pub(", &format!("fn f(\n{groups}"));
    assert_parses_within(
        "body then pub(",
        &format!("fn f() {{\n    <div class={{\n{groups}"),
    );
    assert_parses_within("top-level pub(", &groups);
}
