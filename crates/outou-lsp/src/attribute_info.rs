//! Hover and definition at a JSX attribute, answered by this server and
//! never forwarded to rust-analyzer (`docs/backend-leakage.md` row 62).
//!
//! rust-analyzer sees the expanded Rust, where an attribute name is a
//! builder method of the backend's props type or a constant of its HTML
//! vocabulary. Its hover there was the builder's generics (`__label =
//! (String,)`) or the backend's rustdoc (`pub const r#type:
//! AttributeDescription`), and its definition was a file under the cargo
//! registry. Tag names, completion and fragments are answered locally for
//! the same reason (`crate::complete`); this module does the same for the
//! attribute positions:
//!
//! | Position | Hover | Definition |
//! |---|---|---|
//! | A prop of a component declared in an `.rsx` file | `name: Type` as the parameter is written, plus its doc comment | The parameter in the `.rsx` file |
//! | `key` (on a component or an element) | Outou's own description | `null` |
//! | Any other attribute of an element (`class`, `type`, `onclick`, `data-id`, ...) | `null` | `null` |
//! | A prop that no `.rsx` component declares (a typo, or a component written in a `.rs` file or another crate) | `null` | `null` |
//! | The text of an attribute value (`label="Name"`) | `null` | `null` |
//!
//! An element attribute has no Outou-owned documentation to show: the
//! meaning of `class` or `onclick` belongs to the web platform, and quoting
//! the backend's copy of it is the leak this module closes. A one-line
//! gloss per attribute would be Outou's own claim to maintain; it is left
//! to a later change that links the platform's documentation
//! (`TODO(alpha) (#31)`). `key` is different: it is Outou's own attribute
//! (grammar §5.1) and the backend's copy of it describes something else.
//!
//! A position counts as the name when the cursor touches it, its end
//! included, so a caret just after `label` and the `=` that follows it
//! both answer for `label`, as tag names do. An attribute whose value is an
//! expression (`tags={tags}`) is not claimed at the value: that is Rust, and
//! rust-analyzer answers it (its response is filtered, `crate::response`).

use outou_syntax::ast;
use serde_json::{json, Value};

use crate::complete::{is_component_tag, tag_name, touches};
use crate::documents::{RsxDocument, Workspace};
use crate::mapping;
use crate::params::ParamInfo;

/// What Outou says about `key`, which it owns (grammar §5.1).
const KEY_DESCRIPTION: &str = "Identifies this element among its siblings, so that it keeps its state when the list around it changes. A string or an integer.";

/// What an attribute position resolved to.
enum Target<'a> {
    /// A prop of a component declared in an `.rsx` file.
    Prop {
        /// The `.rsx` document that declares the component.
        uri: &'a str,
        document: &'a RsxDocument,
        param: &'a ParamInfo,
        /// The attribute name under the cursor, for the hover's `range`.
        range: lsp_types::Range,
    },
    /// `key`, which Outou owns.
    Key { range: lsp_types::Range },
    /// An attribute position with nothing of Outou's to say.
    Nothing,
}

/// Answers `textDocument/hover` at `position` locally when it is on a JSX
/// attribute name or on the text of an attribute value; `None` means
/// forward the request. `markdown` is whether the editor prefers Markdown.
pub fn local_hover(
    workspace: &Workspace,
    rsx_uri: &str,
    position: lsp_types::Position,
    markdown: bool,
) -> Option<Value> {
    Some(match target_at(workspace, rsx_uri, position)? {
        Target::Prop { param, range, .. } => hover(prop_text(param, markdown), range),
        Target::Key { range } => hover(key_text(markdown), range),
        Target::Nothing => Value::Null,
    })
}

/// Answers `textDocument/definition` at `position` locally when it is on a
/// JSX attribute name or on the text of an attribute value; `None` means
/// forward the request.
pub fn local_definition(
    workspace: &Workspace,
    rsx_uri: &str,
    position: lsp_types::Position,
) -> Option<Value> {
    Some(match target_at(workspace, rsx_uri, position)? {
        Target::Prop {
            uri,
            document,
            param,
            ..
        } => {
            let range = mapping::to_lsp_range(document.line_index.span_to_range(param.span));
            json!([{ "uri": uri, "range": range }])
        }
        Target::Key { .. } | Target::Nothing => Value::Null,
    })
}

fn hover(contents: Value, range: lsp_types::Range) -> Value {
    json!({ "contents": contents, "range": range })
}

/// `name: Type` (the Outou name: `type`, never `r#type`), then the doc
/// comment.
fn prop_text(param: &ParamInfo, markdown: bool) -> Value {
    let signature = if param.ty.is_empty() {
        param.name.clone()
    } else {
        format!("{}: {}", param.name, param.ty)
    };
    let mut text = if markdown {
        format!("```rust\n{signature}\n```")
    } else {
        signature
    };
    if let Some(doc) = &param.doc {
        text.push_str("\n\n");
        text.push_str(doc);
    }
    markup(&text, markdown)
}

fn key_text(markdown: bool) -> Value {
    let name = if markdown { "**key**" } else { "key" };
    markup(&format!("{name}\n\n{KEY_DESCRIPTION}"), markdown)
}

fn markup(text: &str, markdown: bool) -> Value {
    let kind = if markdown { "markdown" } else { "plaintext" };
    json!({ "kind": kind, "value": text })
}

fn target_at<'a>(
    workspace: &'a Workspace,
    rsx_uri: &str,
    position: lsp_types::Position,
) -> Option<Target<'a>> {
    let doc = workspace.rsx.get(rsx_uri)?;
    let offset = doc
        .line_index
        .position_to_offset(outou_sourcemap::Position::new(
            position.line,
            position.character,
        ));
    let parsed = crate::documents::parse_rsx(doc.line_index.text());
    for element in parsed.file.jsx_elements() {
        for attribute in &element.attributes {
            if touches(attribute.name.span, offset) {
                return Some(resolve_name(workspace, doc, element, attribute));
            }
            if let Some(ast::JsxAttributeValue::Text(text)) = &attribute.value {
                if text.span.start <= offset && offset < text.span.end {
                    return Some(Target::Nothing);
                }
            }
        }
    }
    None
}

fn resolve_name<'a>(
    workspace: &'a Workspace,
    doc: &RsxDocument,
    element: &ast::JsxElement,
    attribute: &ast::JsxAttribute,
) -> Target<'a> {
    let name = ast::unraw_name(&attribute.name.name);
    let range = mapping::to_lsp_range(doc.line_index.span_to_range(attribute.name.span));
    if name == "key" {
        return Target::Key { range };
    }
    let Some(tag) = tag_name(&element.open).filter(|tag| is_component_tag(tag)) else {
        return Target::Nothing;
    };
    workspace
        .find_component(&tag)
        .and_then(|(uri, document, component)| {
            let param = component.params.iter().find(|param| param.name == name)?;
            Some(Target::Prop {
                uri,
                document,
                param,
                range,
            })
        })
        .unwrap_or(Target::Nothing)
}
