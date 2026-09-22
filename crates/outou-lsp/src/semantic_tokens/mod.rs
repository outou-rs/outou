//! `textDocument/semanticTokens/full`: rust-analyzer's tokens for the
//! generated file, decoded and mapped back through the source map
//! (`crate::mapping`'s registry, reused rather than duplicated), overlaid
//! with Outou's own AST-derived tokens (component, HTML element,
//! attribute, event, JSX text — see [`outou_tokens`]) and re-encoded.
//!
//! Split into [`legend`] (which legend is advertised and how Outou's own
//! token names resolve against it), [`token`] (the pure decode/encode/
//! split/merge transforms, independently unit-tested), and [`outou_tokens`]
//! (the pure AST walk). This file is only the orchestration: turning a
//! `Workspace` and a rust-analyzer response into the final JSON payload.
//! See `docs/adr/0012-semantic-tokens-legend-and-overlay.md` for the
//! design decisions (legend strategy, overlay precedence, degraded-mode
//! fallback).

pub mod legend;
pub mod outou_tokens;
pub mod token;

pub use legend::Legend;

use outou_sourcemap::{LineIndex, Position as OutouPosition, PositionRange as OutouPositionRange};
use serde_json::Value;

use crate::documents::Workspace;
use crate::mapping;
use crate::uri;
use token::AbsoluteToken;

/// Outou's own AST-derived tokens for `file`, in legend-indexed absolute
/// LSP coordinates. A token name with no match in `legend` is dropped
/// (should not happen for either legend this server ever chooses — see
/// `legend`'s own doc comment — but never silently mis-indexed).
fn outou_tokens_absolute(
    file: &outou_syntax::ast::File,
    line_index: &LineIndex,
    legend: &Legend,
) -> Vec<AbsoluteToken> {
    outou_tokens::collect(file)
        .into_iter()
        .filter_map(|named| {
            let type_index = legend.type_index(named.type_name)?;
            Some(token::span_to_line_tokens(
                line_index,
                named.span,
                type_index,
                named.modifiers,
            ))
        })
        .flatten()
        .collect()
}

/// Maps one rust-analyzer token (in the generated file's own coordinates)
/// back to zero or more `.rsx`-coordinate tokens belonging to
/// `rsx_uri_string`, reusing [`mapping::generated_range_to_all_sources`]
/// (issue #14 review, BLOCKING-1) — the same narrowing primitive rename
/// and references use, not the coarser whole-mapping-span
/// `outou_sourcemap::Registry::reverse`: a *verbatim* mapping (the common
/// case for a plain Rust token) narrows to the token's own exact bytes,
/// never the whole containing statement/item; a mapping with several
/// sources whose generated span exactly equals the query (an element name
/// mapped from both its opening and closing tag) is emitted at each, per
/// the issue's own requirement; anything unmappable (synthesized code, a
/// length-mismatched mapping, an undisambiguatable sub-range, or a source
/// belonging to a *different* `.rsx` file than the one this request is
/// for) is dropped.
fn map_ra_token(
    workspace: &Workspace,
    generated_uri: &lsp_types::Uri,
    rsx_uri_string: &str,
    token: AbsoluteToken,
) -> Vec<AbsoluteToken> {
    let range = lsp_types::Range::new(
        lsp_types::Position::new(token.line, token.start),
        lsp_types::Position::new(token.line, token.start.saturating_add(token.length)),
    );
    let Some(sources) = mapping::generated_range_to_all_sources(workspace, generated_uri, range)
    else {
        return Vec::new();
    };
    let Some(doc) = workspace.rsx.get(rsx_uri_string) else {
        return Vec::new();
    };
    sources
        .into_iter()
        .filter(|(uri, _)| uri::to_outou(uri).as_str() == rsx_uri_string)
        .flat_map(|(_, range)| {
            let span = doc.line_index.range_to_span(OutouPositionRange::new(
                OutouPosition::new(range.start.line, range.start.character),
                OutouPosition::new(range.end.line, range.end.character),
            ));
            token::span_to_line_tokens(&doc.line_index, span, token.token_type, token.modifiers)
        })
        .collect()
}

/// Encodes a final, merged token list into the `SemanticTokens` JSON shape.
fn encode_response(tokens: Vec<AbsoluteToken>) -> Value {
    let semantic_tokens = lsp_types::SemanticTokens {
        result_id: None,
        data: token::encode(tokens),
    };
    serde_json::to_value(semantic_tokens).unwrap_or(Value::Null)
}

/// Answers `textDocument/semanticTokens/full` with Outou's own tokens
/// alone, no rust-analyzer involved: used in degraded mode (no crate root)
/// and whenever a workspace exists but rust-analyzer is not attached — see
/// `crate::dispatch::requests::dispatch_semantic_tokens_request`. Still
/// useful on its own: JSX vocabulary (component vs. element, event
/// attributes, …) never depended on rust-analyzer in the first place.
pub fn local_only_response(source: &str, legend: &Legend) -> Value {
    let file = outou_syntax::parse(source).file;
    let line_index = LineIndex::new(source);
    let tokens = outou_tokens_absolute(&file, &line_index, legend);
    encode_response(tokens)
}

/// Rewrites a rust-analyzer `textDocument/semanticTokens/full` response
/// (a `SemanticTokens` object, or `null`) in place into the final
/// `.rsx`-coordinate response: rust-analyzer's tokens are decoded, mapped
/// back through the source map, and merged with Outou's own AST-derived
/// overlay (`crate::semantic_tokens::token::merge`'s overlay-wins
/// precedence). A `null`/unparsable rust-analyzer response still produces
/// Outou's own tokens rather than an empty result, exactly like
/// [`local_only_response`] would for this document.
pub fn rewrite_response(
    workspace: &Workspace,
    legend: &Legend,
    generated_uri: &lsp_types::Uri,
    rsx_uri_string: &str,
    value: &mut Value,
) {
    let generated = uri::to_outou(generated_uri);
    let ra_tokens = match workspace.generated.get(generated.as_str()) {
        Some(unit) => {
            // Issue #14 review (SHOULD-LAND-8): a token whose decoded
            // line falls at or past the generated file's own last line —
            // malformed rust-analyzer output, or a decode that saturated
            // rather than panicked on a pathological delta — has nothing
            // sensible to map against and is dropped before it ever
            // reaches `map_ra_token`'s own `Position`/`LineIndex` calls.
            let last_line = unit.line_index.line_count();
            decode_ra_tokens(value)
                .into_iter()
                .filter(|t| t.line < last_line)
                .flat_map(|t| map_ra_token(workspace, generated_uri, rsx_uri_string, t))
                .collect()
        }
        None => Vec::new(),
    };

    let Some(doc) = workspace.rsx.get(rsx_uri_string) else {
        *value = Value::Null;
        return;
    };
    let file = outou_syntax::parse(doc.line_index.text()).file;
    let outou_tokens = outou_tokens_absolute(&file, &doc.line_index, legend);

    *value = encode_response(token::merge(ra_tokens, outou_tokens));
}

fn decode_ra_tokens(value: &Value) -> Vec<AbsoluteToken> {
    if value.is_null() {
        return Vec::new();
    }
    match serde_json::from_value::<lsp_types::SemanticTokens>(value.clone()) {
        Ok(tokens) => token::decode(&tokens.data),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use outou_sourcemap::{file_uri, Mapping, MappingKind, Registry, SourceId, SourceMap, Span};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn local_only_response_encodes_a_component_tag_as_a_class_token() {
        let legend = Legend::default_legend();
        let value = local_only_response("fn f() { <Greeting>hi</Greeting> }", &legend);
        let tokens: lsp_types::SemanticTokens = serde_json::from_value(value).unwrap();
        // 2 component-name tokens (open + close) + 1 text token.
        assert_eq!(tokens.data.len(), 3);
        let class_index = legend.type_index(legend::COMPONENT_TYPE).unwrap();
        assert_eq!(tokens.data[0].token_type, class_index);
    }

    fn sample_workspace() -> (Workspace, lsp_types::Uri, lsp_types::Uri) {
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/main.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let rsx_source = "fn f() { <Greeting>hi</Greeting> }";
        // "Greeting" appears once in generated text (bytes 9..17), mapping
        // back to *two* rsx sources: the opening name (10..18) and the
        // closing name (25..33), mirroring
        // `outou-backend-dioxus::element::lower_element_body`.
        let generated_text = "let _ = Greeting;";
        let open_start = rsx_source.find("Greeting").unwrap() as u32;
        let close_start = rsx_source.rfind("Greeting").unwrap() as u32;

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(8, 16),
                vec![
                    outou_sourcemap::SourceSpan::new(
                        SourceId(0),
                        Span::new(open_start, open_start + 8),
                    ),
                    outou_sourcemap::SourceSpan::new(
                        SourceId(0),
                        Span::new(close_start, close_start + 8),
                    ),
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
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
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
    fn rewrite_response_emits_a_rust_analyzer_token_at_each_of_its_several_sources() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let legend = Legend::default_legend();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();

        // rust-analyzer's own token for the generated identifier "Greeting"
        // at bytes 8..16 -> line 0, chars 8..16.
        let mut value = json!({
            "data": [0, 8, 8, legend.type_index("function").unwrap(), 0]
        });
        rewrite_response(
            &workspace,
            &legend,
            &generated_uri,
            &rsx_uri_string,
            &mut value,
        );

        let tokens: lsp_types::SemanticTokens = serde_json::from_value(value).unwrap();
        let function_index = legend.type_index("function").unwrap();
        let function_tokens: Vec<_> = tokens
            .data
            .iter()
            .filter(|t| t.token_type == function_index)
            .collect();
        // The single rust-analyzer token reverse-maps to two `.rsx` spans
        // (open and close tag names) and is emitted at each — but since
        // Outou's own overlay also produces a `class` token at each of the
        // same two spans and wins the overlap, no bare `function`-typed
        // token should actually survive.
        assert!(function_tokens.is_empty(), "{tokens:#?}");
        let class_index = legend.type_index(legend::COMPONENT_TYPE).unwrap();
        assert_eq!(
            tokens
                .data
                .iter()
                .filter(|t| t.token_type == class_index)
                .count(),
            2
        );
    }

    /// A workspace whose generated Rust is a verbatim-copied function
    /// signature (single-source, equal-length `Expression` mapping) plus
    /// a length-mismatched attribute mapping — the corpus-derived fixture
    /// `mapping.rs`'s own `narrow_fixture_workspace` uses, rebuilt here so
    /// `rewrite_response`'s own tests exercise the same shapes end to end.
    fn signature_workspace() -> (Workspace, lsp_types::Uri, lsp_types::Uri, String) {
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/main.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        // "fn Greeting(x: i32) {}" copied verbatim (byte-identical, same
        // length) into the generated file at offset 10; a length-
        // mismatched `Attribute` mapping (6 generated bytes standing in
        // for `r#type`, 4 source bytes for `type`) at offset 40.
        let rsx_source = "fn Greeting(x: i32) {} type";
        let sig_text = "fn Greeting(x: i32) {}";
        let type_start = rsx_source.rfind("type").unwrap() as u32;

        let generated_text = format!(
            "{}{}{}{}",
            "a".repeat(10),
            sig_text,
            "b".repeat(10),
            "ABCDEF",
        );
        let sig_generated_start = 10u32;
        let type_generated_start = 10 + sig_text.len() as u32 + 10;

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()])
            .with_mapping(Mapping::new(
                Span::new(
                    sig_generated_start,
                    sig_generated_start + sig_text.len() as u32,
                ),
                vec![outou_sourcemap::SourceSpan::new(
                    SourceId(0),
                    Span::new(0, sig_text.len() as u32),
                )],
                MappingKind::Expression,
            ))
            .with_mapping(Mapping::new(
                Span::new(type_generated_start, type_generated_start + 6),
                vec![outou_sourcemap::SourceSpan::new(
                    SourceId(0),
                    Span::new(type_start, type_start + 4),
                )],
                MappingKind::Attribute,
            ));
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
            crate::documents::RsxDocument::new(rsx_source.to_string(), 0),
        );
        workspace.generated.insert(
            generated_uri.as_str().to_string(),
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated_text),
        );
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );

        (
            workspace,
            uri::to_lsp(&rsx_uri),
            uri::to_lsp(&generated_uri),
            rsx_source.to_string(),
        )
    }

    /// Issue #14 review, BLOCKING-1/2: three separate rust-analyzer tokens
    /// (`fn`, `Greeting`, the `(x: i32)` parameter list) all fall *inside*
    /// one coarse verbatim mapping. Each must narrow to its own exact
    /// `.rsx` sub-range — never the whole mapping's span, which would make
    /// them identical and overlapping.
    #[test]
    fn rewrite_response_never_emits_overlapping_tokens() {
        let (workspace, rsx_uri, generated_uri, rsx_source) = signature_workspace();
        let legend = Legend::default_legend();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
        let keyword_index = legend.type_index("keyword").unwrap();

        // Generated bytes: 10.."fn" (0..2 within sig, generated 10..12),
        // "Greeting" (sig offset 3..11, generated 13..21), "x" (sig offset
        // 12..13, generated 22..23) — all inside the one verbatim mapping
        // (generated 10..32).
        let mut value = json!({
            "data": [
                0, 10, 2, keyword_index, 0,
                0, 3, 8, legend.type_index("function").unwrap(), 0,
                0, 9, 1, legend.type_index("parameter").unwrap(), 0,
            ]
        });
        rewrite_response(
            &workspace,
            &legend,
            &generated_uri,
            &rsx_uri_string,
            &mut value,
        );

        let tokens: lsp_types::SemanticTokens = serde_json::from_value(value).unwrap();
        assert!(!tokens.data.is_empty(), "{tokens:#?}");

        // Decode back to absolute coordinates and assert strict,
        // non-overlapping order on every line.
        let absolute = token::decode(&tokens.data);
        for pair in absolute.windows(2) {
            let (prev, next) = (pair[0], pair[1]);
            if prev.line == next.line {
                assert!(
                    prev.start + prev.length <= next.start,
                    "overlapping tokens: {prev:?} then {next:?}"
                );
            }
        }
        let _ = rsx_source;
    }

    /// A rust-analyzer token whose generated span falls inside a
    /// length-mismatched mapping (the `r#type`/`type` case) must not
    /// produce any `.rsx` token at all — never a proportional guess, and
    /// never the whole mismatched span.
    #[test]
    fn rewrite_response_drops_a_token_inside_a_length_mismatched_mapping() {
        let (workspace, rsx_uri, generated_uri, _rsx_source) = signature_workspace();
        let legend = Legend::default_legend();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();

        // Generated bytes 42..46 (inside the 42..48 `Attribute` mapping
        // standing in for `r#type`: 10 ("a" prefix) + 22 (`sig_text`) + 10
        // (`b` mid) = 42).
        let mut value = json!({
            "data": [0, 42, 4, legend.type_index("property").unwrap(), 0]
        });
        rewrite_response(
            &workspace,
            &legend,
            &generated_uri,
            &rsx_uri_string,
            &mut value,
        );

        let tokens: lsp_types::SemanticTokens = serde_json::from_value(value).unwrap();
        let property_index = legend.type_index("property").unwrap();
        assert!(
            tokens.data.iter().all(|t| t.token_type != property_index),
            "{tokens:#?}"
        );
        let _ = rsx_uri;
    }

    /// Issue #14 review (SHOULD-LAND-8): a rust-analyzer token whose line
    /// is at or past the generated file's own last line (malformed input,
    /// or a saturated decode of a pathological delta) must not reach the
    /// `Position`/`LineIndex` mapping machinery at all — dropped, not a
    /// panic and not a wrong-but-plausible token.
    #[test]
    fn rewrite_response_drops_a_token_beyond_the_generated_files_last_line() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let legend = Legend::default_legend();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();

        // The fixture's generated text is a single line; line 9999 does
        // not exist.
        let mut value = json!({
            "data": [9999, 0, 1, legend.type_index("function").unwrap(), 0]
        });
        rewrite_response(
            &workspace,
            &legend,
            &generated_uri,
            &rsx_uri_string,
            &mut value,
        ); // must not panic

        let tokens: lsp_types::SemanticTokens = serde_json::from_value(value).unwrap();
        let function_index = legend.type_index("function").unwrap();
        assert!(
            tokens.data.iter().all(|t| t.token_type != function_index),
            "{tokens:#?}"
        );
    }

    #[test]
    fn rewrite_response_falls_back_to_outou_only_tokens_for_a_null_ra_response() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let legend = Legend::default_legend();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();

        let mut value = Value::Null;
        rewrite_response(
            &workspace,
            &legend,
            &generated_uri,
            &rsx_uri_string,
            &mut value,
        );

        let tokens: lsp_types::SemanticTokens = serde_json::from_value(value).unwrap();
        assert!(!tokens.data.is_empty());
    }
}
