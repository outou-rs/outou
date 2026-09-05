//! Outou-native completion for `.rsx` positions rust-analyzer cannot ever
//! answer correctly, because they do not exist in the *expanded* Rust it
//! sees: a JSX tag name (`<UserC`) or an attribute name (`<UserCard us`).
//!
//! `docs/phase0.md` lists "component and prop completion" and "completion
//! that keeps working while the file is half-typed" among Gate 3's
//! non-droppable criteria. Forwarding these two positions to
//! rust-analyzer through the generated Rust answers a *different*
//! question — "what identifier or struct-literal field fits here" — which
//! is why `<UserC` used to return `__TEMPLATE_ROOTS`/`PropsBuilder`
//! completions alongside the one correct answer (`UserCard`) buried in
//! them (issue #9 Gate 3 review, HIGH-7/HIGH-10; `<div class=` was worse:
//! the wrongly-mapped edit range corrupted the buffer on accept). Outou
//! already has the parse tree and the plan, so answering these two
//! positions itself is small, deterministic and structurally leak-free.
//!
//! `textDocument/completion` for every other position (`user.`,
//! `user={us}`, an attribute *value*) is still forwarded to rust-analyzer
//! exactly as before; [`crate::response::map_completion_response`] is the
//! safety net for whatever leaks through that path.

use outou_syntax::ast;
use serde_json::{json, Value};

use crate::documents::Workspace;
use crate::mapping;
use crate::plan;

/// A small, deliberately non-exhaustive list of HTML element names: enough
/// for Gate 3's `<di` probe and ordinary Phase 0 use, not a full HTML
/// vocabulary (which belongs to whatever validates element names at all,
/// tracked separately — `docs/backend-leakage.md` row 12).
const HTML_ELEMENTS: &[&str] = &[
    "a",
    "article",
    "aside",
    "audio",
    "b",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "code",
    "div",
    "em",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "label",
    "li",
    "link",
    "main",
    "meta",
    "nav",
    "ol",
    "option",
    "p",
    "pre",
    "script",
    "section",
    "select",
    "small",
    "span",
    "strong",
    "style",
    "svg",
    "table",
    "tbody",
    "td",
    "textarea",
    "th",
    "thead",
    "title",
    "tr",
    "u",
    "ul",
    "video",
];

// TODO(phase0) (issue #9 Gate 3 review, "Recovery quality for `<div
// class=`" SKIP item): the recovery parser swallows a following sibling
// element into a broken tag's own attributes (`<div class=` followed by
// `<Greeting name="Outou" />` recovers as `div { Greeting: true, name:
// "Outou" }`), so there is nothing meaningful at that cursor for even
// this module's own classifier to find — `classify` falls through to
// `Cursor::Expression`, and it is `crate::response`'s cursor-containment
// filter (M4) that keeps the resulting forwarded completion from
// corrupting the buffer, not a correct classification here. Improving
// the recovery shape itself is a parser/codegen question for a follow-up
// to issue #7, out of scope for this module.

/// Where the cursor sits inside an `.rsx` file's own (recovery) parse
/// tree, for the purpose of deciding whether this server or
/// rust-analyzer should answer a completion request.
enum Cursor {
    /// Inside (or immediately after) a JSX element's own name.
    TagName {
        /// What has been typed of the name so far.
        partial: String,
        /// Span of the partial name, for the completion's `textEdit`.
        span: outou_sourcemap::Span,
    },
    /// Inside (or immediately after) a JSX attribute's own name, with no
    /// value yet.
    AttrName {
        /// The enclosing element's tag name, if it has one.
        tag_name: Option<String>,
        /// What has been typed of the attribute name so far.
        partial: String,
        /// Span of the partial name, for the completion's `textEdit`.
        span: outou_sourcemap::Span,
    },
    /// Anywhere else: an ordinary Rust expression position (`user.`, an
    /// attribute value, plain Rust). Forward to rust-analyzer as before.
    Expression,
}

/// Answers `textDocument/completion` locally when `position` (inside
/// `rsx_uri`) is a tag-name or attribute-name position, per this module's
/// doc comment. Returns `None` for every other position, telling the
/// caller to forward the request to rust-analyzer as usual.
pub fn local_completion(
    workspace: &Workspace,
    rsx_uri: &str,
    position: lsp_types::Position,
) -> Option<Vec<Value>> {
    let doc = workspace.rsx.get(rsx_uri)?;
    let offset = doc
        .line_index
        .position_to_offset(outou_sourcemap::Position::new(
            position.line,
            position.character,
        ));
    let parsed = outou_syntax::parse(doc.line_index.text());

    match classify(&parsed.file, offset) {
        Cursor::TagName { partial, span } => {
            let range = mapping::to_lsp_range(doc.line_index.span_to_range(span));
            Some(tag_name_items(workspace, &partial, range))
        }
        Cursor::AttrName {
            tag_name,
            partial,
            span,
        } => {
            let range = mapping::to_lsp_range(doc.line_index.span_to_range(span));
            Some(attr_name_items(
                workspace,
                tag_name.as_deref(),
                &partial,
                range,
            ))
        }
        Cursor::Expression => None,
    }
}

/// Whether `position` inside `rsx_uri` sits on a JSX tag name — element or
/// component, opening or closing (issue #9 Gate 3 review, H2): used by
/// `crate::dispatch::requests` to answer `textDocument/hover` there with
/// `null` directly, exactly like [`local_completion`] does for
/// completion, rather than forwarding it to rust-analyzer and sanitizing
/// whatever comes back. Every generated occurrence of a tag name is
/// either backend vocabulary (an HTML element's own `dioxus_html::…`
/// rustdoc) or this backend's own macro-internal token for a user
/// component — and a *closing* tag's occurrence in particular
/// reverse-maps to the *opening* tag's `.rsx` position (H1), which would
/// make even a correctly-sanitized non-`null` hover shown there
/// misleading regardless of its content.
pub fn is_tag_name_position(
    workspace: &Workspace,
    rsx_uri: &str,
    position: lsp_types::Position,
) -> bool {
    let Some(doc) = workspace.rsx.get(rsx_uri) else {
        return false;
    };
    let offset = doc
        .line_index
        .position_to_offset(outou_sourcemap::Position::new(
            position.line,
            position.character,
        ));
    let parsed = outou_syntax::parse(doc.line_index.text());
    matches!(classify(&parsed.file, offset), Cursor::TagName { .. })
}

fn touches(span: outou_sourcemap::Span, offset: u32) -> bool {
    span.contains(outou_sourcemap::Span::new(offset, offset))
}

fn classify(file: &ast::File, offset: u32) -> Cursor {
    for item in &file.items {
        if let Some(cursor) = classify_item(item, offset) {
            return cursor;
        }
    }
    Cursor::Expression
}

fn classify_item(item: &ast::Item, offset: u32) -> Option<Cursor> {
    match item {
        ast::Item::Function(function) => {
            touches(function.span, offset).then(|| classify_block(&function.body, offset))?
        }
        ast::Item::Module(module) => {
            if !touches(module.span, offset) {
                return None;
            }
            module
                .items
                .as_ref()?
                .iter()
                .find_map(|item| classify_item(item, offset))
        }
        ast::Item::Rust(rust) => {
            if !touches(rust.span, offset) {
                return None;
            }
            rust.parts
                .iter()
                .find_map(|expr| classify_expr(expr, offset))
        }
        ast::Item::Error(_) => None,
    }
}

fn classify_block(block: &ast::Block, offset: u32) -> Option<Cursor> {
    if !touches(block.span, offset) {
        return None;
    }
    for statement in &block.statements {
        if let Some(cursor) = classify_expr(statement, offset) {
            return Some(cursor);
        }
    }
    block
        .tail
        .as_ref()
        .and_then(|tail| classify_expr(tail, offset))
}

fn classify_expr(expr: &ast::Expr, offset: u32) -> Option<Cursor> {
    match expr {
        // Verbatim Rust never contains JSX itself — any JSX nested inside
        // it already surfaced as its own sibling `Expr::Jsx` at parse
        // time (`ast::Island`/`ast::RustItem`'s own partitioning
        // guarantee) — so there is nothing further to classify here.
        ast::Expr::Rust(_) | ast::Expr::Error(_) => None,
        ast::Expr::Jsx(element) => classify_element(element, offset),
    }
}

fn classify_element(element: &ast::JsxElement, offset: u32) -> Option<Cursor> {
    if !touches(element.span, offset) {
        return None;
    }
    if let Some(cursor) = classify_tag(&element.open, offset) {
        return Some(cursor);
    }
    for attribute in &element.attributes {
        if let Some(cursor) = classify_attribute(&element.open, attribute, offset) {
            return Some(cursor);
        }
    }
    for child in &element.children {
        if let Some(cursor) = classify_child(child, offset) {
            return Some(cursor);
        }
    }
    // H1 (issue #9 Gate 3 review): a closing tag's own name is a tag-name
    // position too, exactly like the opening tag's, and must be answered
    // locally for the same reason — forwarding it to rust-analyzer
    // returned a `dioxus_html::AttributeDescription`-shaped completion (or
    // the element's full rustdoc on hover), and its `textEdit`/range
    // always reverse-mapped back to the *opening* tag's `.rsx` position
    // instead (`crate::mapping::generated_location_to_source`'s multi-source
    // mapping always uses the first source) — accepting it silently
    // rewrote the wrong tag.
    if let Some(close) = &element.close {
        if let Some(cursor) = classify_tag(close, offset) {
            return Some(cursor);
        }
    }
    None
}

fn classify_tag(tag: &ast::JsxTag, offset: u32) -> Option<Cursor> {
    match tag {
        ast::JsxTag::Named { name, .. } => touches(name.span, offset).then(|| Cursor::TagName {
            partial: prefix_before_cursor(name, offset),
            span: name.span,
        }),
        ast::JsxTag::Incomplete(incomplete) => match &incomplete.name {
            Some(name) => touches(name.span, offset).then(|| Cursor::TagName {
                partial: prefix_before_cursor(name, offset),
                span: name.span,
            }),
            // M6 (issue #9 Gate 3 review): a bare `<` with no name typed
            // yet at all is still a tag-name position, not an ordinary
            // Rust expression — without this, `JsxTag::Incomplete { name:
            // None }` fell through every classifier all the way to
            // `Cursor::Expression`, which then forwarded the bare `<`
            // into an overlay that is not even valid Rust (recovery emits
            // it verbatim), so the advertised `<` trigger character
            // returned nothing.
            None => touches(incomplete.span, offset).then(|| Cursor::TagName {
                partial: String::new(),
                span: outou_sourcemap::Span::new(offset, offset),
            }),
        },
    }
}

/// The prefix of `name`'s text up to (not including) byte offset `cursor`
/// — the identifier text actually typed so far — rather than `name`'s
/// whole (possibly longer) text (issue #9 Gate 3 review, M7): filtering
/// candidates by the *whole* name instead left only an exact match while
/// an edit was still in progress (editing `<TagL‸ist` in place filtered
/// candidates by the full `"TagList"`, not the `"TagL"` actually typed
/// before the cursor, so `TagList` itself was the only survivor and
/// nothing else). Byte-safe: falls back to the whole name if `cursor`
/// does not land on a `char` boundary within it (defensive only — tag and
/// attribute names are ordinary Rust identifiers, always ASCII).
fn prefix_before_cursor(name: &ast::Ident, cursor: u32) -> String {
    let len = cursor.saturating_sub(name.span.start) as usize;
    name.name.get(..len).unwrap_or(&name.name).to_string()
}

fn tag_name(tag: &ast::JsxTag) -> Option<String> {
    match tag {
        ast::JsxTag::Named { name, .. } => Some(name.name.clone()),
        ast::JsxTag::Incomplete(incomplete) => incomplete.name.as_ref().map(|n| n.name.clone()),
    }
}

fn classify_attribute(
    open: &ast::JsxTag,
    attribute: &ast::JsxAttribute,
    offset: u32,
) -> Option<Cursor> {
    if attribute.value.is_none() && touches(attribute.name.span, offset) {
        return Some(Cursor::AttrName {
            tag_name: tag_name(open),
            partial: prefix_before_cursor(&attribute.name, offset),
            span: attribute.name.span,
        });
    }
    if let Some(ast::JsxAttributeValue::Expression(island)) = &attribute.value {
        for part in &island.parts {
            if let Some(cursor) = classify_expr(part, offset) {
                return Some(cursor);
            }
        }
    }
    None
}

fn classify_child(child: &ast::JsxChild, offset: u32) -> Option<Cursor> {
    match child {
        ast::JsxChild::Text(_) | ast::JsxChild::Error(_) => None,
        ast::JsxChild::Element(element) => classify_element(element, offset),
        ast::JsxChild::Expression(island) => island
            .parts
            .iter()
            .find_map(|part| classify_expr(part, offset)),
    }
}

/// Every parsed `.rsx` file this workspace currently knows the text of
/// (`Workspace::rsx`, populated for every planned unit whether or not the
/// editor has it open), reparsed fresh so tag-name/attribute-name
/// completion always reflects the current buffers rather than a plan-time
/// snapshot.
///
/// TODO(phase0) (issue #9 Gate 3 review, L15's SKIP item): this reparses
/// **every** `.rsx` file in the workspace on every tag/attribute-name
/// keystroke (plus one further reparse of the current document inside
/// `local_completion`, whose own `RsxDocument` already parsed it). Fine
/// at Phase 0 scale (`examples/phase0-app`'s two files reparse in well
/// under a millisecond); caching each file's last parse (invalidated on
/// its own edit) or reusing the current document's own parse is future
/// work, not required for Gate 3.
fn parsed_files(workspace: &Workspace) -> Vec<outou_syntax::Parsed> {
    workspace
        .rsx
        .values()
        .map(|doc| outou_syntax::parse(doc.line_index.text()))
        .collect()
}

fn tag_name_items(workspace: &Workspace, partial: &str, range: lsp_types::Range) -> Vec<Value> {
    let mut labels: Vec<String> = Vec::new();
    for parsed in parsed_files(workspace) {
        for function in plan::component_functions(&parsed.file) {
            if function.name.name.starts_with(partial) && !labels.contains(&function.name.name) {
                labels.push(function.name.name.clone());
            }
        }
    }
    let mut items: Vec<Value> = labels
        .into_iter()
        .map(|label| completion_item(&label, CompletionKind::Class, range))
        .collect();
    for element in HTML_ELEMENTS {
        if element.starts_with(partial) {
            items.push(completion_item(element, CompletionKind::Class, range));
        }
    }
    items
}

fn attr_name_items(
    workspace: &Workspace,
    tag_name: Option<&str>,
    partial: &str,
    range: lsp_types::Range,
) -> Vec<Value> {
    let Some(tag_name) = tag_name else {
        return Vec::new();
    };
    if HTML_ELEMENTS.contains(&tag_name) {
        // Phase 0 has no HTML attribute vocabulary of its own
        // (`docs/backend-leakage.md` row 12); an honest empty list beats
        // forwarding to rust-analyzer and leaking `dioxus_html::…`.
        return Vec::new();
    }
    for parsed in parsed_files(workspace) {
        for function in plan::component_functions(&parsed.file) {
            if function.name.name == tag_name {
                return function_params(&function.signature.text)
                    .into_iter()
                    .filter(|param| param.starts_with(partial))
                    .map(|param| completion_item(&param, CompletionKind::Field, range))
                    .collect();
            }
        }
    }
    Vec::new()
}

#[derive(Clone, Copy)]
enum CompletionKind {
    Class,
    Field,
}

impl CompletionKind {
    fn lsp_number(self) -> i64 {
        match self {
            // `lsp_types::CompletionItemKind::CLASS`/`FIELD`, spelled out
            // as plain numbers since this module builds JSON directly
            // rather than round-tripping through `lsp_types::CompletionItem`.
            CompletionKind::Class => 7,
            CompletionKind::Field => 5,
        }
    }
}

fn completion_item(label: &str, kind: CompletionKind, range: lsp_types::Range) -> Value {
    json!({
        "label": label,
        "kind": kind.lsp_number(),
        "textEdit": { "range": range, "newText": label },
    })
}

/// Extracts parameter names from a `#[component]` function's signature
/// text (`ast::Function::signature`, verbatim from `fn` to the opening
/// `{`), stripping the raw-identifier prefix (`r#type` -> `type`) per
/// grammar §5 / `docs/backend-leakage.md` row 17.
fn function_params(signature: &str) -> Vec<String> {
    let Some(open) = signature.find('(') else {
        return Vec::new();
    };
    let bytes = signature.as_bytes();
    let mut depth = 0i32;
    let mut close = None;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return Vec::new();
    };
    split_top_level_commas(&signature[open + 1..close])
        .into_iter()
        .filter_map(param_name)
        .collect()
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

fn param_name(param: &str) -> Option<String> {
    let param = param.trim();
    if param.is_empty() || param == "self" || param == "&self" || param == "&mut self" {
        return None;
    }
    let name = param.split(':').next()?.trim();
    let name = name.strip_prefix("mut ").unwrap_or(name).trim();
    let name = name.strip_prefix("r#").unwrap_or(name);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_params_extracts_plain_parameter_names() {
        assert_eq!(
            function_params("fn UserCard(user: User) -> Element"),
            vec!["user"]
        );
    }

    #[test]
    fn function_params_strips_the_raw_identifier_prefix() {
        assert_eq!(
            function_params("fn Field(label: String, r#type: String) -> Element"),
            vec!["label", "type"]
        );
    }

    #[test]
    fn function_params_handles_generic_parameter_types() {
        assert_eq!(
            function_params("fn TagList(tags: Vec<String>) -> Element"),
            vec!["tags"]
        );
    }

    #[test]
    fn classify_finds_a_partial_component_tag_name() {
        let source = "#[component]\nfn App() -> Element {\n    <UserC\n}\n";
        let parsed = outou_syntax::parse(source);
        let offset = source.find("<UserC").unwrap() as u32 + "<UserC".len() as u32;
        match classify(&parsed.file, offset) {
            Cursor::TagName { partial, .. } => assert_eq!(partial, "UserC"),
            _ => panic!("expected a TagName cursor"),
        }
    }

    #[test]
    fn classify_finds_a_partial_attribute_name_on_a_known_tag() {
        let source = "#[component]\nfn App() -> Element {\n    <UserCard us\n}\n";
        let parsed = outou_syntax::parse(source);
        let offset = source.find("<UserCard us").unwrap() as u32 + "<UserCard us".len() as u32;
        match classify(&parsed.file, offset) {
            Cursor::AttrName {
                tag_name, partial, ..
            } => {
                assert_eq!(tag_name.as_deref(), Some("UserCard"));
                assert_eq!(partial, "us");
            }
            _ => panic!("expected an AttrName cursor"),
        }
    }

    #[test]
    fn classify_treats_an_attribute_value_position_as_expression() {
        let source = "#[component]\nfn App() -> Element {\n    <div class=\n}\n";
        let parsed = outou_syntax::parse(source);
        let offset = source.find("class=").unwrap() as u32 + "class=".len() as u32;
        assert!(matches!(classify(&parsed.file, offset), Cursor::Expression));
    }

    /// H1: a *closing* tag's own name is a tag-name position too, not
    /// merely something that falls through to `Cursor::Expression` (which
    /// used to forward it to rust-analyzer).
    #[test]
    fn classify_finds_a_closing_tag_name() {
        let source = "#[component]\nfn App() -> Element {\n    <p>hi</p>\n}\n";
        let parsed = outou_syntax::parse(source);
        let offset = source.find("</p>").unwrap() as u32 + "</p".len() as u32;
        match classify(&parsed.file, offset) {
            Cursor::TagName { partial, .. } => assert_eq!(partial, "p"),
            _ => panic!("expected a TagName cursor for the closing tag"),
        }
    }

    /// M6: a bare `<` with nothing typed after it yet is still a
    /// tag-name position (with an empty partial), not `Cursor::Expression`.
    /// Needs a nested-child `<` (grammar §2.1's "always a nested element,
    /// no Rust-vs-JSX ambiguity" rule,
    /// `outou_syntax::parser::jsx::children::parse_nested_child_element`)
    /// rather than a top-level one: a bare `<` at a fresh Rust-expression
    /// position with no JSX-shaped token after it (here, `}`) is rule 2's
    /// ordinary less-than operator, and never becomes JSX at all.
    #[test]
    fn classify_treats_a_bare_open_angle_bracket_as_an_empty_tag_name() {
        let source = "#[component]\nfn App() -> Element {\n    <div>\n        <\n    </div>\n}\n";
        let parsed = outou_syntax::parse(source);
        let offset = source.find("<\n").unwrap() as u32 + 1;
        match classify(&parsed.file, offset) {
            Cursor::TagName { partial, .. } => assert_eq!(partial, ""),
            _ => panic!("expected an empty TagName cursor"),
        }
    }

    /// M7: editing a name *in place* must filter by the prefix before the
    /// cursor, not the whole (longer) name — `<TagL‸ist` should behave
    /// like `<TagL`, not like a complete, exact `"TagList"` filter.
    #[test]
    fn classify_uses_the_prefix_before_the_cursor_not_the_whole_name() {
        let source = "#[component]\nfn App() -> Element {\n    <TagList\n}\n";
        let parsed = outou_syntax::parse(source);
        let offset = source.find("<TagList").unwrap() as u32 + "<TagL".len() as u32;
        match classify(&parsed.file, offset) {
            Cursor::TagName { partial, .. } => assert_eq!(partial, "TagL"),
            _ => panic!("expected a TagName cursor"),
        }
    }

    #[test]
    fn is_tag_name_position_is_true_on_a_closing_tag_and_false_on_an_expression() {
        let source =
            "#[component]\nfn App() -> Element {\n    let user = load_user();\n    <p>hi</p>\n}\n";
        let workspace = crate::documents::Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new(),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
        };
        let mut workspace = workspace;
        workspace.rsx.insert(
            "file:///app/src/main.rsx".to_string(),
            crate::documents::RsxDocument::new(source.to_string(), 1),
        );
        let doc = workspace.rsx.get("file:///app/src/main.rsx").unwrap();
        let close_offset = source.find("</p>").unwrap() as u32 + "</p".len() as u32;
        let close_position = mapping::to_lsp_range(
            doc.line_index
                .span_to_range(outou_sourcemap::Span::new(close_offset, close_offset)),
        )
        .start;
        assert!(is_tag_name_position(
            &workspace,
            "file:///app/src/main.rsx",
            close_position
        ));

        let expr_offset = source.find("load_user()").unwrap() as u32 + 2;
        let expr_position = mapping::to_lsp_range(
            doc.line_index
                .span_to_range(outou_sourcemap::Span::new(expr_offset, expr_offset)),
        )
        .start;
        assert!(!is_tag_name_position(
            &workspace,
            "file:///app/src/main.rsx",
            expr_position
        ));
    }
}
