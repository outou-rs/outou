//! Fragments, `<>…</>` (ADR 0015): the AST shape, where they may appear,
//! the closing-tag rules and the diagnostics for the broken shapes.

use outou_syntax::{ast, Severity};

fn parse(body: &str) -> outou_syntax::Parsed {
    outou_syntax::parse(&format!("fn app() -> Element {{\n    {body}\n}}\n"))
}

fn errors(body: &str) -> Vec<String> {
    parse(body)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

/// The element the function's tail expression is.
fn tail_element(parsed: &outou_syntax::Parsed) -> &ast::JsxElement {
    let ast::Item::Function(function) = &parsed.file.items[0] else {
        panic!("expected a function: {:#?}", parsed.file.items);
    };
    let Some(ast::Expr::Jsx(element)) = function.body.tail.as_deref() else {
        panic!("expected a JSX tail: {:#?}", function.body);
    };
    element
}

fn is_fragment(element: &ast::JsxElement) -> bool {
    matches!(element.open, ast::JsxTag::Fragment { .. })
}

fn text(parsed: &outou_syntax::Parsed, span: outou_sourcemap::Span) -> &str {
    &parsed.source[span.start as usize..span.end as usize]
}

#[test]
fn a_top_level_fragment_has_fragment_tags_and_its_children() {
    let parsed = parse("<>\n        <p>a</p>\n        <p>b</p>\n    </>");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let fragment = tail_element(&parsed);
    let ast::JsxTag::Fragment { span: open } = &fragment.open else {
        panic!("{:?}", fragment.open);
    };
    let Some(ast::JsxTag::Fragment { span: close }) = &fragment.close else {
        panic!("{:?}", fragment.close);
    };
    assert_eq!(text(&parsed, *open), "<>");
    assert_eq!(text(&parsed, *close), "</>");
    assert!(fragment.attributes.is_empty());
    assert_eq!(fragment.children.len(), 2);
}

#[test]
fn an_empty_fragment_is_legal_and_has_no_children() {
    for body in ["<></>", "<>\n    </>", "<>\n\n    </>"] {
        let parsed = parse(body);
        assert!(
            parsed.diagnostics.is_empty(),
            "{body}: {:?}",
            parsed.diagnostics
        );
        let fragment = tail_element(&parsed);
        assert!(is_fragment(fragment));
        assert!(fragment.children.is_empty(), "{body}");
    }
}

#[test]
fn a_fragment_may_be_a_child_a_nested_fragment_and_a_single_child_fragment() {
    let parsed = parse("<ul><><li>a</li></><li>b</li><><></></></ul>");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ul = tail_element(&parsed);
    let kinds: Vec<bool> = ul
        .children
        .iter()
        .map(|child| match child {
            ast::JsxChild::Element(e) => is_fragment(e),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(kinds, [true, false, true]);
    let ast::JsxChild::Element(last) = &ul.children[2] else {
        unreachable!()
    };
    assert_eq!(last.children.len(), 1);
}

#[test]
fn a_fragment_may_be_an_attribute_island_and_a_map_closure_body() {
    let parsed =
        parse("<List items={xs.iter().map(|x| <><b>{x}</b><i/></>).collect::<Vec<_>>()} />");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let fragments = parsed
        .file
        .jsx_elements()
        .into_iter()
        .filter(|e| is_fragment(e))
        .count();
    assert_eq!(fragments, 1);

    let parsed = parse("<ul>{xs.iter().map(|x| <><li>{x}</li><li>!</li></>)}</ul>");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let all = parsed.file.jsx_elements();
    assert_eq!(all.iter().filter(|e| is_fragment(e)).count(), 1);
    assert_eq!(all.len(), 4, "ul, fragment and its two items");
}

#[test]
fn a_fragment_is_found_by_the_flat_traversal_with_its_children() {
    let parsed = parse("<><p><>x</></p></>");
    let all = parsed.file.jsx_elements();
    assert_eq!(all.iter().filter(|e| is_fragment(e)).count(), 2);
    assert_eq!(all.len(), 3);
}

#[test]
fn trivia_inside_the_fragment_tags_is_skipped() {
    for body in [
        "< >x</ >",
        "<\n>x</\n>",
        "< /* c */ >x</ /* c */ >",
        "<div>< >x</ ></div>",
        "<div><// c\n>x</ // c\n></div>",
    ] {
        assert_eq!(errors(body), Vec::<String>::new(), "{body}");
    }
    let parsed = parse("< >x</ >");
    let fragment = tail_element(&parsed);
    assert!(is_fragment(fragment));
    assert!(matches!(fragment.close, Some(ast::JsxTag::Fragment { .. })));
}

#[test]
fn attributes_after_the_angle_bracket_are_text_as_in_react() {
    let parsed = parse("<> class=\"a\"</>");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let fragment = tail_element(&parsed);
    assert!(fragment.attributes.is_empty());
    assert!(matches!(
        fragment.children.as_slice(),
        [ast::JsxChild::Text(_)]
    ));
}

#[test]
fn a_fragment_closed_by_a_named_tag_reports_a_mismatch() {
    assert_eq!(
        errors("<>\n        <p/>\n    </div>"),
        ["closing tag `</div>` does not match opening tag `<>`"]
    );
}

#[test]
fn an_element_closed_by_the_fragment_close_reports_a_mismatch() {
    assert_eq!(
        errors("<div>\n        <p/>\n    </>"),
        ["closing tag `</>` does not match opening tag `<div>`"]
    );
}

#[test]
fn a_mismatch_inside_a_fragment_leaves_the_fragment_open_for_its_own_close() {
    assert_eq!(
        errors("<><div></span></>"),
        ["closing tag `</span>` does not match opening tag `<div>`"]
    );
}

#[test]
fn a_fragment_close_that_belongs_to_an_outer_fragment_ends_the_inner_element() {
    // `</>` closes the outer fragment, so the unclosed `<div>` is the one
    // reported missing, and the fragment itself still closes.
    assert_eq!(errors("<><div></>"), ["missing closing tag `</div>`"]);
}

#[test]
fn an_unclosed_fragment_is_reported_once_at_its_opening_angle_bracket() {
    let parsed = parse("<>\n        <p/>\n");
    let found: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|d| d.message == "missing closing tag `</>`")
        .collect();
    assert_eq!(found.len(), 1, "{:?}", parsed.diagnostics);
    assert_eq!(
        found[0].span.start as usize,
        parsed.source.find("<>").unwrap()
    );
}

#[test]
fn an_unclosed_fragment_inside_an_element_is_closed_by_the_elements_tag() {
    assert_eq!(errors("<div><><p/></div>"), ["missing closing tag `</>`"]);
}

#[test]
fn a_stray_fragment_close_has_no_matching_opening_tag() {
    assert_eq!(
        errors("</>"),
        ["closing tag `</>` has no matching opening tag"]
    );
    assert_eq!(
        errors("<div></></div>"),
        [
            "closing tag `</>` does not match opening tag `<div>`",
            "closing tag `</div>` has no matching opening tag"
        ]
    );
}

#[test]
fn a_keyed_fragment_attempt_gets_one_clear_message() {
    let found = errors("<key={x}>\n        <p/>\n    </key>");
    assert_eq!(
        found,
        ["unexpected `=` after the tag name `key`: a fragment `<>` takes no attributes, so it has no `key`; put the `key` on an element"]
    );
    // Only for a tag literally named `key`; other tags keep the general rule.
    assert_eq!(
        errors("<div=\"a\"></div>")[0],
        "unexpected `=` inside tag `<div>`, expected an attribute or `>`"
    );
}

#[test]
fn a_named_component_called_fragment_is_an_ordinary_tag() {
    assert!(errors("<Fragment key={k}><p/></Fragment>").is_empty());
}

#[test]
fn fragments_count_against_the_nesting_cap_and_never_overflow_the_stack() {
    let depth = 400;
    let body = format!("{}x{}", "<>".repeat(depth), "</>".repeat(depth));
    let found = errors(&body);
    assert!(
        found.iter().any(|m| m.contains("nested too deeply")),
        "{found:?}"
    );
}

#[test]
fn the_fragment_tag_spans_are_inside_the_element_span() {
    let parsed = parse("<div><>x</></div>");
    let div = tail_element(&parsed);
    let ast::JsxChild::Element(fragment) = &div.children[0] else {
        panic!("{:?}", div.children);
    };
    let ast::JsxTag::Fragment { span: open } = &fragment.open else {
        panic!()
    };
    let Some(ast::JsxTag::Fragment { span: close }) = &fragment.close else {
        panic!()
    };
    assert_eq!(fragment.span.start, open.start);
    assert_eq!(fragment.span.end, close.end);
    assert_eq!(text(&parsed, fragment.span), "<>x</>");
}

#[test]
fn a_mismatched_fragment_close_is_an_unnamed_incomplete_tag() {
    // The tag the user wrote is `</>`, which has no name to recover.
    let parsed = parse("<div>\n        <p/>\n    </>");
    let div = tail_element(&parsed);
    let Some(ast::JsxTag::Incomplete(incomplete)) = &div.close else {
        panic!("{:?}", div.close);
    };
    assert!(incomplete.name.is_none(), "{incomplete:?}");
    // And a named mismatch keeps the name it saw.
    let parsed = parse("<>\n        <p/>\n    </div>");
    let fragment = tail_element(&parsed);
    let Some(ast::JsxTag::Incomplete(incomplete)) = &fragment.close else {
        panic!("{:?}", fragment.close);
    };
    assert_eq!(
        incomplete.name.as_ref().map(|n| n.name.as_str()),
        Some("div")
    );
}
