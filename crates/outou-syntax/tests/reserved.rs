//! Reserved JSX syntax (grammar §10) must be diagnosed identically no
//! matter how deep the construct is nested (M5): a fragment or a tag with
//! explicit generic arguments used to be silently mishandled once nested
//! inside another element, instead of producing the same diagnostic (and
//! only that diagnostic) it produces at the top level.

fn diagnostics_matching(source: &str, message: &str) -> usize {
    let parsed = outou_syntax::parse(source);
    parsed
        .diagnostics
        .iter()
        .filter(|d| d.message == message)
        .count()
}

fn all_messages(source: &str) -> Vec<String> {
    outou_syntax::parse(source)
        .diagnostics
        .into_iter()
        .map(|d| d.message)
        .collect()
}

struct Case {
    label: &'static str,
    snippet: &'static str,
    message: String,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            label: "fragment",
            snippet: "<></>",
            message: "fragments are not supported in Phase 0".to_string(),
        },
        Case {
            label: "generic-arguments",
            snippet: "<List<T> />",
            message: "generic arguments on a tag are not supported in Phase 0".to_string(),
        },
        Case {
            label: "dotted-tag-name",
            snippet: "<Foo.Bar />",
            message: "dotted tag names are not supported in Phase 0".to_string(),
        },
        Case {
            label: "namespaced-name",
            snippet: "<svg:rect />",
            message: "namespaced names are not supported in Phase 0".to_string(),
        },
        Case {
            label: "spread-attribute",
            snippet: "<A {...p} />",
            message: "spread attributes are not supported in Phase 0".to_string(),
        },
        Case {
            label: "self-component-name",
            snippet: "<Self />",
            message: "`Self` is not a valid component name".to_string(),
        },
        Case {
            label: "single-quoted-attribute-value",
            snippet: "<div title='x' />",
            message: "attribute values must be double-quoted strings or `{…}` expressions"
                .to_string(),
        },
    ]
}

#[test]
fn reserved_syntax_is_diagnosed_at_every_nesting_level() {
    for case in cases() {
        let top_level = format!("fn f() {{ {}; }}", case.snippet);
        let nested = format!("fn f() {{ <root>{}</root>; }}", case.snippet);

        let top_count = diagnostics_matching(&top_level, &case.message);
        assert_eq!(
            top_count,
            1,
            "{}: top-level: expected exactly 1 `{}`, got {:?}",
            case.label,
            case.message,
            all_messages(&top_level)
        );

        let nested_count = diagnostics_matching(&nested, &case.message);
        assert_eq!(
            nested_count,
            1,
            "{}: nested: expected exactly 1 `{}`, got {:?}",
            case.label,
            case.message,
            all_messages(&nested)
        );
    }
}

// ---------------------------------------------------------------------
// Item 14 (M7): postfix on a JSX expression must be rejected even across
// trivia (whitespace, comments) between the element and the postfix
// token — grammar §7.1's `check_postfix_on_jsx` used to look at the very
// next byte only, missing every case with intervening trivia.
// ---------------------------------------------------------------------

const POSTFIX_MESSAGE: &str =
    "a JSX expression cannot be followed by `.`, `?`, `(` or `[`; parenthesize it";

#[test]
fn postfix_is_rejected_across_trivia() {
    let rejected = [
        "fn f() {\n    <A/> .into()\n}",
        "fn f() {\n    <A/> /* c */ (x)\n}",
        "fn f() {\n    <A/>\n    ?\n}",
    ];
    for source in rejected {
        let count = diagnostics_matching(source, POSTFIX_MESSAGE);
        assert_eq!(count, 1, "source: {source}: {:?}", all_messages(source));
    }

    let accepted = ["fn f() {\n    <A/> ;\n}", "fn f() { <A/> }"];
    for source in accepted {
        let count = diagnostics_matching(source, POSTFIX_MESSAGE);
        assert_eq!(count, 0, "source: {source}: {:?}", all_messages(source));
    }
}

// ---------------------------------------------------------------------
// Item 18 (M8): a stray, uncatalogued byte skipped inside a tag must
// leave behind a lossless `ErrorNode`, not just a diagnostic — otherwise
// nothing in the AST records that a byte was ever there.
// ---------------------------------------------------------------------

use outou_syntax::ast;

fn find_jsx_element(file: &ast::File) -> Option<&ast::JsxElement> {
    for item in &file.items {
        if let ast::Item::Function(f) = item {
            for expr in f.body.statements.iter().chain(f.body.tail.as_deref()) {
                if let ast::Expr::Jsx(el) = expr {
                    return Some(el);
                }
            }
        }
    }
    None
}

#[test]
fn unexpected_byte_in_tag_produces_an_error_node() {
    let source = "fn f() { <div!>; }";
    // Byte offsets: '<' at 9, "div" 10..13, '!' at 13.
    assert_eq!(&source[13..14], "!");
    let parsed = outou_syntax::parse(source);
    let element = find_jsx_element(&parsed.file).expect("jsx element");
    assert_eq!(
        element.errors,
        vec![ast::ErrorNode {
            span: outou_sourcemap::Span::new(13, 14),
            expected: None,
        }],
        "{:?}",
        element.errors
    );
    let matching = parsed
        .diagnostics
        .iter()
        .filter(|d| d.message.contains('!') && d.message.contains("div"))
        .count();
    assert_eq!(matching, 1, "{:?}", parsed.diagnostics);
}
