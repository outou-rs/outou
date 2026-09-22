//! `textDocument/rename` and `textDocument/prepareRename`: forwarded to
//! rust-analyzer at the mapped generated position
//! (`crate::dispatch::requests`, reusing the same
//! [`crate::mapping::rsx_position_to_generated`] every other position-based
//! request already goes through — a cursor on a *closing* tag name maps
//! correctly with no special case, because its identifier mapping shares
//! the same generated span as the opening tag's, ADR 0007's own example),
//! and translated back here.
//!
//! # Design decisions (`docs/adr/0013-rename-translation-and-refusal.md`)
//!
//! - **Both tags update from one translation, for free.** A component or
//!   element name's generated identifier maps back to *both* its opening
//!   and closing tag spans in one [`outou_sourcemap::Mapping`] (see
//!   `crates/outou-backend-dioxus/src/element.rs`'s `lower_element_body`).
//!   [`crate::mapping::generated_range_to_all_sources`] (built on
//!   [`outou_sourcemap::Registry::reverse`], which already flattens every
//!   source of a matching mapping) therefore emits the same rename edit at
//!   both tags automatically — there is no separate "also edit the closing
//!   tag" step to get wrong.
//! - **Path-qualified component names do not need special handling.**
//!   `docs/grammar.md` §5 requires a component tag name to be a plain Rust
//!   `IDENTIFIER`: `<ui::Button>` is not valid Outou JSX syntax at all, so
//!   a JSX tag's own identifier span can never contain `::`. A rename that
//!   also touches a `use ui::Button;` import (an ordinary `.rs` position)
//!   passes through unchanged, like any other non-generated location.
//! - **Refuse rather than partially rename.** An edit that lands inside a
//!   known generated file but maps to *no* source at all (synthesized
//!   code — a recovery placeholder, or the `rsx!` macro's own punctuation)
//!   is not silently dropped: the whole rename is refused with an
//!   Outou-vocabulary error, because a partially-applied rename (some
//!   occurrences updated, one silently skipped) is worse than none.
//! - **Both `changes` and `documentChanges` are translated**, and a
//!   translated `.rsx` entry's `textDocument.version` (in the
//!   `documentChanges` form) is the `.rsx` document's own version, never
//!   the generated file's — an ordinary `.rs` entry keeps whatever
//!   identifier rust-analyzer already gave it.
//! - **A resource operation (create/rename/delete a file) that touches a
//!   generated file** cannot be translated at all — refused for the same
//!   reason as an unmappable edit.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use crate::documents::Workspace;
use crate::mapping::{self, MappedLocation};
use crate::uri;

/// A rename could not be translated back to `.rsx` terms without silently
/// dropping part of it. Carries the generated URI (or a fixed marker for a
/// resource operation) for the error message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappableRename {
    pub generated_uri: String,
}

/// Rewrites a `textDocument/prepareRename` response in place: `null` and
/// `{defaultBehavior: bool}` (neither of which carries a location) pass
/// through unchanged; a bare `Range` or a `{range, placeholder}` object has
/// its `range` mapped back to `.rsx` coordinates, or the whole response
/// replaced with `null` (not renameable here) if that position has no
/// source at all.
/// TODO(phase0) (issue #14 review, MEDIUM-8): [`mapping::generated_location_to_source`]
/// always resolves a multi-source mapping to its *first* source (the
/// opening tag, ADR 0007's own documented behavior, shared with hover/
/// definition). A `prepareRename` request made with the cursor on a
/// *closing* tag name is still forwarded and mapped correctly on the way
/// out (`rsx_position_to_generated` needs no special case, see this
/// module's own doc comment), but the range this function then returns
/// always highlights the *opening* tag, never the closing one the cursor
/// actually sat on. Not fixed here: doing so would need
/// `generated_location_to_source` (or a new single-location variant) to
/// take the originating `.rsx` position as a hint for which source to
/// prefer, which no other caller of that shared function currently needs.
pub fn translate_prepare_rename_response(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    value: &mut Value,
) {
    if value.is_null() {
        return;
    }
    let Some(range) = extract_range(value) else {
        return;
    };
    match mapping::generated_location_to_source(workspace, generated_uri, range) {
        MappedLocation::Source { range, .. } => set_range(value, range),
        MappedLocation::Unmapped | MappedLocation::Unchanged => *value = Value::Null,
    }
}

fn extract_range(value: &Value) -> Option<lsp_types::Range> {
    if value.get("start").is_some() && value.get("end").is_some() {
        serde_json::from_value(value.clone()).ok()
    } else {
        serde_json::from_value(value.get("range")?.clone()).ok()
    }
}

fn set_range(value: &mut Value, range: lsp_types::Range) {
    let range_value = serde_json::to_value(range).unwrap_or(Value::Null);
    if value.get("start").is_some() && value.get("end").is_some() {
        *value = range_value;
    } else if let Some(object) = value.as_object_mut() {
        object.insert("range".to_string(), range_value);
    }
}

/// Translates a `textDocument/rename` `WorkspaceEdit` in place, per this
/// module's own doc comment. `Err` means the whole rename must be refused
/// (the caller turns this into an LSP error response, never a partial
/// edit); `value` is left in an unspecified, must-not-be-used state on
/// `Err`.
pub fn translate_workspace_edit(
    workspace: &Workspace,
    value: &mut Value,
) -> Result<(), UnmappableRename> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    if let Some(changes) = object.get("changes").cloned() {
        let translated = translate_changes(workspace, &changes)?;
        object.insert("changes".to_string(), translated);
    }
    if let Some(document_changes) = object.get("documentChanges").cloned() {
        let translated = translate_document_changes(workspace, &document_changes)?;
        object.insert("documentChanges".to_string(), translated);
    }
    Ok(())
}

fn extract_edit_range(edit: &Value) -> Option<lsp_types::Range> {
    serde_json::from_value(edit.get("range")?.clone()).ok()
}

fn extract_edit_new_text(edit: &Value) -> Option<String> {
    edit.get("newText")?.as_str().map(str::to_string)
}

/// One translated edit, still tagged by its target `.rsx`/`.rs` URI:
/// `.rs` edits keep their original range untouched; `.rsx` edits may
/// appear more than once for the same generated edit (opening + closing
/// tag) before [`dedup`] collapses exact duplicates.
type TranslatedEdit = (lsp_types::Uri, lsp_types::Range, String);

/// Translates `changes` (`WorkspaceEdit.changes`, `Uri -> TextEdit[]`):
/// entries for a generated file are mapped back to `.rsx` (or refuse the
/// whole rename); every other entry — an ordinary `.rs` file — passes
/// through completely unchanged, keyed by its original URI string.
fn translate_changes(workspace: &Workspace, changes: &Value) -> Result<Value, UnmappableRename> {
    let Some(map) = changes.as_object() else {
        return Ok(changes.clone());
    };
    let mut translated_edits: Vec<TranslatedEdit> = Vec::new();
    let mut passthrough: Map<String, Value> = Map::new();

    for (uri_str, edits_value) in map {
        let Ok(uri) = uri_str.parse::<lsp_types::Uri>() else {
            continue;
        };
        if workspace.registry.is_generated(&uri::to_outou(&uri)) {
            let edits = edits_value.as_array().cloned().unwrap_or_default();
            translated_edits.extend(translate_generated_edits(workspace, &uri, &edits)?);
        } else {
            passthrough.insert(uri_str.clone(), edits_value.clone());
        }
    }

    let mut result = passthrough;
    for (uri_str, edits) in group_by_uri(dedup(translated_edits)) {
        result.insert(uri_str, Value::Array(edits));
    }
    Ok(Value::Object(result))
}

/// Translates `documentChanges` (`(TextDocumentEdit | ResourceOp)[]`):
/// a `TextDocumentEdit` for a generated file is mapped back to one or more
/// `.rsx` `TextDocumentEdit`s, each versioned with the `.rsx` document's
/// own version (never the generated file's, per this module's own doc
/// comment); one for an ordinary `.rs` file passes through unchanged,
/// version and all. A resource operation (create/rename/delete) passes
/// through unless it names a generated file, which refuses the rename —
/// there is no sound `.rsx` translation of a file-level operation on
/// generated code.
fn translate_document_changes(
    workspace: &Workspace,
    document_changes: &Value,
) -> Result<Value, UnmappableRename> {
    let Some(items) = document_changes.as_array() else {
        return Ok(document_changes.clone());
    };
    let mut translated_edits: Vec<TranslatedEdit> = Vec::new();
    let mut passthrough_items: Vec<Value> = Vec::new();

    for item in items {
        match (
            item.get("textDocument"),
            item.get("edits").and_then(Value::as_array),
        ) {
            (Some(text_document), Some(edits)) => {
                let Some(uri_str) = text_document.get("uri").and_then(Value::as_str) else {
                    passthrough_items.push(item.clone());
                    continue;
                };
                let Ok(uri) = uri_str.parse::<lsp_types::Uri>() else {
                    passthrough_items.push(item.clone());
                    continue;
                };
                if workspace.registry.is_generated(&uri::to_outou(&uri)) {
                    translated_edits.extend(translate_generated_edits(workspace, &uri, edits)?);
                } else {
                    passthrough_items.push(item.clone());
                }
            }
            _ => {
                if resource_op_touches_generated(workspace, item) {
                    return Err(UnmappableRename {
                        generated_uri: "<resource operation on generated code>".to_string(),
                    });
                }
                passthrough_items.push(item.clone());
            }
        }
    }

    let mut out: Vec<Value> = group_by_uri(dedup(translated_edits))
        .into_iter()
        .map(|(uri_str, edits)| document_text_edit_json(workspace, &uri_str, edits))
        .collect();
    out.extend(passthrough_items);
    Ok(Value::Array(out))
}

/// Whether a `documentChanges` resource operation (`{"kind": "create" |
/// "rename" | "delete", ...}`) names a generated file in any of its
/// URI-bearing fields (`uri`, `oldUri`, `newUri`).
fn resource_op_touches_generated(workspace: &Workspace, item: &Value) -> bool {
    ["uri", "oldUri", "newUri"]
        .iter()
        .filter_map(|field| item.get(field)?.as_str())
        .filter_map(|s| s.parse::<lsp_types::Uri>().ok())
        .any(|uri| workspace.registry.is_generated(&uri::to_outou(&uri)))
}

fn document_text_edit_json(workspace: &Workspace, uri_str: &str, edits: Vec<Value>) -> Value {
    let version = workspace.rsx.get(uri_str).map(|doc| doc.version);
    json!({
        "textDocument": { "uri": uri_str, "version": version },
        "edits": edits,
    })
}

/// Translates every edit of one generated file's `TextEdit[]` back to
/// `.rsx` coordinates via [`mapping::generated_range_to_all_sources`],
/// which already flattens a multi-source mapping (opening + closing tag)
/// into one entry per source. An edit whose generated range maps to no
/// source at all refuses the whole rename immediately, per this module's
/// own doc comment.
fn translate_generated_edits(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    edits: &[Value],
) -> Result<Vec<TranslatedEdit>, UnmappableRename> {
    let mut out = Vec::new();
    for edit in edits {
        let (Some(range), Some(new_text)) = (extract_edit_range(edit), extract_edit_new_text(edit))
        else {
            continue;
        };
        let sources = mapping::generated_range_to_all_sources(workspace, generated_uri, range)
            .unwrap_or_default();
        if sources.is_empty() {
            return Err(UnmappableRename {
                generated_uri: generated_uri.as_str().to_string(),
            });
        }
        out.extend(
            sources
                .into_iter()
                .map(|(uri, range)| (uri, range, new_text.clone())),
        );
    }
    Ok(out)
}

/// Drops exact duplicate `(uri, range, newText)` triples: several
/// generated edits (e.g. two separate rust-analyzer edits that both
/// reverse-map onto the same `.rsx` span) must collapse to one, never be
/// applied twice.
fn dedup(edits: Vec<TranslatedEdit>) -> Vec<TranslatedEdit> {
    let mut seen = HashSet::new();
    edits
        .into_iter()
        .filter(|(uri, range, text)| {
            seen.insert((uri.as_str().to_string(), range_key(*range), text.clone()))
        })
        .collect()
}

fn range_key(range: lsp_types::Range) -> (u32, u32, u32, u32) {
    (
        range.start.line,
        range.start.character,
        range.end.line,
        range.end.character,
    )
}

fn group_by_uri(edits: Vec<TranslatedEdit>) -> Vec<(String, Vec<Value>)> {
    let mut grouped: Vec<(String, Vec<Value>)> = Vec::new();
    for (uri, range, new_text) in edits {
        let uri_str = uri.as_str().to_string();
        let edit_json = json!({ "range": range, "newText": new_text });
        match grouped.iter_mut().find(|(u, _)| *u == uri_str) {
            Some((_, edits)) => edits.push(edit_json),
            None => grouped.push((uri_str, vec![edit_json])),
        }
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::RsxDocument;
    use outou_sourcemap::{
        file_uri, Mapping, MappingKind, Registry, SourceId, SourceMap, SourceSpan, Span,
    };
    use std::path::Path;

    /// A workspace with one component `<Greeting>hi</Greeting>` whose
    /// identifier is mapped from both tags to one generated span, exactly
    /// like `outou-backend-dioxus`'s real output.
    fn sample_workspace() -> (Workspace, lsp_types::Uri, lsp_types::Uri) {
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/main.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let rsx_source = "fn f() { <Greeting>hi</Greeting> }";
        let generated_text = "let _ = Greeting;";
        let open_start = rsx_source.find("Greeting").unwrap() as u32;
        let close_start = rsx_source.rfind("Greeting").unwrap() as u32;

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(8, 16),
                vec![
                    SourceSpan::new(SourceId(0), Span::new(open_start, open_start + 8)),
                    SourceSpan::new(SourceId(0), Span::new(close_start, close_start + 8)),
                ],
                MappingKind::Identifier,
            ),
        );
        let registry = Registry::new().with_map(map);

        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: std::collections::HashMap::new(),
            generated: std::collections::HashMap::new(),
            rsx_to_generated: std::collections::HashMap::new(),
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            RsxDocument::new(rsx_source.to_string(), 3),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );

        (
            workspace,
            uri::to_lsp(&rsx_uri),
            uri::to_lsp(&generated_uri),
        )
    }

    #[test]
    fn translate_workspace_edit_updates_both_the_opening_and_closing_tag() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let mut edit = json!({
            "changes": {
                generated_uri.as_str(): [
                    { "range": { "start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 16}}, "newText": "Hello" }
                ]
            }
        });

        translate_workspace_edit(&workspace, &mut edit).expect("translatable");

        let edits = edit["changes"][rsx_uri.as_str()].as_array().expect("edits");
        assert_eq!(edits.len(), 2, "{edits:#?}");
        for e in edits {
            assert_eq!(e["newText"], "Hello");
        }
    }

    #[test]
    fn translate_workspace_edit_passes_through_an_ordinary_rust_file_edit() {
        let (workspace, _rsx_uri, _generated_uri) = sample_workspace();
        let rs_uri = "file:///app/src/other.rs";
        let mut edit = json!({
            "changes": {
                rs_uri: [
                    { "range": { "start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 5}}, "newText": "Hello" }
                ]
            }
        });

        translate_workspace_edit(&workspace, &mut edit).expect("translatable");

        assert_eq!(edit["changes"][rs_uri][0]["newText"], "Hello");
    }

    #[test]
    fn translate_workspace_edit_refuses_when_a_generated_edit_has_no_source() {
        let (workspace, _rsx_uri, generated_uri) = sample_workspace();
        // Bytes 0..3 of the generated text ("let") are synthesized
        // scaffolding with no mapping at all.
        let mut edit = json!({
            "changes": {
                generated_uri.as_str(): [
                    { "range": { "start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}}, "newText": "xxx" }
                ]
            }
        });

        let result = translate_workspace_edit(&workspace, &mut edit);
        assert!(
            result.is_err(),
            "an unmappable edit must refuse the whole rename"
        );
    }

    #[test]
    fn translate_workspace_edit_deduplicates_document_changes_versioned_with_the_rsx_version() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let mut edit = json!({
            "documentChanges": [
                {
                    "textDocument": { "uri": generated_uri.as_str(), "version": 7 },
                    "edits": [
                        { "range": { "start": {"line": 0, "character": 8}, "end": {"line": 0, "character": 16}}, "newText": "Hello" }
                    ]
                }
            ]
        });

        translate_workspace_edit(&workspace, &mut edit).expect("translatable");

        let changes = edit["documentChanges"].as_array().unwrap();
        assert_eq!(changes.len(), 1, "{changes:#?}");
        assert_eq!(changes[0]["textDocument"]["uri"], rsx_uri.as_str());
        // The `.rsx` document's own version (3 in `sample_workspace`), not
        // the generated file's (7, as rust-analyzer sent it).
        assert_eq!(changes[0]["textDocument"]["version"], 3);
        assert_eq!(changes[0]["edits"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn translate_prepare_rename_response_maps_a_bare_range() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let mut value = json!({
            "start": {"line": 0, "character": 8},
            "end": {"line": 0, "character": 16}
        });
        translate_prepare_rename_response(&workspace, &generated_uri, &mut value);
        // Known limitation (issue #14 review, MEDIUM-8, see this
        // function's own TODO(phase0)): reverse-maps to the first
        // (opening tag) source per `generated_location_to_source`'s
        // multi-source rule, *even if* the original request's cursor was
        // on the closing tag — this test only exercises the generated
        // position both origins share, not a distinction this function
        // is able to make.
        let start = value["start"]["character"].as_u64().unwrap();
        let end = value["end"]["character"].as_u64().unwrap();
        let doc = workspace.rsx.get(rsx_uri.as_str()).unwrap();
        assert_eq!(
            &doc.line_index.text()[start as usize..end as usize],
            "Greeting"
        );
    }

    #[test]
    fn translate_prepare_rename_response_nulls_out_an_unmappable_position() {
        let (workspace, _rsx_uri, generated_uri) = sample_workspace();
        let mut value = json!({
            "start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 3}
        });
        translate_prepare_rename_response(&workspace, &generated_uri, &mut value);
        assert!(value.is_null());
    }

    #[test]
    fn translate_workspace_edit_refuses_a_resource_op_touching_a_generated_file() {
        let (workspace, _rsx_uri, generated_uri) = sample_workspace();
        let mut edit = json!({
            "documentChanges": [
                { "kind": "delete", "uri": generated_uri.as_str() }
            ]
        });
        let result = translate_workspace_edit(&workspace, &mut edit);
        assert!(result.is_err());
    }
}
