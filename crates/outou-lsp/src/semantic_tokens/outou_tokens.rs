//! Outou's own AST-derived semantic tokens: component name, HTML element
//! name (both the opening AND the closing tag), attribute name, event
//! attribute, and JSX text — everything rust-analyzer cannot know, because
//! it only ever sees the *generated* Rust (`docs/phase0/issues/
//! 14-semantic-tokens-rename.md`). Pure AST walk, byte spans only; no
//! legend, no line index, no rust-analyzer needed (see this module's own
//! tests) — [`super::token::span_to_line_tokens`] converts the result to
//! LSP-shaped tokens once a legend and a [`outou_sourcemap::LineIndex`]
//! are available.

use outou_sourcemap::Span;
use outou_syntax::ast::{self, JsxTag};

use super::legend::{ATTRIBUTE_TYPE, COMPONENT_TYPE, ELEMENT_TYPE, EVENT_TYPE, TEXT_TYPE};

/// One span this module classifies, named by the legend type it belongs
/// to (a `&'static str`, looked up by [`super::legend::Legend::type_index`]
/// once a legend is chosen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedToken {
    pub span: Span,
    pub type_name: &'static str,
    pub modifiers: u32,
}

/// Every Outou-native token in `file`.
pub fn collect(file: &ast::File) -> Vec<NamedToken> {
    let mut out = Vec::new();
    for element in file.jsx_elements() {
        collect_element_tokens(element, &mut out);
    }
    out
}

fn collect_element_tokens(element: &ast::JsxElement, out: &mut Vec<NamedToken>) {
    if let Some(token) = tag_name_token(&element.open) {
        out.push(token);
    }
    if let Some(close) = &element.close {
        if let Some(token) = tag_name_token(close) {
            out.push(token);
        }
    }
    for attribute in &element.attributes {
        out.push(attribute_name_token(&attribute.name));
    }
    for child in &element.children {
        if let ast::JsxChild::Text(text) = child {
            if !text.value.trim().is_empty() {
                out.push(NamedToken {
                    span: text.span,
                    type_name: TEXT_TYPE,
                    modifiers: 0,
                });
            }
        }
        // `JsxChild::Element`/`Expression` are already reached by
        // `File::jsx_elements()`'s own traversal (it descends into both),
        // so nothing further is collected for them here.
    }
}

fn tag_name_token(tag: &JsxTag) -> Option<NamedToken> {
    let JsxTag::Named { name, .. } = tag else {
        // An incomplete tag (no name recovered yet) has nothing to token.
        return None;
    };
    Some(NamedToken {
        span: name.span,
        type_name: if is_component_name(&name.name) {
            COMPONENT_TYPE
        } else {
            ELEMENT_TYPE
        },
        modifiers: 0,
    })
}

fn attribute_name_token(name: &ast::Ident) -> NamedToken {
    NamedToken {
        span: name.span,
        type_name: if is_event_attribute(&name.name) {
            EVENT_TYPE
        } else {
            ATTRIBUTE_TYPE
        },
        modifiers: 0,
    }
}

/// A tag name beginning with an uppercase letter is a component reference
/// (grammar §5); everything else is an intrinsic HTML element.
fn is_component_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// `on*` attribute names are event handlers (`onClick`, `oninput`, …) —
/// matched by prefix alone, the same lightweight rule the TextMate grammar
/// uses (`packages/vscode-outou/syntaxes/outou-rsx.tmLanguage.json`'s
/// `jsx-event-attribute`), not a fixed vocabulary of known DOM events.
fn is_event_attribute(name: &str) -> bool {
    name.len() > 2 && name.starts_with("on")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> ast::File {
        outou_syntax::parse(source).file
    }

    #[test]
    fn tokens_a_component_name_on_both_the_opening_and_closing_tag() {
        let file = parse("fn f() { <Greeting>hi</Greeting> }");
        let tokens = collect(&file);
        let component_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.type_name == COMPONENT_TYPE)
            .collect();
        assert_eq!(component_tokens.len(), 2, "{tokens:#?}");
    }

    #[test]
    fn tokens_an_html_element_name_as_the_element_type() {
        let file = parse("fn f() { <div>hi</div> }");
        let tokens = collect(&file);
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.type_name == ELEMENT_TYPE)
                .count(),
            2
        );
    }

    #[test]
    fn tokens_a_plain_attribute_as_attribute_and_an_on_attribute_as_event() {
        let file = parse(r#"fn f() { <button onClick={handler} class="x"></button> }"#);
        let tokens = collect(&file);
        assert_eq!(
            tokens.iter().filter(|t| t.type_name == EVENT_TYPE).count(),
            1
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.type_name == ATTRIBUTE_TYPE)
                .count(),
            1
        );
    }

    #[test]
    fn tokens_non_blank_jsx_text() {
        let file = parse("fn f() { <p>Hello</p> }");
        let tokens = collect(&file);
        assert_eq!(
            tokens.iter().filter(|t| t.type_name == TEXT_TYPE).count(),
            1
        );
    }

    #[test]
    fn does_not_token_blank_whitespace_only_text() {
        let file = parse("fn f() { <div>\n    <span>x</span>\n</div> }");
        let tokens = collect(&file);
        // Only "x" is non-blank text; the whitespace between the elements
        // is normalized away by the parser or is blank and skipped here.
        assert_eq!(
            tokens.iter().filter(|t| t.type_name == TEXT_TYPE).count(),
            1
        );
    }

    #[test]
    fn tokens_a_nested_component_reached_through_an_island() {
        let file = parse("fn f() { <div>{ if true { <Greeting /> } else { <div /> } }</div> }");
        let tokens = collect(&file);
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.type_name == COMPONENT_TYPE)
                .count(),
            1
        );
    }
}
