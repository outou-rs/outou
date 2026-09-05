//! Rewrites the *payload* of an already-received rust-analyzer response —
//! as opposed to `mapping.rs`, which maps a single position or range.
//!
//! `textDocument/definition`'s `Location`/`Location[]` and
//! `textDocument/completion`'s `textEdit`/`additionalTextEdits` ranges are
//! handled generically as JSON, rather than by round-tripping through
//! `lsp_types` response structs, so a field this server does not know
//! about (a rust-analyzer extension, a future protocol addition) is
//! forwarded unchanged instead of silently dropped.
//!
//! This server strips `linkSupport` from the capabilities it forwards to
//! rust-analyzer (`crate::server::forward_capabilities`), so
//! `textDocument/definition` only ever has to handle plain `Location`
//! objects, never `LocationLink`.

use serde_json::Value;

use crate::documents::Workspace;
use crate::mapping::{self, MappedLocation};

/// Rewrites a `textDocument/hover` result's `range` field in place, per
/// ADR 0007: mapped -> the `.rsx` range; unmapped or unchanged (should not
/// happen for a hover request, which is always inside the one generated
/// file just queried) -> drop the range rather than show a generated-file
/// position in an `.rsx` buffer.
pub fn map_hover_range(workspace: &Workspace, generated_uri: &lsp_types::Uri, value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(range_value) = object.get("range").cloned() else {
        return;
    };
    let Ok(range) = serde_json::from_value::<lsp_types::Range>(range_value) else {
        return;
    };
    match mapping::generated_location_to_source(workspace, generated_uri, range) {
        MappedLocation::Source { range, .. } => {
            object.insert("range".to_string(), serde_json::to_value(range).unwrap());
        }
        MappedLocation::Unmapped | MappedLocation::Unchanged => {
            object.remove("range");
        }
    }
}

/// Rewrites a `textDocument/definition` response (`null`, a `Location`, or
/// a `Location[]`) in place: every generated location this registry
/// produced is mapped back to its `.rsx` file (ADR 0007's reverse-mapping
/// rule); anything else — a plain `.rs` module, a dependency crate — is
/// left untouched. An entry whose specific span has no source
/// (synthesized code) is dropped from the list rather than shown pointing
/// at generated Rust.
pub fn map_definition_response(workspace: &Workspace, value: &mut Value) {
    match value {
        Value::Array(items) => {
            let mapped: Vec<Value> = items
                .drain(..)
                .filter_map(|item| map_one_location(workspace, item))
                .collect();
            *items = mapped;
        }
        Value::Object(_) => {
            *value = map_one_location(workspace, value.take()).unwrap_or(Value::Null);
        }
        _ => {}
    }
}

fn map_one_location(workspace: &Workspace, mut item: Value) -> Option<Value> {
    let uri: lsp_types::Uri = item.get("uri")?.as_str()?.parse().ok()?;
    let range: lsp_types::Range = serde_json::from_value(item.get("range")?.clone()).ok()?;

    match mapping::generated_location_to_source(workspace, &uri, range) {
        MappedLocation::Source { uri, range } => {
            item["uri"] = serde_json::to_value(uri).ok()?;
            item["range"] = serde_json::to_value(range).ok()?;
            Some(item)
        }
        MappedLocation::Unchanged => Some(item),
        MappedLocation::Unmapped => None,
    }
}

/// Rewrites every range inside a `textDocument/completion` response (a
/// `CompletionList` or a plain array of `CompletionItem`) from generated
/// to `.rsx` coordinates: each item's `textEdit` (a `TextEdit` or an
/// `InsertReplaceEdit`) and `additionalTextEdits`. Unlike
/// [`map_definition_response`], every range in a completion response is
/// inside the one generated file the request was sent for.
pub fn map_completion_response(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    value: &mut Value,
) {
    let items: &mut Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(object) => match object.get_mut("items").and_then(Value::as_array_mut) {
            Some(items) => items,
            None => return,
        },
        _ => return,
    };
    for item in items.iter_mut() {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        if let Some(text_edit) = object.get_mut("textEdit") {
            map_text_edit(workspace, generated_uri, text_edit);
        }
        if let Some(edits) = object
            .get_mut("additionalTextEdits")
            .and_then(Value::as_array_mut)
        {
            for edit in edits.iter_mut() {
                map_range_field(workspace, generated_uri, edit, "range");
            }
        }
    }
}

fn map_text_edit(workspace: &Workspace, generated_uri: &lsp_types::Uri, text_edit: &mut Value) {
    if text_edit.get("range").is_some() {
        map_range_field(workspace, generated_uri, text_edit, "range");
    } else {
        map_range_field(workspace, generated_uri, text_edit, "insert");
        map_range_field(workspace, generated_uri, text_edit, "replace");
    }
}

/// Maps one range-valued field of a JSON object in place, leaving it
/// untouched if it is absent, unparsable, or maps to nothing (an
/// unmapped/unchanged edit at the wrong-but-plausible generated position
/// is more useful to a user than one silently dropped).
fn map_range_field(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    object: &mut Value,
    field: &str,
) {
    let Some(range_value) = object.get(field).cloned() else {
        return;
    };
    let Ok(range) = serde_json::from_value::<lsp_types::Range>(range_value) else {
        return;
    };
    if let MappedLocation::Source { range, .. } =
        mapping::generated_location_to_source(workspace, generated_uri, range)
    {
        object[field] = serde_json::to_value(range).unwrap();
    }
}
