//! Extra tokens after the name of a closing tag (`</div class>`, `</div/>`):
//! the first name is kept, the extra tokens are diagnosed at their own
//! span, and the tag still closes its element so no mismatch cascade
//! follows. React (Babel, TypeScript) rejects every one of these inputs.

use outou_syntax::{ast, Severity};

fn parse(body: &str) -> outou_syntax::Parsed {
    outou_syntax::parse(&format!("fn app() -> Element {{\n    {body}\n}}\n"))
}

/// Each error as `(message, source text of its span)`.
fn errors(parsed: &outou_syntax::Parsed) -> Vec<(String, String)> {
    parsed
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| {
            (
                d.message.clone(),
                parsed.source[d.span.start as usize..d.span.end as usize].to_string(),
            )
        })
        .collect()
}

fn extra_message(extra: &str, name: &str) -> String {
    format!(
        "unexpected `{extra}` inside closing tag `</{name}>`; a closing tag holds only the tag name"
    )
}

fn tail_element(parsed: &outou_syntax::Parsed) -> &ast::JsxElement {
    let ast::Item::Function(function) = &parsed.file.items[0] else {
        panic!("expected a function: {:#?}", parsed.file.items);
    };
    let Some(ast::Expr::Jsx(element)) = function.body.tail.as_deref() else {
        panic!("expected a JSX tail: {:#?}", function.body);
    };
    element
}

fn assert_single_extra(body: &str, extra: &str, name: &str) {
    let parsed = parse(body);
    assert_eq!(
        errors(&parsed),
        vec![(extra_message(extra, name), extra.to_string())],
        "{body}"
    );
}

#[test]
fn a_bare_word_is_diagnosed_and_the_first_name_is_kept() {
    assert_single_extra("<div>x</div class>", "class", "div");
    assert_single_extra("<div><span>x</span class></div>", "class", "span");
}

#[test]
fn several_words_are_one_diagnostic_naming_all_of_them() {
    assert_single_extra("<div><span>x</span foo bar></div>", "foo bar", "span");
}

#[test]
fn an_attribute_is_diagnosed() {
    assert_single_extra(r#"<div>x</div class="x">"#, r#"class="x""#, "div");
}

#[test]
fn a_slash_is_diagnosed_for_self_closing_style_closers() {
    assert_single_extra("<div>x</div/>", "/", "div");
    assert_single_extra("<div>x</div / >", "/", "div");
}

#[test]
fn a_fragment_closer_with_a_stray_slash_keeps_the_empty_name() {
    assert_single_extra("<>x</ / >", "/", "");
    assert_single_extra("<>x</ />", "/", "");
}

#[test]
fn a_name_after_the_name_of_a_fragment_closer_still_mismatches_the_fragment() {
    let parsed = parse("<>x</ x y>");
    assert_eq!(
        errors(&parsed),
        vec![
            (extra_message("y", "x"), "y".to_string()),
            (
                "closing tag `</x>` does not match opening tag `<>`".to_string(),
                "<".to_string()
            ),
        ]
    );
}

#[test]
fn a_stray_byte_before_the_name_is_diagnosed() {
    assert_single_extra("<div>x</=div>", "=", "div");
}

#[test]
fn the_tag_still_closes_its_element_and_the_tree_stays_intact() {
    let parsed = parse("<div><span>x</span class></div>");
    assert_eq!(errors(&parsed).len(), 1, "{:?}", parsed.diagnostics);
    let div = tail_element(&parsed);
    assert!(matches!(div.close, Some(ast::JsxTag::Named { .. })));
    let ast::JsxChild::Element(span) = &div.children[0] else {
        panic!("expected the nested span: {:#?}", div.children);
    };
    let Some(ast::JsxTag::Named { name, .. }) = &span.close else {
        panic!("span lost its closing tag: {:#?}", span.close);
    };
    assert_eq!(name.name, "span");
}

#[test]
fn a_closer_for_an_ancestor_reports_its_extra_tokens_once() {
    let parsed = parse("<div><span>x</div class>");
    let found = errors(&parsed);
    let extras: Vec<_> = found
        .iter()
        .filter(|(message, _)| message == &extra_message("class", "div"))
        .collect();
    assert_eq!(extras.len(), 1, "{found:?}");
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, "missing closing tag `</span>`");
}

fn extra_count(body: &str) -> usize {
    errors(&parse(body))
        .iter()
        .filter(|(message, _)| message.contains("holds only the tag name"))
        .count()
}

#[test]
fn an_ancestor_closer_across_an_island_reports_its_extra_tokens_once() {
    assert_eq!(extra_count("<div>{<li>x</div class>}</div>"), 1);
    assert_eq!(extra_count("<div class={<li>x</div class>}>y</div>"), 1);
}

#[test]
fn a_spaced_closer_with_extra_tokens_is_read_as_text() {
    let found = errors(&parse("<div>x< /div class></div>"));
    assert_eq!(extra_count("<div>x< /div class></div>"), 0, "{found:?}");
    assert!(
        found[0].0.starts_with("expected a tag name after `<`"),
        "{found:?}"
    );
}

#[test]
fn extras_on_both_sides_of_the_name_are_quoted_together_at_the_first() {
    let parsed = parse("<div>x</=div class></div>");
    let found = errors(&parsed);
    assert_eq!(found[0], (extra_message("= class", "div"), "=".to_string()));
}

#[test]
fn the_dotted_name_diagnostic_wins_over_further_extra_tokens() {
    assert_eq!(
        errors(&parse("<a>x</a.b c>")),
        vec![(
            "dotted tag names are not supported".to_string(),
            ".".to_string()
        )]
    );
}

#[test]
fn a_stray_closer_with_extra_tokens_reports_both_problems() {
    let parsed = parse("</div class>");
    assert_eq!(
        errors(&parsed),
        vec![
            (extra_message("class", "div"), "class".to_string()),
            (
                "closing tag `</div>` has no matching opening tag".to_string(),
                "<".to_string()
            ),
        ]
    );
}

#[test]
fn a_mismatched_closer_with_extra_tokens_reports_both_problems() {
    let parsed = parse("<div>x</span class></div>");
    let messages: Vec<_> = errors(&parsed).into_iter().map(|(m, _)| m).collect();
    assert_eq!(
        messages,
        vec![
            extra_message("class", "span"),
            "closing tag `</span>` does not match opening tag `<div>`".to_string(),
            "closing tag `</div>` has no matching opening tag".to_string(),
        ]
    );
}

#[test]
fn a_long_or_multiline_extra_is_quoted_cut_down() {
    let long = "a".repeat(60);
    let parsed = parse(&format!("<div>x</div {long}>"));
    let found = errors(&parsed);
    assert_eq!(
        found[0].0,
        extra_message(&format!("{}…", "a".repeat(40)), "div")
    );
    assert_eq!(found[0].1, long, "the span still covers all of it");

    let parsed = parse("<div>x</div foo\n bar>");
    assert_eq!(errors(&parsed)[0].0, extra_message("foo…", "div"));
}

#[test]
fn a_non_ascii_extra_is_quoted_whole() {
    assert_single_extra("<div>x</div クラス>", "クラス", "div");
}

#[test]
fn a_comment_after_the_name_is_extra_tokens() {
    let parsed = parse("<div>x</div /* c */>");
    assert_eq!(
        errors(&parsed),
        vec![(extra_message("/* c */", "div"), "/* c */".to_string())]
    );
}

#[test]
fn truncated_closing_tags_keep_their_own_diagnostics() {
    let rbrace = errors(&parse("<div>x</div {x}>"));
    assert_eq!(
        rbrace[0].0,
        "unexpected `}` inside closing tag, expected `>`"
    );
    let lt = errors(&parse("<div>x</div class<b/>"));
    assert_eq!(lt[0].0, "unexpected `<` inside closing tag, expected `>`");
    let eof = errors(&outou_syntax::parse("fn f() { <div>x</div class"));
    assert_eq!(
        eof[0].0,
        "unexpected end of file inside closing tag, expected `>`"
    );
    assert!(
        eof.iter().all(|(m, _)| !m.contains("holds only")),
        "{eof:?}"
    );
}

#[test]
fn dotted_and_namespaced_closing_names_get_the_reserved_name_diagnostic() {
    let dotted = errors(&parse("<a>x</a.b>"));
    assert_eq!(
        dotted,
        vec![(
            "dotted tag names are not supported".to_string(),
            ".".to_string()
        )]
    );
    let namespaced = errors(&parse("<a>x</a:b>"));
    assert_eq!(
        namespaced,
        vec![(
            "namespaced names are not supported".to_string(),
            ":".to_string()
        )]
    );
}

#[test]
fn valid_closing_tags_are_unchanged() {
    for body in [
        "<div>x</div>",
        "<div>x</div >",
        "<div>x</ div>",
        "<div>x< /div>",
        "<div>x</\n div\n>",
        "<div>x</ /* c */ div>",
        "<>x</>",
        "<> x </ >",
        "<x-y>x</x-y>",
    ] {
        let parsed = parse(body);
        assert!(
            errors(&parsed).is_empty(),
            "{body}: {:?}",
            parsed.diagnostics
        );
    }
}
