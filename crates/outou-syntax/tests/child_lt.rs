//! A `<` in element content that begins no tag (issue #24).
//!
//! It used to become an incomplete tag with no diagnostic, which broke the
//! parser's invariant that every incomplete tag is diagnosed: `outou check`
//! said "no errors" and a Strict build generated a recovery element.
//!
//! Trivia between `<` and the tag name is accepted in child position as at
//! the top level (grammar §4 rule 2, §2.1), and so is trivia between the
//! `<` and the `/` of a closing tag; `a < b` text keeps its friendly
//! diagnostic through a recovery heuristic that never changes what a valid
//! file means.

use outou_syntax::{ast, Severity};

fn errors(body: &str) -> Vec<String> {
    let source = format!("fn app() -> Element {{\n    {body}\n}}\n");
    outou_syntax::parse(&source)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message)
        .collect()
}

#[test]
fn lt_without_a_tag_name_in_child_position_is_an_error() {
    for body in [
        "<div>a < b</div>",
        "<div><3></div>",
        "<div><<A/></div>",
        "<div>< 3></div>",
        "<div>x <</div>",
    ] {
        assert_eq!(
            errors(body),
            ["expected a tag name after `<`; write `{\"<\"}` to include a literal `<` in text"],
            "input: {body}"
        );
    }
}

#[test]
fn valid_comparisons_and_nested_elements_stay_clean() {
    for body in [
        "<div>{a < b}</div>",
        "<div>{if a < b { <p/> } else { <q/> }}</div>",
        "<div><span>a</span><b/>{x < 3}</div>",
        "<div class={if a < b { \"x\" } else { \"y\" }}>text &lt; more</div>",
        "<Foo<u8> />",
    ] {
        let found = errors(body);
        assert!(
            !found.iter().any(|m| m.contains("tag name after")),
            "input: {body}: {found:?}"
        );
    }
    assert!(errors("<div>{a < b}</div>").is_empty());
    assert!(errors("<div><span>a</span><b/>{x < 3}</div>").is_empty());
}

#[test]
fn an_html_comment_in_child_position_points_at_the_jsx_comment() {
    assert_eq!(
        errors("<div><!-- note --></div>"),
        ["expected a tag name after `<`; write a comment as `{/* ... */}`"]
    );
}

const LT_MESSAGE: &str =
    "expected a tag name after `<`; write `{\"<\"}` to include a literal `<` in text";

/// `name(child,child)` for the first element of the first function's tail.
fn shape(body: &str) -> String {
    fn element(e: &ast::JsxElement) -> String {
        let name = match &e.open {
            ast::JsxTag::Named { name, .. } => name.name.clone(),
            other => format!("{other:?}"),
        };
        let kids: Vec<String> = e
            .children
            .iter()
            .map(|c| match c {
                ast::JsxChild::Element(n) => element(n),
                ast::JsxChild::Text(t) => format!("{:?}", t.value),
                other => format!("{other:?}"),
            })
            .collect();
        format!("{name}({})", kids.join(","))
    }
    let source = format!("fn app() -> Element {{\n    {body}\n}}\n");
    let parsed = outou_syntax::parse(&source);
    let ast::Item::Function(f) = &parsed.file.items[0] else {
        panic!("no function")
    };
    let Some(ast::Expr::Jsx(e)) = f.body.tail.as_deref() else {
        panic!("no JSX tail in {body:?}")
    };
    element(e)
}

#[test]
fn trivia_between_lt_and_the_tag_name_is_accepted_in_child_position() {
    let plain = shape("<div><span/></div>");
    for body in [
        "<div>< span/></div>",
        "<div><\n  span/></div>",
        "<div><\t\r\n span/></div>",
        "<div>< /* c */ span/></div>",
        "<div><// c\n span/></div>",
        "<div>< span\n  class=\"x\"\n/></div>",
    ] {
        assert!(errors(body).is_empty(), "input: {body}: {:?}", errors(body));
        assert_eq!(shape(body), plain, "input: {body}");
    }
}

#[test]
fn trivia_in_closing_tags_is_accepted_in_child_position() {
    for body in [
        "<div>< span>x</ span></div>",
        "<div><span>x</span ></div>",
        "<div><span>x< /span></div>",
        "<div><span>x<\n/\nspan\n></div>",
        "<div><span>x< /* c */ /span></div>",
        "<div><span>x</ /* c */ span></div>",
        "<div><span>x</ /* > */ span></div>",
        "<div>< span>x< / span ></div>",
    ] {
        assert!(errors(body).is_empty(), "input: {body}: {:?}", errors(body));
        assert_eq!(shape(body), "div(span(\"x\"))", "input: {body}");
    }
}

#[test]
fn a_comment_right_after_lt_is_trivia_so_it_is_not_a_closing_tag() {
    // Babel reads `</* c */ span>` as `<`, a comment and an opening `span`.
    assert_eq!(errors("<div></* c */ span/></div>"), Vec::<String>::new());
}

#[test]
fn the_name_span_of_a_spaced_tag_excludes_the_trivia() {
    let source = "fn app() -> Element {\n    <div>< span/></div>\n}\n";
    let parsed = outou_syntax::parse(source);
    let ast::Item::Function(f) = &parsed.file.items[0] else {
        panic!("no function")
    };
    let Some(ast::Expr::Jsx(div)) = f.body.tail.as_deref() else {
        panic!("no JSX tail")
    };
    let ast::JsxChild::Element(span) = &div.children[0] else {
        panic!("no child element")
    };
    let ast::JsxTag::Named { name, .. } = &span.open else {
        panic!("not a named tag")
    };
    assert_eq!(
        &source[name.span.start as usize..name.span.end as usize],
        "span"
    );
    assert_eq!(
        &source[span.span.start as usize..span.span.end as usize],
        "< span/>"
    );
}

#[test]
fn literal_lt_in_text_still_gets_the_friendly_message() {
    for body in [
        "<div>a < b</div>",
        "<div>if x < y then</div>",
        "<div>x < y and y < z</div>",
        "<div>a < b",
        "<div>1 < 2 }",
        "<div>x <</div>",
        "<div>a </* c */ b</div>",
    ] {
        let found = errors(body);
        assert_eq!(
            found.first().map(String::as_str),
            Some(LT_MESSAGE),
            "input: {body}: {found:?}"
        );
    }
}

#[test]
fn lt_with_no_space_before_a_name_keeps_the_tag_diagnostics() {
    assert_eq!(
        errors("<div>a<b c</div>"),
        ["unexpected `<` inside tag `<b>`, expected an attribute or `>`"]
    );
}

#[test]
fn a_spaced_tag_with_generic_arguments_is_diagnosed_as_generics() {
    assert_eq!(
        errors("<div>< Foo<u8> /></div>"),
        ["generic arguments on a tag are not supported"]
    );
}

#[test]
fn a_spaced_tag_whose_attributes_contain_angle_brackets_stays_valid() {
    for body in [
        "<div>< span class={a < b} /></div>",
        "<div>< span title=\"a > b\" /></div>",
        "<div>< span a={if x < 1 { 1 } else { 2 }}>t</span></div>",
    ] {
        assert!(errors(body).is_empty(), "input: {body}: {:?}", errors(body));
    }
}

#[test]
fn a_spaced_closing_tag_that_does_not_match_is_diagnosed() {
    assert_eq!(
        errors("<div>< span></ b></div>"),
        ["closing tag `</b>` does not match opening tag `<span>`"]
    );
}

#[test]
fn text_with_a_later_greater_than_reads_as_a_tag_as_in_react() {
    // `< b and c >` reaches a `>`: it is a tag head, so this is an unclosed
    // element, not the literal-`<` message (documented in grammar §9).
    assert_eq!(
        errors("<div>a < b and c > d</div>"),
        ["missing closing tag `</b>`"]
    );
}

#[test]
fn a_spaced_fragment_is_a_fragment() {
    assert!(errors("<div>< >x</ ></div>").is_empty());
    assert!(errors("<div><\n></></div>").is_empty());
    assert_eq!(errors("<div>< ></div>"), ["missing closing tag `</>`"]);
}

#[test]
fn a_spaced_tag_with_an_attribute_assignment_stays_a_tag_when_unfinished() {
    for body in [
        "<div>< span class={a < b}</div>",
        "<div>< span class=\"x\"</div>",
    ] {
        assert_eq!(
            errors(body),
            ["unexpected `<` inside tag `<span>`, expected an attribute or `>`"],
            "input: {body}"
        );
    }
}

#[test]
fn prose_after_a_spaced_lt_keeps_the_friendly_message() {
    for body in [
        "<div>a < b = c</div>",
        "<div>if a < b { x } else { y }</div>",
        "<div>while n < max { n += 1 }</div>",
        "<div>Use x < y = \"z\" here</div>",
        "<div>when n < limit then say \"hi\"</div>",
        "<div>x < y z = w</div>",
        "<div>< Us</div>",
        "<div>< 3</div>",
    ] {
        let found = errors(body);
        assert_eq!(found, [LT_MESSAGE], "input: {body}: {found:?}");
    }
}

#[test]
fn a_spaced_lt_before_a_slash_that_is_not_a_complete_closing_tag_is_text() {
    for body in [
        "<div><p>ratio a < /b</p><i>ok</i></div>",
        "<div>a < /b and c > d</div>",
        "<div>a < / </div>",
    ] {
        let found = errors(body);
        assert_eq!(found, [LT_MESSAGE], "input: {body}: {found:?}");
    }
    assert_eq!(
        shape("<div><p>x</p><i>ok< /i></div>"),
        "div(p(\"x\"),i(\"ok\"))"
    );
}

#[test]
fn a_spaced_slash_gt_in_text_is_a_literal_lt_but_fragment_closes_still_close() {
    // `< />` has trivia between `<` and `/`, and no name: prose, not a close.
    assert_eq!(errors("<div>a < /> b</div>"), [LT_MESSAGE]);
    // `</>` and `</ >` keep closing a fragment.
    assert!(errors("<div><>a</></div>").is_empty());
    assert!(errors("<div><>a</ ></div>").is_empty());
}
