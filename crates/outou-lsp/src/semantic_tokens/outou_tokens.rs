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

use super::legend::{
    ATTRIBUTE_TYPE, COMPONENT_TYPE, ELEMENT_TYPE, EVENT_TYPE, FRAGMENT_PUNCTUATION_TYPE, TEXT_TYPE,
};

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
pub fn collect(file: &ast::File, source: &str) -> Vec<NamedToken> {
    let mut out = Vec::new();
    for element in file.jsx_elements() {
        collect_element_tokens(element, source, &mut out);
    }
    out
}

fn collect_element_tokens(element: &ast::JsxElement, source: &str, out: &mut Vec<NamedToken>) {
    if let Some(token) = tag_name_token(&element.open) {
        out.push(token);
    }
    push_fragment_punctuation(&element.open, source, out);
    if let Some(close) = &element.close {
        if let Some(token) = tag_name_token(close) {
            out.push(token);
        }
        push_fragment_punctuation(close, source, out);
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

/// `<>` and `</>` have no name, so their punctuation is what gets a token.
/// A plain tag is `<>` or `</>` (two or three bytes) and is one token; a
/// spaced one (`< >`, `<!-- -->`-free trivia between) is its first and last
/// byte, so trivia inside it, a comment for one, keeps its own colour.
fn push_fragment_punctuation(tag: &JsxTag, source: &str, out: &mut Vec<NamedToken>) {
    let JsxTag::Fragment { span } = tag else {
        return;
    };
    let token = |span| NamedToken {
        span,
        type_name: FRAGMENT_PUNCTUATION_TYPE,
        modifiers: 0,
    };
    if span.end - span.start <= 3 {
        out.push(token(*span));
        return;
    }
    out.push(token(Span::new(span.start, span.start + 1)));
    // The `/` of a closing tag is the first `/` outside trivia, which is
    // the first `/` after the `<` unless a comment sits in between.
    if let Some(slash) = closing_slash(source, *span) {
        out.push(token(Span::new(slash, slash + 1)));
    }
    out.push(token(Span::new(span.end - 1, span.end)));
}

/// The offset of the `/` of the closing tag `span` (`</>`, `< / >`), or
/// `None` for an opening tag. Trivia between `<` and `/` is skipped the way
/// the parser skips it.
fn closing_slash(source: &str, span: Span) -> Option<u32> {
    let bytes = source.as_bytes();
    let from = span.start as usize + 1;
    let at = outou_syntax::lexer::rust_token::next_significant(bytes, from).start;
    (at < span.end as usize && bytes.get(at) == Some(&b'/')).then_some(at as u32)
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
        type_name: if is_event_attribute(&ast::unraw_name(&name.name)) {
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

    fn tokens_of(source: &str) -> Vec<NamedToken> {
        collect(&parse(source), source)
    }

    fn parse(source: &str) -> ast::File {
        crate::documents::parse_rsx(source).file
    }

    #[test]
    fn tokens_the_punctuation_of_a_fragment_and_nothing_else_of_it() {
        let source = "fn f() { <><p>x</p></> }";
        let tokens = collect(&parse(source), source);
        let punctuation: Vec<&str> = tokens
            .iter()
            .filter(|t| t.type_name == FRAGMENT_PUNCTUATION_TYPE)
            .map(|t| &source[t.span.start as usize..t.span.end as usize])
            .collect();
        assert_eq!(punctuation, ["<>", "</>"], "{tokens:#?}");
        // The `<p>` inside is tokened as ever.
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.type_name == ELEMENT_TYPE)
                .count(),
            2
        );
    }

    #[test]
    fn a_spaced_fragment_tag_is_tokened_byte_by_byte_around_its_trivia() {
        let source = "fn f() { < /* c */ >x</ /* c */ > }";
        let tokens = collect(&parse(source), source);
        let punctuation: Vec<&str> = tokens
            .iter()
            .filter(|t| t.type_name == FRAGMENT_PUNCTUATION_TYPE)
            .map(|t| &source[t.span.start as usize..t.span.end as usize])
            .collect();
        assert_eq!(punctuation, ["<", ">", "<", "/", ">"], "{tokens:#?}");
        let source = "fn f() { <>x</ > }";
        let tokens = collect(&parse(source), source);
        let punctuation: Vec<&str> = tokens
            .iter()
            .filter(|t| t.type_name == FRAGMENT_PUNCTUATION_TYPE)
            .map(|t| &source[t.span.start as usize..t.span.end as usize])
            .collect();
        assert_eq!(punctuation, ["<>", "<", "/", ">"], "{tokens:#?}");
    }

    #[test]
    fn tokens_a_component_name_on_both_the_opening_and_closing_tag() {
        let tokens = tokens_of("fn f() { <Greeting>hi</Greeting> }");
        let component_tokens: Vec<_> = tokens
            .iter()
            .filter(|t| t.type_name == COMPONENT_TYPE)
            .collect();
        assert_eq!(component_tokens.len(), 2, "{tokens:#?}");
    }

    #[test]
    fn tokens_an_html_element_name_as_the_element_type() {
        let tokens = tokens_of("fn f() { <div>hi</div> }");
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
        let tokens = tokens_of(r#"fn f() { <button onClick={handler} class="x"></button> }"#);
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
    fn tokens_a_raw_on_attribute_as_event() {
        let tokens = tokens_of(r#"fn f() { <button r#onclick={handler} r#class="x"></button> }"#);
        assert_eq!(
            tokens.iter().filter(|t| t.type_name == EVENT_TYPE).count(),
            1,
            "{tokens:#?}"
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.type_name == ATTRIBUTE_TYPE)
                .count(),
            1,
            "{tokens:#?}"
        );
    }

    #[test]
    fn tokens_non_blank_jsx_text() {
        let tokens = tokens_of("fn f() { <p>Hello</p> }");
        assert_eq!(
            tokens.iter().filter(|t| t.type_name == TEXT_TYPE).count(),
            1
        );
    }

    #[test]
    fn does_not_token_blank_whitespace_only_text() {
        let tokens = tokens_of("fn f() { <div>\n    <span>x</span>\n</div> }");
        // Only "x" is non-blank text; the whitespace between the elements
        // is normalized away by the parser or is blank and skipped here.
        assert_eq!(
            tokens.iter().filter(|t| t.type_name == TEXT_TYPE).count(),
            1
        );
    }

    #[test]
    fn tokens_a_nested_component_reached_through_an_island() {
        let tokens =
            tokens_of("fn f() { <div>{ if true { <Greeting /> } else { <div /> } }</div> }");
        assert_eq!(
            tokens
                .iter()
                .filter(|t| t.type_name == COMPONENT_TYPE)
                .count(),
            1
        );
    }
}
