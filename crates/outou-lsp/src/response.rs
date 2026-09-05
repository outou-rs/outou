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
//!
//! Every function here is also where backend vocabulary that survived
//! position-mapping gets sanitized before it ever reaches the editor
//! (issue #9 Gate 3 review, M4): rust-analyzer answers every one of these
//! requests against *generated* Rust, so its raw answer routinely names
//! `dioxus_*` paths, `PropsBuilder` machinery or a `src/.generated/…`
//! location — all forbidden in user-facing output by `AGENTS.md`.

use serde_json::Value;

use crate::documents::Workspace;
use crate::mapping::{self, MappedLocation};
use crate::translate;

/// Rewrites a `textDocument/hover` result in place: maps `range` back to
/// the `.rsx` file (dropping it, per ADR 0007, if the hover position has
/// no source — should not happen for a hover request, which is always
/// inside the one generated file just queried) and sanitizes `contents`
/// (see [`sanitize_hover_contents`]).
pub fn sanitize_hover(workspace: &Workspace, generated_uri: &lsp_types::Uri, value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if let Some(range_value) = object.get("range").cloned() {
        if let Ok(range) = serde_json::from_value::<lsp_types::Range>(range_value) {
            match mapping::generated_location_to_source(workspace, generated_uri, range) {
                MappedLocation::Source { range, .. } => {
                    object.insert("range".to_string(), serde_json::to_value(range).unwrap());
                }
                MappedLocation::Unmapped | MappedLocation::Unchanged => {
                    object.remove("range");
                }
            }
        }
    }
    if let Some(contents) = object.get_mut("contents") {
        if !sanitize_hover_contents(contents) {
            *value = Value::Null;
        }
    }
}

/// Sanitizes a hover's `contents` in place (a `MarkupContent`, a bare
/// string, or `MarkedString[]` — this walks whichever shape is present by
/// rewriting every string it finds). Returns `false` when the whole hover
/// should be dropped instead: a Dioxus HTML element's hover
/// (`dioxus_html::elements\n\npub mod main\n…`) has nothing left worth
/// showing once its first line — a bare backend module path — is
/// removed, so the hover is suppressed entirely rather than shown
/// starting mid-sentence.
fn sanitize_hover_contents(contents: &mut Value) -> bool {
    match contents {
        Value::String(s) => sanitize_hover_text(s),
        Value::Array(items) => {
            let mut kept = Vec::with_capacity(items.len());
            for mut item in items.drain(..) {
                let ok = match &mut item {
                    Value::String(s) => sanitize_hover_text(s),
                    Value::Object(object) => object
                        .get_mut("value")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .map(|mut s| {
                            let ok = sanitize_hover_text(&mut s);
                            object.insert("value".to_string(), Value::String(s));
                            ok
                        })
                        .unwrap_or(true),
                    _ => true,
                };
                if ok {
                    kept.push(item);
                }
            }
            *items = kept;
            !items.is_empty()
        }
        Value::Object(object) => {
            let Some(text) = object.get("value").and_then(Value::as_str) else {
                return true;
            };
            let mut text = text.to_string();
            let ok = sanitize_hover_text(&mut text);
            object.insert("value".to_string(), Value::String(text));
            ok
        }
        _ => true,
    }
}

/// Rewrites one hover text block in place per issue #9 Gate 3 review
/// (M4/HIGH-9(d)): every `::outou::__private::…`/`dioxus_*::…` path
/// prefix is stripped (rustc's own type printer routinely chooses the
/// shortest public path, which is this one, `docs/backend-leakage.md`
/// row 19), and any *line* still containing a backend marker after that
/// is dropped outright rather than partially rewritten. Returns `false`
/// when nothing is left (the whole block was backend vocabulary, e.g. an
/// HTML element's own `dioxus_html::elements` doc comment).
fn sanitize_hover_text(text: &mut String) -> bool {
    let rewritten = text
        .replace("::outou::__private::", "")
        .replace("dioxus_core::", "")
        .replace("dioxus_elements::", "")
        .replace("dioxus_signals::", "")
        .replace("dioxus_html::", "")
        .replace("dioxus::", "");
    let kept: Vec<&str> = rewritten
        .lines()
        .filter(|line| !translate::contains_backend_marker(line))
        .collect();
    *text = kept.join("\n");
    !text.trim().is_empty()
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
/// to `.rsx` coordinates, and sanitizes the item list itself (issue #9
/// Gate 3 review, M4/HIGH-9(c) and HIGH-8):
///
/// - Any item whose primary `textEdit` range (in *generated* coordinates,
///   before mapping) does not contain `cursor` — the generated position
///   this request was actually sent for — is dropped outright. This is
///   the generic fix for HIGH-8: a wrongly-mapped edit (e.g. `<div
///   class=`'s zero-width edit ten columns from the cursor) corrupts the
///   buffer if accepted, so it is never returned rather than trusted.
/// - Any item whose `label`, `detail`, `documentation` or `filterText`
///   contains a backend marker (`crate::translate::contains_backend_marker`)
///   is dropped: `dioxus_core::`, `__TEMPLATE_ROOTS`, `GreetingProps`, …
/// - `build`/`into(as Into)`/`try_into(as TryInto)` — the typed-builder
///   internals a `PropsBuilder` chain exposes — are dropped specifically
///   when `detail` names a `PropsBuilder`, rather than blanket-dropping
///   every `.into()`/`.try_into()` completion elsewhere.
/// - `documentation` is stripped from every surviving item rather than
///   attempting to rewrite it (it is free-form Markdown/plaintext, most
///   often rustdoc pulled from the backend's own crates).
pub fn map_completion_response(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    cursor: lsp_types::Position,
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

    let sanitized: Vec<Value> = items
        .drain(..)
        .filter(|item| primary_edit_contains_cursor(item, cursor))
        .filter(|item| !is_backend_leak(item))
        .map(|mut item| {
            if let Some(object) = item.as_object_mut() {
                if let Some(text_edit) = object.get_mut("textEdit") {
                    map_text_edit(workspace, generated_uri, text_edit);
                }
                // TODO(phase0) (issue #9 Gate 3 review, HIGH-8's SKIP
                // item): `additionalTextEdits` (typically an
                // auto-import) is mapped the same way as the primary
                // edit, but this server's `completionProvider` does not
                // advertise `resolveProvider`
                // (`crate::server::build_server_capabilities`), so a
                // conformant client never calls
                // `completionItem/resolve` to begin with and any
                // `additionalTextEdits` rust-analyzer attaches directly
                // to the initial response is simply unused by such a
                // client — degraded (an auto-import edit is lost), not
                // corrupting. Left unfixed for Phase 0; advertising
                // `resolveProvider` and mapping its own response is
                // future work.
                if let Some(edits) = object
                    .get_mut("additionalTextEdits")
                    .and_then(Value::as_array_mut)
                {
                    for edit in edits.iter_mut() {
                        map_range_field(workspace, generated_uri, edit, "range");
                    }
                }
                object.remove("documentation");
            }
            item
        })
        .collect();
    *items = sanitized;
}

/// Whether `item`'s primary edit range (`textEdit.range`, or `.insert`
/// for an `InsertReplaceEdit`) contains `cursor`, in the *generated*
/// coordinates rust-analyzer answered in. An item with no `textEdit` at
/// all (a plain label-only completion, applied by the client at its own
/// default word-replacement range) is always kept — there is nothing
/// here to validate.
fn primary_edit_contains_cursor(item: &Value, cursor: lsp_types::Position) -> bool {
    let Some(text_edit) = item.get("textEdit") else {
        return true;
    };
    let range_value = text_edit
        .get("range")
        .or_else(|| text_edit.get("insert"))
        .cloned();
    let Some(range_value) = range_value else {
        return true;
    };
    let Ok(range) = serde_json::from_value::<lsp_types::Range>(range_value) else {
        return true;
    };
    range_contains(range, cursor)
}

fn range_contains(range: lsp_types::Range, position: lsp_types::Position) -> bool {
    position_le(range.start, position) && position_le(position, range.end)
}

fn position_le(a: lsp_types::Position, b: lsp_types::Position) -> bool {
    (a.line, a.character) <= (b.line, b.character)
}

/// Whether `item` is a backend-vocabulary leak that must never reach the
/// editor: its `label`/`detail`/`documentation`/`filterText` names a
/// backend marker, or it is a typed-builder internal method
/// (`build`/`into`/`try_into`) exposed by a `PropsBuilder` chain.
fn is_backend_leak(item: &Value) -> bool {
    for field in ["label", "detail", "documentation", "filterText"] {
        if let Some(text) = item.get(field).and_then(field_text) {
            if translate::contains_backend_marker(&text) {
                return true;
            }
        }
    }
    let label = item.get("label").and_then(Value::as_str).unwrap_or("");
    let detail = item.get("detail").and_then(Value::as_str).unwrap_or("");
    matches!(label, "build" | "into(as Into)" | "try_into(as TryInto)")
        && (detail.contains("PropsBuilder") || detail.contains("Builder"))
}

/// Extracts the text of a completion item field that may be either a bare
/// string or a `MarkupContent`/`documentation`-shaped object with its own
/// `value`.
fn field_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cursor(line: u32, character: u32) -> lsp_types::Position {
        lsp_types::Position::new(line, character)
    }

    fn edit_range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
        json!({ "range": { "start": { "line": sl, "character": sc }, "end": { "line": el, "character": ec } } })
    }

    #[test]
    fn primary_edit_contains_cursor_accepts_an_edit_that_spans_the_cursor() {
        let item = json!({ "label": "unwrap", "textEdit": edit_range(0, 5, 0, 5) });
        assert!(primary_edit_contains_cursor(&item, cursor(0, 5)));
    }

    #[test]
    fn primary_edit_contains_cursor_rejects_an_edit_far_from_the_cursor() {
        // The `<div class=` corruption (HIGH-8): a zero-width edit at
        // character 8 while the cursor is at character 19.
        let item = json!({ "label": "dioxus_core::", "textEdit": edit_range(0, 8, 0, 8) });
        assert!(!primary_edit_contains_cursor(&item, cursor(0, 19)));
    }

    #[test]
    fn items_with_no_text_edit_are_never_dropped_for_the_cursor_check() {
        let item = json!({ "label": "unwrap" });
        assert!(primary_edit_contains_cursor(&item, cursor(3, 1)));
    }

    #[test]
    fn is_backend_leak_flags_dioxus_module_labels() {
        assert!(is_backend_leak(&json!({ "label": "dioxus_core::" })));
        assert!(is_backend_leak(&json!({ "label": "__TEMPLATE_ROOTS" })));
        assert!(is_backend_leak(&json!({ "label": "GreetingProps" })));
        assert!(!is_backend_leak(&json!({ "label": "unwrap" })));
    }

    #[test]
    fn is_backend_leak_flags_props_builder_internals_only_in_a_builder_context() {
        let leak = json!({
            "label": "build",
            "detail": "fn(self) -> UserCardPropsBuilder<((User,),)>",
        });
        assert!(is_backend_leak(&leak));
        let ordinary = json!({ "label": "build", "detail": "fn build_something()" });
        assert!(!is_backend_leak(&ordinary));
    }

    #[test]
    fn map_completion_response_drops_leaking_items_and_strips_documentation() {
        let workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry: outou_sourcemap::Registry::new(),
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
        };
        let generated_uri: lsp_types::Uri =
            "file:///app/src/.generated/crate-root.rs".parse().unwrap();
        let mut value = json!([
            { "label": "unwrap", "documentation": "docs" },
            { "label": "dioxus_core::" },
        ]);
        map_completion_response(&workspace, &generated_uri, cursor(0, 0), &mut value);
        let items = value.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["label"], "unwrap");
        assert!(items[0].get("documentation").is_none());
    }

    #[test]
    fn sanitize_hover_text_drops_a_pure_backend_module_line() {
        let mut text =
            "dioxus_html::elements\n\npub mod main\n\nBuild a <main> element.".to_string();
        assert!(sanitize_hover_text(&mut text));
        assert!(!text.contains("dioxus_html"));
        assert!(text.contains("pub mod main"));
    }

    #[test]
    fn sanitize_hover_text_rewrites_private_path_prefixes() {
        let mut text =
            "pub fn UserCard(::outou::__private::dioxus_core::Props) -> Element".to_string();
        assert!(sanitize_hover_text(&mut text));
        assert!(!text.contains("::outou::__private::"));
        assert!(!text.contains("dioxus_core::"));
    }

    #[test]
    fn sanitize_hover_text_drops_a_hover_that_is_entirely_backend_vocabulary() {
        let mut text = "dioxus_core::PropsBuilder".to_string();
        assert!(!sanitize_hover_text(&mut text));
    }
}
