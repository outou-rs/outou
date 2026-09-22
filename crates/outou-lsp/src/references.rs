//! `textDocument/references`: forwarded to rust-analyzer at the mapped
//! generated position (reusing `crate::mapping::rsx_position_to_generated`,
//! the same helper hover/definition/completion/rename already use — a
//! cursor on a closing tag name maps correctly with no special case, per
//! ADR 0007), and the resulting `Location[]` translated back here.
//!
//! Reuses [`crate::mapping::generated_range_to_all_sources`] — the same
//! function `crate::rename` uses for `WorkspaceEdit` translation — so a
//! component's generated identifier occurrence, which maps back to both
//! its opening and closing tag (ADR 0007's own example), naturally
//! produces a reference location at *each*, satisfying "include the
//! closing tag occurrences" with no extra code.
//!
//! Unlike rename, an unmappable location (synthesized code with no
//! source) is simply **dropped**, never refused: `textDocument/references`
//! is read-only, so silently omitting one result that could not be placed
//! anywhere meaningful in `.rsx` loses nothing but a stray entry, which is
//! a fine trade for never leaking a generated-file location.

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::documents::Workspace;
use crate::mapping;

/// Rewrites a `textDocument/references` response (`null` or a
/// `Location[]`) in place: every location inside a known generated file is
/// mapped back to its `.rsx` equivalent (or equivalents, for a
/// multi-source mapping), unmappable ones are dropped, and everything else
/// (an ordinary `.rs` location) passes through unchanged. The result is
/// deduplicated: several generated occurrences reverse-mapping to the same
/// `.rsx` location collapse to one.
pub fn translate_references_response(workspace: &Workspace, value: &mut Value) {
    let Some(items) = value.as_array() else {
        return;
    };

    let mut out: Vec<(lsp_types::Uri, lsp_types::Range)> = Vec::new();
    for item in items {
        let Some(uri) = item
            .get("uri")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<lsp_types::Uri>().ok())
        else {
            continue;
        };
        let Some(range) = item
            .get("range")
            .and_then(|r| serde_json::from_value::<lsp_types::Range>(r.clone()).ok())
        else {
            continue;
        };
        // `None` means "the registry itself does not know this URI as
        // generated at all" (an ordinary `.rs` module or a dependency
        // crate, ADR 0007's reverse-mapping rule) — never merely "no live
        // `GeneratedUnit` right now" (issue #14 review, SHOULD-LAND-11:
        // `generated_range_to_all_sources` itself checks the registry
        // first for exactly this reason), so passing it through here can
        // never leak a `.generated/…` location.
        match mapping::generated_range_to_all_sources(workspace, &uri, range) {
            None => out.push((uri, range)),
            Some(sources) => out.extend(sources),
        }
    }

    *value = Value::Array(dedup(out).into_iter().map(location_json).collect());
}

fn location_json((uri, range): (lsp_types::Uri, lsp_types::Range)) -> Value {
    json!({ "uri": uri, "range": range })
}

fn dedup(
    locations: Vec<(lsp_types::Uri, lsp_types::Range)>,
) -> Vec<(lsp_types::Uri, lsp_types::Range)> {
    let mut seen = HashSet::new();
    locations
        .into_iter()
        .filter(|(uri, range)| seen.insert((uri.as_str().to_string(), range_key(*range))))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::RsxDocument;
    use crate::uri;
    use outou_sourcemap::{
        file_uri, Mapping, MappingKind, Registry, SourceId, SourceMap, SourceSpan, Span,
    };
    use std::path::Path;

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
            RsxDocument::new(rsx_source.to_string(), 1),
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
    fn translate_references_response_emits_both_tags_for_one_generated_occurrence() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let mut value = json!([
            { "uri": generated_uri.as_str(), "range": { "start": {"line":0,"character":8}, "end": {"line":0,"character":16}}}
        ]);
        translate_references_response(&workspace, &mut value);
        let items = value.as_array().unwrap();
        assert_eq!(items.len(), 2, "{items:#?}");
        for item in items {
            assert_eq!(item["uri"], rsx_uri.as_str());
        }
    }

    #[test]
    fn translate_references_response_drops_an_unmappable_location() {
        let (workspace, _rsx_uri, generated_uri) = sample_workspace();
        let mut value = json!([
            { "uri": generated_uri.as_str(), "range": { "start": {"line":0,"character":0}, "end": {"line":0,"character":3}}}
        ]);
        translate_references_response(&workspace, &mut value);
        assert_eq!(value.as_array().unwrap().len(), 0);
    }

    #[test]
    fn translate_references_response_passes_through_an_ordinary_rust_file_location() {
        let (workspace, _rsx_uri, _generated_uri) = sample_workspace();
        let rs_uri = "file:///app/src/other.rs";
        let mut value = json!([
            { "uri": rs_uri, "range": { "start": {"line":1,"character":0}, "end": {"line":1,"character":5}}}
        ]);
        translate_references_response(&workspace, &mut value);
        assert_eq!(value.as_array().unwrap()[0]["uri"], rs_uri);
    }

    #[test]
    fn translate_references_response_deduplicates_identical_locations() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let mut value = json!([
            { "uri": generated_uri.as_str(), "range": { "start": {"line":0,"character":8}, "end": {"line":0,"character":16}}},
            { "uri": generated_uri.as_str(), "range": { "start": {"line":0,"character":8}, "end": {"line":0,"character":16}}}
        ]);
        translate_references_response(&workspace, &mut value);
        // Two identical generated queries each expand to the same 2
        // `.rsx` locations (open + close); deduplicated down to 2, not 4.
        let items = value.as_array().unwrap();
        assert_eq!(items.len(), 2, "{items:#?}");
        let _ = rsx_uri;
    }
}
