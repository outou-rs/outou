//! Builds and publishes `.rsx` diagnostics: Outou's own syntax
//! diagnostics from the last parse, merged with the last rust-analyzer
//! diagnostics for that file's generated unit, mapped back through the
//! registry and translated out of backend vocabulary
//! (`crate::translate`).

use std::collections::HashMap;

use lsp_server::{Connection, Message, Notification as LspNotification};
use lsp_types::{Diagnostic, DiagnosticSeverity, PublishDiagnosticsParams};
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
                if let MappedLocation::Source {
                    uri: source_uri,
                    range,
                } =
                    mapping::generated_location_to_source(workspace, &generated_uri_lsp, raw.range)
                {
                    if uri::to_outou(&source_uri).as_str() == rsx_uri_string {
                        let mut mapped = raw.clone();
                        mapped.range = range;
                        diagnostics.push(translate::translate_diagnostic(mapped));
                    }
                }
            }
        }
    }

    publish(connection, rsx_uri_string, diagnostics);
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
    publish(connection, rsx_uri_string, diagnostics);
}

fn publish(connection: &Connection, rsx_uri_string: &str, diagnostics: Vec<Diagnostic>) {
    let params = PublishDiagnosticsParams {
        uri: uri::to_lsp(&outou_sourcemap::Uri::new(rsx_uri_string)),
        diagnostics,
        version: None,
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
