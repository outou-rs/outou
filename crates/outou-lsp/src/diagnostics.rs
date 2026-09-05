//! Builds and publishes `.rsx` diagnostics: Outou's own syntax
//! diagnostics from the last parse, merged with the last rust-analyzer
//! diagnostics for that file's generated unit, mapped back through the
//! registry and translated out of backend vocabulary
//! (`crate::translate`).

use std::collections::HashMap;

use lsp_server::{Connection, Message, Notification as LspNotification};
use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, PublishDiagnosticsParams,
};
use outou_sourcemap::LineIndex;
use outou_syntax::{Diagnostic as SyntaxDiagnostic, Severity};

use crate::documents::{RsxDocument, Workspace};
use crate::mapping::{self, MappedLocation};
use crate::{translate, uri};

/// Converts one Outou syntax diagnostic to its LSP shape.
fn syntax_to_lsp(diagnostic: &SyntaxDiagnostic, line_index: &LineIndex) -> Diagnostic {
    Diagnostic {
        range: mapping::to_lsp_range(line_index.span_to_range(diagnostic.span)),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        source: Some("outou".to_string()),
        message: diagnostic.message.clone(),
        ..Default::default()
    }
}

/// Publishes the merged diagnostic set for one `.rsx` file: its own
/// syntax diagnostics, plus the last rust-analyzer diagnostics for its
/// generated unit that map back to it.
pub fn publish_for_rsx(connection: &Connection, workspace: &Workspace, rsx_uri_string: &str) {
    let Some(doc) = workspace.rsx.get(rsx_uri_string) else {
        return;
    };
    let mut diagnostics: Vec<Diagnostic> = doc
        .diagnostics
        .iter()
        .map(|d| syntax_to_lsp(d, &doc.line_index))
        .collect();

    if let Some(generated_uri_string) = workspace.rsx_to_generated.get(rsx_uri_string) {
        if let Some(unit) = workspace.generated.get(generated_uri_string) {
            let generated_uri_lsp = uri::to_lsp(&unit.generated_uri);
            for raw in &unit.last_ra_diagnostics {
                if let Some(mapped) =
                    merge_ra_diagnostic(workspace, &generated_uri_lsp, rsx_uri_string, raw)
                {
                    diagnostics.push(mapped);
                }
            }
        }
    }

    publish(connection, rsx_uri_string, diagnostics, doc.version);
}

/// Maps one rust-analyzer/flycheck diagnostic (in generated coordinates)
/// back onto `rsx_uri_string`, or returns `None` if it belongs to a
/// different `.rsx` file, or has no source and is not severe enough to
/// force a fallback position.
///
/// A mapped `severity == ERROR` diagnostic is never dropped just because
/// its exact range is unmapped (issue #9 Gate 3 review, M5): a hard
/// compile error — a missing required prop, most concretely — must still
/// reach the user, even approximately positioned, rather than vanish or
/// silently survive as a lower-severity hint (as it did before this fix,
/// via `translate_diagnostic`'s generic rewrite keeping the diagnostic's
/// own `severity` field intact but the diagnostic itself being dropped
/// upstream whenever its span had no direct mapping).
fn merge_ra_diagnostic(
    workspace: &Workspace,
    generated_uri_lsp: &lsp_types::Uri,
    rsx_uri_string: &str,
    raw: &Diagnostic,
) -> Option<Diagnostic> {
    let is_error = raw.severity == Some(DiagnosticSeverity::ERROR);
    let range = match mapping::generated_location_to_source(workspace, generated_uri_lsp, raw.range)
    {
        MappedLocation::Source {
            uri: source_uri,
            range,
        } => {
            if uri::to_outou(&source_uri).as_str() != rsx_uri_string {
                return None;
            }
            range
        }
        MappedLocation::Unmapped | MappedLocation::Unchanged if is_error => {
            mapping::nearest_source_position(workspace, generated_uri_lsp, raw.range)
        }
        MappedLocation::Unmapped | MappedLocation::Unchanged => return None,
    };

    let mut mapped = raw.clone();
    mapped.range = range;
    if let Some(infos) = mapped.related_information.take() {
        mapped.related_information = Some(sanitize_related_information(workspace, infos));
    }
    Some(translate::translate_diagnostic(mapped))
}

/// Recursively maps every `relatedInformation` entry's location back to
/// its `.rsx` source (issue #9 Gate 3 review, M4/HIGH-9(b)): rust-analyzer
/// never reverse-maps these itself, so left alone they point straight at
/// a `src/.generated/…` URI, at generated line numbers — exactly what
/// this server exists to prevent for the diagnostic's own `range`. An
/// entry whose location has no source (synthesized code) is dropped
/// rather than shown pointing at generated Rust; one whose location this
/// registry did not produce at all (a dependency, a plain `.rs` file) is
/// kept unchanged. Every surviving entry's `message` is also translated.
fn sanitize_related_information(
    workspace: &Workspace,
    infos: Vec<DiagnosticRelatedInformation>,
) -> Vec<DiagnosticRelatedInformation> {
    infos
        .into_iter()
        .filter_map(|mut info| {
            match mapping::generated_location_to_source(
                workspace,
                &info.location.uri,
                info.location.range,
            ) {
                MappedLocation::Source { uri, range } => {
                    info.location.uri = uri;
                    info.location.range = range;
                }
                MappedLocation::Unchanged => {}
                MappedLocation::Unmapped => return None,
            }
            info.message = translate::translate_message(&info.message);
            Some(info)
        })
        .collect()
}

/// Publishes Outou syntax diagnostics for a `.rsx` file in degraded mode
/// (no crate root, so there is no generated unit or registry to merge
/// against).
pub fn publish_degraded(
    connection: &Connection,
    docs: &HashMap<String, RsxDocument>,
    rsx_uri_string: &str,
) {
    let Some(doc) = docs.get(rsx_uri_string) else {
        return;
    };
    let diagnostics = doc
        .diagnostics
        .iter()
        .map(|d| syntax_to_lsp(d, &doc.line_index))
        .collect();
    publish(connection, rsx_uri_string, diagnostics, doc.version);
}

fn publish(
    connection: &Connection,
    rsx_uri_string: &str,
    diagnostics: Vec<Diagnostic>,
    version: i32,
) {
    let params = PublishDiagnosticsParams {
        uri: uri::to_lsp(&outou_sourcemap::Uri::new(rsx_uri_string)),
        diagnostics,
        // `0` means "read from disk, never opened/edited" (`RsxDocument`'s
        // own doc comment); LSP has no such sentinel, so that case alone
        // omits `version` rather than publishing a nonsensical `0`.
        version: (version != 0).then_some(version),
    };
    let notification = LspNotification::new("textDocument/publishDiagnostics".to_string(), params);
    let _ = connection.sender.send(Message::Notification(notification));
}

/// Handles a `textDocument/publishDiagnostics` notification from
/// rust-analyzer for one *generated* file: stores the raw diagnostics on
/// that unit and republishes the merged set for its `.rsx` source.
pub fn handle_ra_publish(
    connection: &Connection,
    workspace: &mut Workspace,
    params: PublishDiagnosticsParams,
) {
    let generated_uri = uri::to_outou(&params.uri);
    let rsx_uri_string = {
        let Some(unit) = workspace.generated.get_mut(generated_uri.as_str()) else {
            return;
        };
        unit.last_ra_diagnostics = params.diagnostics;
        unit.rsx_uri.as_str().to_string()
    };
    publish_for_rsx(connection, workspace, &rsx_uri_string);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{Location, NumberOrString, Position, Range};
    use outou_sourcemap::{file_uri, Mapping, MappingKind, SourceId, SourceMap, Span};
    use std::path::Path;

    /// Same fixture shape as `mapping::tests::sample_workspace`: one
    /// generated unit mapping generated bytes 30..34 back to `.rsx` bytes
    /// 4..8 (`user`).
    fn sample_workspace() -> (Workspace, lsp_types::Uri, lsp_types::Uri) {
        let rsx_path = Path::new("/app/src/main.rsx");
        let generated_path = Path::new("/app/src/.generated/crate-root.rs");
        let rsx_uri = file_uri(rsx_path);
        let generated_uri = file_uri(generated_path);

        let map = SourceMap::new(generated_uri.clone(), vec![rsx_uri.clone()]).with_mapping(
            Mapping::new(
                Span::new(30, 34),
                vec![outou_sourcemap::SourceSpan::new(
                    SourceId(0),
                    Span::new(4, 8),
                )],
                MappingKind::Identifier,
            ),
        );
        let registry = outou_sourcemap::Registry::new().with_map(map);

        let mut workspace = Workspace {
            manifest_dir: std::path::PathBuf::from("/app"),
            plan: None,
            registry,
            rsx: HashMap::new(),
            generated: HashMap::new(),
            rsx_to_generated: HashMap::new(),
        };
        workspace.rsx.insert(
            rsx_uri.as_str().to_string(),
            RsxDocument::new("let user = load_user();\n".to_string(), 3),
        );
        let generated_text = format!("{}user{}", "x".repeat(30), "y".repeat(10));
        let mut unit =
            crate::documents::test_generated_unit(&generated_uri, &rsx_uri, &generated_text);
        unit.last_ra_diagnostics = Vec::new();
        workspace
            .generated
            .insert(generated_uri.as_str().to_string(), unit);
        workspace.rsx_to_generated.insert(
            rsx_uri.as_str().to_string(),
            generated_uri.as_str().to_string(),
        );

        (
            workspace,
            crate::uri::to_lsp(&rsx_uri),
            crate::uri::to_lsp(&generated_uri),
        )
    }

    #[test]
    fn merge_ra_diagnostic_keeps_a_mapped_diagnostic() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
        let raw = Diagnostic {
            range: Range::new(Position::new(0, 30), Position::new(0, 34)),
            severity: Some(DiagnosticSeverity::ERROR),
            message: "mismatched types".to_string(),
            ..Default::default()
        };
        let merged = merge_ra_diagnostic(&workspace, &generated_uri, &rsx_uri_string, &raw)
            .expect("a mapped diagnostic is kept");
        assert_eq!(merged.range.start, Position::new(0, 4));
        assert_eq!(merged.range.end, Position::new(0, 8));
        assert!(merged.data.is_none());
    }

    /// M5: an unmapped diagnostic (points at synthesized code, e.g. a
    /// `rsx!` wrapper) with `severity == ERROR` must still be published,
    /// at the nearest mapped source position, never silently dropped.
    #[test]
    fn merge_ra_diagnostic_keeps_an_unmapped_error_at_the_nearest_position() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
        let raw = Diagnostic {
            range: Range::new(Position::new(0, 60), Position::new(0, 64)),
            severity: Some(DiagnosticSeverity::ERROR),
            message:
                "argument of type UserCardPropsBuilder_Error_Missing_required_field_user is missing"
                    .to_string(),
            ..Default::default()
        };
        let merged = merge_ra_diagnostic(&workspace, &generated_uri, &rsx_uri_string, &raw)
            .expect("an unmapped ERROR diagnostic must still be published");
        assert_eq!(merged.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(merged.range.start, Position::new(0, 4));
    }

    /// A non-error diagnostic with no mapping is still dropped: only
    /// `severity == ERROR` forces the nearest-position fallback.
    #[test]
    fn merge_ra_diagnostic_drops_an_unmapped_non_error() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
        let raw = Diagnostic {
            range: Range::new(Position::new(0, 60), Position::new(0, 64)),
            severity: Some(DiagnosticSeverity::HINT),
            message: "note".to_string(),
            ..Default::default()
        };
        assert!(merge_ra_diagnostic(&workspace, &generated_uri, &rsx_uri_string, &raw).is_none());
    }

    #[test]
    fn merge_ra_diagnostic_maps_related_information_and_drops_unmapped_entries() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
        let raw = Diagnostic {
            range: Range::new(Position::new(0, 30), Position::new(0, 34)),
            severity: Some(DiagnosticSeverity::ERROR),
            message: "mismatched types".to_string(),
            related_information: Some(vec![
                DiagnosticRelatedInformation {
                    location: Location {
                        uri: generated_uri.clone(),
                        range: Range::new(Position::new(0, 30), Position::new(0, 34)),
                    },
                    message: "original diagnostic".to_string(),
                },
                DiagnosticRelatedInformation {
                    location: Location {
                        uri: generated_uri.clone(),
                        range: Range::new(Position::new(0, 60), Position::new(0, 64)),
                    },
                    message: "synthesized, no source".to_string(),
                },
            ]),
            ..Default::default()
        };
        let merged =
            merge_ra_diagnostic(&workspace, &generated_uri, &rsx_uri_string, &raw).expect("mapped");
        let infos = merged.related_information.expect("kept related info");
        assert_eq!(infos.len(), 1, "the unmapped entry must be dropped");
        assert_eq!(infos[0].location.uri, rsx_uri);
        assert_eq!(infos[0].location.range.start, Position::new(0, 4));
    }

    #[test]
    fn merge_ra_diagnostic_never_leaves_data_populated() {
        let (workspace, rsx_uri, generated_uri) = sample_workspace();
        let rsx_uri_string = uri::to_outou(&rsx_uri).as_str().to_string();
        let raw = Diagnostic {
            range: Range::new(Position::new(0, 30), Position::new(0, 34)),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("E0308".to_string())),
            message: "rsx! macro expansion failed".to_string(),
            data: Some(serde_json::json!({ "leak": true })),
            ..Default::default()
        };
        let merged =
            merge_ra_diagnostic(&workspace, &generated_uri, &rsx_uri_string, &raw).expect("mapped");
        assert!(merged.data.is_none());
    }
}
