//! A `<` in element content that begins no tag (issue #24).
//!
//! It used to become an incomplete tag with no diagnostic, which broke the
//! parser's invariant that every incomplete tag is diagnosed: `outou check`
//! said "no errors" and a Strict build generated a recovery element.

use outou_syntax::Severity;

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
        "<div>< span/></div>",
        "<div><<A/></div>",
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
