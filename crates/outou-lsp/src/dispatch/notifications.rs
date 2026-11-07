//! `.rsx` document lifecycle notifications from the editor:
//! `didOpen`/`didChange`/`didSave`/`didClose`, plus `$/cancelRequest`
//! (dispatched from here since it arrives as a notification, but
//! resolved against the pending-request map owned by
//! `crate::dispatch::requests`). Everything else is forwarded to
//! rust-analyzer untouched.

use std::collections::{HashMap, HashSet};

use lsp_server::{Connection, Message, Notification};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, MessageType, ShowMessageParams,
};
use serde_json::json;

use super::requests::handle_cancel_request;
use super::State;
use crate::documents;
use crate::{diagnostics, plan, uri};

pub(crate) fn dispatch_client_notification(
    state: &mut State,
    connection: &Connection,
    note: Notification,
) {
    match note.method.as_str() {
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(note.params) {
                if is_outou_rsx(&params.text_document.uri, &params.text_document.language_id) {
                    let rsx_uri = uri::to_outou(&params.text_document.uri)
                        .as_str()
                        .to_string();
                    handle_rsx_change(
                        state,
                        connection,
                        rsx_uri,
                        params.text_document.text,
                        params.text_document.version,
                    );
                } else if let Some(ra) = &state.ra {
                    ra.notify(
                        "textDocument/didOpen",
                        serde_json::to_value(params).unwrap(),
                    );
                }
            }
        }
        "textDocument/didChange" => {
            if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(note.params) {
                let is_rsx = params.text_document.uri.as_str().ends_with(".rsx");
                if is_rsx {
                    let version = params.text_document.version;
                    let rsx_uri = uri::to_outou(&params.text_document.uri)
                        .as_str()
                        .to_string();
                    if let Some(change) = params.content_changes.into_iter().next_back() {
                        handle_rsx_change(state, connection, rsx_uri, change.text, version);
                    }
                } else if let Some(ra) = &state.ra {
                    ra.notify(
                        "textDocument/didChange",
                        serde_json::to_value(params).unwrap(),
                    );
                }
            }
        }
        "textDocument/didClose" => {
            if let Ok(params) = serde_json::from_value::<DidCloseTextDocumentParams>(note.params) {
                if params.text_document.uri.as_str().ends_with(".rsx") {
                    handle_rsx_close(state, connection, &params.text_document.uri);
                }
            }
            // The generated overlay stays open on rust-analyzer for the
            // life of the session regardless; only a full re-plan ever
            // closes one.
        }
        "textDocument/didSave" => {
            if let Ok(params) = serde_json::from_value::<DidSaveTextDocumentParams>(note.params) {
                if params.text_document.uri.as_str().ends_with(".rsx") {
                    handle_rsx_save(state, connection, &params.text_document.uri);
                } else {
                    replan_after_rust_save(state, connection);
                    if let Some(ra) = &state.ra {
                        ra.notify(
                            "textDocument/didSave",
                            serde_json::to_value(params).unwrap(),
                        );
                    }
                }
            }
        }
        "workspace/didChangeWatchedFiles" => {
            super::watched_files::handle_did_change_watched_files(state, connection, note.params);
        }
        "$/cancelRequest" => handle_cancel_request(state, note.params),
        "window/workDoneProgress/cancel" => {
            let mut params = note.params;
            if let Some(token) = params.get("token") {
                params["token"] = state.progress_tokens.upstream_for(token);
            }
            if let Some(ra) = &state.ra {
                ra.notify(note.method, params);
            }
        }
        _ => {
            if let Some(ra) = &state.ra {
                ra.notify(note.method, note.params);
            }
        }
    }
}

/// A plain `.rs` file declared the module a plan error is about (or any
/// other `.rs` file was saved while one stands): re-plan from disk, so
/// saving the fix clears the error. `**/*.rs` is not a watcher glob (the
/// server's own `.generated/*.rs` writes would trigger it), and an editor's
/// save is the signal available for these files.
fn replan_after_rust_save(state: &mut State, connection: &Connection) {
    if state
        .workspace
        .as_ref()
        .is_some_and(|workspace| workspace.plan_error.is_some())
    {
        super::watched_files::replan_from_disk(state, connection);
    }
}

fn is_outou_rsx(uri: &lsp_types::Uri, language_id: &str) -> bool {
    language_id == "outou-rsx" || uri.as_str().ends_with(".rsx")
}

/// Handles a `.rsx` `didClose`: reloads the document from disk and resets
/// its version to `0` (issue #9 Gate 3 review, L10) — `Workspace::build_overlay`
/// keys off `version != 0` to decide which buffers count as "open" for
/// planning purposes, so a closed (and possibly externally reverted or
/// edited) file must stop being treated as an open editor buffer, rather
/// than keeping whatever text it had at the moment it was closed for the
/// rest of the session and driving every later re-plan with it. The
/// generated overlay this server keeps open on rust-analyzer itself is
/// untouched — it stays open for the life of the session regardless; only
/// a full re-plan ever closes one.
fn handle_rsx_close(state: &mut State, connection: &Connection, uri: &lsp_types::Uri) {
    let rsx_uri_string = uri::to_outou(uri).as_str().to_string();
    let Some(path) = uri::outou_uri_str_to_path(&rsx_uri_string) else {
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            forget_closed_deleted(state, connection, &rsx_uri_string);
            return;
        }
        Err(e) => {
            // Not readable now: at least stop treating the stale buffer as
            // an open editor document.
            eprintln!("outou-lsp: reading {}: {e}", path.display());
            reset_version(state, &rsx_uri_string);
            return;
        }
    };
    // Closing discards the buffer: when the disk text differs (an unsaved
    // edit), or a plan error stands that the buffer may have caused, the
    // file is re-planned and regenerated from disk like any edit, so
    // rust-analyzer and the plan error stop describing the discarded text.
    if close_needs_update(state, &rsx_uri_string, &text) {
        handle_rsx_change(state, connection, rsx_uri_string, text, 0);
        return;
    }
    match state.workspace.as_mut() {
        Some(workspace) => {
            workspace
                .rsx
                .insert(rsx_uri_string.clone(), documents::RsxDocument::new(text, 0));
            diagnostics::publish_for_rsx(connection, workspace, &rsx_uri_string);
        }
        None => {
            state
                .degraded_docs
                .insert(rsx_uri_string.clone(), documents::RsxDocument::new(text, 0));
            diagnostics::publish_degraded(connection, &state.degraded_docs, &rsx_uri_string);
        }
    }
}

fn close_needs_update(state: &State, rsx_uri: &str, disk_text: &str) -> bool {
    state.workspace.as_ref().is_some_and(|workspace| {
        workspace.plan_error.is_some()
            || workspace
                .rsx
                .get(rsx_uri)
                .is_some_and(|doc| doc.line_index.text() != disk_text)
    })
}

/// A document deleted from disk while open, now closed: the same handling
/// as a watched deletion, or, in degraded mode, just dropping it.
fn forget_closed_deleted(state: &mut State, connection: &Connection, rsx_uri: &str) {
    if state.workspace.is_some() {
        super::watched_files::handle_deleted_after_close(state, connection, rsx_uri);
    } else {
        state.degraded_docs.remove(rsx_uri);
        diagnostics::clear_for_rsx(connection, rsx_uri);
    }
}

fn reset_version(state: &mut State, rsx_uri: &str) {
    let doc = match state.workspace.as_mut() {
        Some(workspace) => workspace.rsx.get_mut(rsx_uri),
        None => state.degraded_docs.get_mut(rsx_uri),
    };
    if let Some(doc) = doc {
        doc.version = 0;
    }
}

/// Handles a `.rsx` file being saved: re-plans the crate from the
/// now-on-disk sources and writes every unit's generated Rust through the
/// exact same transactional Strict path `outou build` uses
/// (`outou_cli::build::emit::emit`), then forwards `didSave` to
/// rust-analyzer for the saved file's *generated* unit, so its own
/// `checkOnSave` flycheck — the only mechanism that surfaces semantic
/// (type-mismatch) errors, per the Week 1 spike's known blockers — runs
/// against a build that actually compiles.
///
/// Never writes Recovery-mode placeholder text (issue #9 Gate 3 review,
/// M1/CRITICAL-1, confirmed live: saving a half-typed `<UserCard us`
/// replaced `examples/phase0-app`'s real `[[bin]]` target,
/// `src/.generated/crate-root.rs`, with a placeholder call, and the
/// sibling `.rs.map.json` was never rewritten at all, leaving on-disk
/// code and map silently diverged). If Strict generation fails anywhere
/// in the plan, nothing is written, `didSave` is not forwarded to
/// rust-analyzer, and the in-memory workspace (already updated by the
/// preceding `didChange`, still in Recovery mode for the editor overlay)
/// is left exactly as it was — Outou's own syntax diagnostic, published
/// by the ordinary `didChange` path, already tells the user why nothing
/// was written *for the broken file itself*.
///
/// A save blocked crate-wide by some *other* broken `.rsx` file (issue #9
/// Gate 3 review, L14, fixed): before this fix, the user who just saved a
/// different, valid file got no editor-visible signal at all that their
/// save was blocked — only the broken file's own Outou diagnostic hinted
/// at it, and only if the user happened to be looking at that file.
/// [`notify_save_blocked_by_another_file`] below closes this with a
/// `window/showMessage` naming the blocking file.
fn handle_rsx_save(state: &mut State, connection: &Connection, rsx_uri: &lsp_types::Uri) {
    let Some(workspace) = &state.workspace else {
        return;
    };
    let rsx_uri_string = uri::to_outou(rsx_uri).as_str().to_string();

    let plan = match plan::resolve(&workspace.manifest_dir) {
        Ok(plan::Resolved::Planned { plan, .. }) => plan,
        Ok(plan::Resolved::Degraded { .. }) => {
            eprintln!(
                "outou-lsp: no `.rsx` crate root after saving {rsx_uri_string}; not writing generated Rust"
            );
            return;
        }
        Err(e) => {
            eprintln!("outou-lsp: re-planning on save failed: {e}; not writing generated Rust");
            return;
        }
    };

    if let Err(e) = outou_cli::build::emit::emit(&plan) {
        eprintln!(
            "outou-lsp: not writing generated Rust for {rsx_uri_string} (syntax errors prevent generation): {e}"
        );
        notify_save_blocked_by_another_file(connection, &rsx_uri_string, &e);
        return;
    }

    let Some(saved_unit) = plan
        .units
        .iter()
        .find(|unit| outou_sourcemap::file_uri(&unit.source_file).as_str() == rsx_uri_string)
    else {
        super::warn_unplanned_once(
            &mut state.unplanned_reported,
            &rsx_uri_string,
            "is not part of the planned crate; saving it does not run rust-analyzer's checks",
        );
        return;
    };
    let generated_uri = outou_sourcemap::file_uri(&saved_unit.generated_file);
    let Some(ra) = &state.ra else {
        return;
    };
    let text = workspace
        .generated
        .get(generated_uri.as_str())
        .map(|unit| unit.text.clone());
    ra.notify(
        "textDocument/didSave",
        json!({
            "textDocument": { "uri": generated_uri.as_str() },
            "text": text,
        }),
    );
}

/// Sends `window/showMessage` (Warning) naming the `.rsx` file whose
/// syntax error (`outou_cli::build::EmitError::SyntaxErrors`) blocked
/// [`handle_rsx_save`]'s write, unless it is the very file that was just
/// saved: that file's own Outou syntax diagnostic (published by the
/// ordinary `didChange` path) already tells the user why, right where
/// they are looking, so a second, editor-wide notice would only be noise
/// for the common case of saving a file that is itself broken. It is the
/// case this exists for — some *other* `.rsx` file being broken, with no
/// diagnostic visible unless the user happens to have that file open —
/// that had no editor-visible signal at all before this fix (issue #9
/// Gate 3 review, L14).
fn notify_save_blocked_by_another_file(
    connection: &Connection,
    saved_rsx_uri: &str,
    error: &outou_cli::build::EmitError,
) {
    let outou_cli::build::EmitError::SyntaxErrors { path, .. } = error else {
        // Every other `EmitError` variant (an unreadable file, an
        // unsupported construct, a write failure) is either not the
        // "some other file is broken" case this notice is for, or is
        // already an unusual enough failure that `handle_rsx_save`'s own
        // `eprintln!` is the right place for it, not a user-facing popup.
        return;
    };
    if outou_sourcemap::file_uri(path).as_str() == saved_rsx_uri {
        return;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let params = ShowMessageParams {
        typ: MessageType::WARNING,
        message: format!(
            "outou: this save was not written because `{name}` has a syntax error; fix it and save again"
        ),
    };
    let _ = connection
        .sender
        .send(Message::Notification(Notification::new(
            "window/showMessage".to_string(),
            params,
        )));
}

/// Regenerates one `.rsx` unit after an edit (or, in degraded mode,
/// reparses it stand-alone) and republishes its diagnostics. Shared by
/// `didOpen` and `didChange`: both are, from the compiler's point of
/// view, "here is this file's current text".
pub(super) fn handle_rsx_change(
    state: &mut State,
    connection: &Connection,
    rsx_uri_string: String,
    new_text: String,
    version: i32,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        state.degraded_docs.insert(
            rsx_uri_string.clone(),
            documents::RsxDocument::new(new_text, version),
        );
        diagnostics::publish_degraded(connection, &state.degraded_docs, &rsx_uri_string);
        return;
    };

    // While a plan error stands, every edit re-plans, wherever the error is
    // shown: the edit may be exactly what fixes it (including undoing the
    // edit that caused an error in *another* file), and the range of an
    // error on this file is only valid for the text it was computed from.
    // The cost is one crate-wide plan per edit for as long as the error
    // stands (L1); the plan is cheap next to rust-analyzer's own work.
    if workspace.module_shape_changed(&rsx_uri_string, &new_text) || workspace.plan_error.is_some()
    {
        replan_and_resync(state, connection, rsx_uri_string, new_text, version);
        return;
    }

    regenerate_and_forward(state, &rsx_uri_string, new_text, version, false);
    if let Some(workspace) = state.workspace.as_ref() {
        diagnostics::publish_for_rsx(connection, workspace, &rsx_uri_string);
    }
}

/// Regenerates one unit from `new_text` and tells rust-analyzer, when the
/// generated text changed. `keep_plan_shape` is for an edit that could not
/// be re-planned (`Workspace::plan_error`): the unit is regenerated against
/// the plan still in force and keeps that plan's declared modules, so a
/// later edit is still compared with the plan in force.
fn regenerate_and_forward(
    state: &mut State,
    rsx_uri_string: &str,
    new_text: String,
    version: i32,
    keep_plan_shape: bool,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    let result = if keep_plan_shape {
        workspace.regenerate_against_plan(rsx_uri_string, &new_text, version)
    } else {
        workspace.regenerate(rsx_uri_string, &new_text, version)
    };
    forward_regenerated(state, rsx_uri_string, new_text, version, result);
}

/// Installs the outcome of regenerating one unit: tells rust-analyzer when
/// the generated text changed, and in the two cases where no unit was
/// regenerated (not part of the plan, or generation failed) still replaces
/// the document so the editor's current text is what is served.
pub(super) fn forward_regenerated(
    state: &mut State,
    rsx_uri_string: &str,
    new_text: String,
    version: i32,
    result: Result<Option<crate::documents::Regenerated>, crate::documents::LoadError>,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    match result {
        Ok(Some(regenerated)) => {
            // Unchanged generated text (a `didOpen` of a file as it is on
            // disk): rust-analyzer already has it, and a redundant
            // `didChange` would only make it recompute.
            let ra = state.ra.as_ref().filter(|_| regenerated.generated_changed);
            let unit = workspace.generated.get_mut(&regenerated.generated_uri);
            if let (Some(ra), Some(unit)) = (ra, unit) {
                unit.ra_version += 1;
                ra.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": regenerated.generated_uri, "version": unit.ra_version },
                        "contentChanges": [{ "text": unit.text }],
                    }),
                );
            }
        }
        Ok(None) => {
            super::warn_unplanned_once(
                &mut state.unplanned_reported,
                rsx_uri_string,
                "is not part of the planned crate (no `mod` declaration reaches it); it is \
                 checked for syntax errors only and is not sent to rust-analyzer",
            );
            workspace.rsx.insert(
                rsx_uri_string.to_string(),
                documents::RsxDocument::new(new_text, version),
            );
        }
        Err(e) => {
            eprintln!("outou-lsp: regenerating {rsx_uri_string}: {e}");
            workspace.rsx.insert(
                rsx_uri_string.to_string(),
                documents::RsxDocument::new(new_text, version),
            );
        }
    }
}

/// Handles a module-declaration change (issue #9 architecture note: "if
/// changed, re-plan and regenerate all units"): re-resolves the crate,
/// resyncs rust-analyzer's overlays for every unit (new ones get
/// `didOpen`, previously known ones get a full-text `didChange`), and
/// republishes diagnostics for every known `.rsx` file.
pub(super) fn replan_and_resync(
    state: &mut State,
    connection: &Connection,
    rsx_uri_string: String,
    new_text: String,
    version: i32,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    // Captured before `replan` rebuilds `workspace.generated` from
    // scratch (every unit comes back with `ra_version: 0`, regardless of
    // what rust-analyzer was last told): S3 (issue #9 Gate 3 review)
    // needs each previously-open unit's *last-sent* version to keep
    // counting up rather than resetting to 1, which rust-analyzer is
    // free to treat as a stale/duplicate update after N edits.
    let previous_ra_versions = last_sent_ra_versions(workspace);
    let previous_plan_error_uri = workspace.plan_error_uri().map(str::to_string);

    if let Err(e) = workspace.replan(&rsx_uri_string, &new_text) {
        eprintln!("outou-lsp: re-planning after a module declaration change: {e}");
        // The crate-wide re-plan failed (issue #9 Gate 3 review, M2), but
        // the editor's own buffer still changed: keep serving *this*
        // document from the new text rather than leaving stale syntax
        // diagnostics up for content the user no longer has on screen, and
        // keep rust-analyzer's copy of it current. `Workspace::replan` is
        // transactional: the previous plan, units and registry are intact,
        // so the unit is regenerated against the plan in force
        // ([`regenerate_and_forward`]).
        regenerate_and_forward(state, &rsx_uri_string, new_text, version, true);
        let Some(workspace) = state.workspace.as_ref() else {
            return;
        };
        // The editor is told why the re-plan failed (`Workspace::plan_error`),
        // on the file that declares the module, alongside its other
        // diagnostics.
        diagnostics::publish_for_rsx(connection, workspace, &rsx_uri_string);
        diagnostics::publish_plan_error_change(
            connection,
            workspace,
            previous_plan_error_uri.as_deref(),
            &rsx_uri_string,
        );
        return;
    }
    // A successful re-plan invalidates every generated-file position any
    // in-flight rust-analyzer request was computed against (S3): bump the
    // epoch so `handle_ra_response` answers those `RequestCancelled`
    // instead of mapping a stale response against the new state.
    state.epoch += 1;
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    // M5 (issue #9 Gate 3 review): `Workspace::replan` restores every
    // document's version from its *pre-edit* snapshot (needed so an
    // untouched document's version does not appear to jump backwards),
    // which for the triggering document itself means the version the
    // editor just sent in this very `didChange` is lost. A stale version
    // here makes `vscode-languageclient` (and any client following the
    // same rule) discard the `publishDiagnostics` below outright, since
    // it no longer matches the document's current version.
    if let Some(doc) = workspace.rsx.get_mut(&rsx_uri_string) {
        doc.version = version;
    }
    resync_after_replan(
        state,
        connection,
        &previous_ra_versions,
        previous_plan_error_uri.as_deref(),
    );
}

/// Each generated unit's last-sent rust-analyzer document version, to be
/// captured before a re-plan (see [`replan_and_resync`]).
pub(super) fn last_sent_ra_versions(workspace: &documents::Workspace) -> HashMap<String, i32> {
    workspace
        .generated
        .iter()
        .map(|(uri, unit)| (uri.clone(), unit.ra_version))
        .collect()
}

/// The part of a successful re-plan that follows `Workspace::replan`:
/// resyncs rust-analyzer's overlays with `previous_ra_versions` (from
/// [`last_sent_ra_versions`], captured before the re-plan) and
/// republishes diagnostics for every known `.rsx` file. The re-plan
/// succeeded, so `previous_plan_error_uri` (the file that showed the last
/// failure, captured before it) no longer shows one.
pub(super) fn resync_after_replan(
    state: &mut State,
    connection: &Connection,
    previous_ra_versions: &HashMap<String, i32>,
    previous_plan_error_uri: Option<&str>,
) {
    let Some(workspace) = state.workspace.as_mut() else {
        return;
    };
    if let Some(ra) = &state.ra {
        let now_open: HashSet<String> = workspace.generated.keys().cloned().collect();
        for generated_uri in previous_ra_versions.keys() {
            if !now_open.contains(generated_uri) {
                // This unit no longer exists after the re-plan (a module
                // was removed, or renamed to a different generated path);
                // tell rust-analyzer its overlay is gone rather than
                // leaving a phantom open document (S3).
                ra.notify(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": generated_uri } }),
                );
            }
        }
        for generated_uri in &now_open {
            let Some(unit) = workspace.generated.get_mut(generated_uri) else {
                continue;
            };
            match previous_ra_versions.get(generated_uri) {
                Some(&previous_version) => {
                    unit.ra_version = previous_version + 1;
                    ra.notify(
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": generated_uri, "version": unit.ra_version },
                            "contentChanges": [{ "text": unit.text }],
                        }),
                    );
                }
                None => {
                    unit.ra_version = documents::INITIAL_RA_VERSION;
                    ra.notify(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": generated_uri,
                                "languageId": "rust",
                                "version": unit.ra_version,
                                "text": unit.text,
                            }
                        }),
                    );
                }
            }
        }
    }

    let rsx_uris: Vec<String> = workspace.rsx.keys().cloned().collect();
    for uri in rsx_uris {
        diagnostics::publish_for_rsx(connection, workspace, &uri);
    }
    // A file the plan error was shown on that is still not a known
    // document (the declaration was removed) was not republished above.
    if let Some(uri) = previous_plan_error_uri.filter(|uri| !workspace.rsx.contains_key(*uri)) {
        diagnostics::refresh_for_rsx(connection, workspace, uri);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use lsp_server::{Connection, Message};

    use super::*;
    use crate::documents::{LoadOutcome, Workspace};

    fn temp_crate_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "outou-lsp-test-notifications-{tag}-{}-{}",
            std::process::id(),
            line!()
        ))
    }

    fn load_state(manifest_dir: &Path) -> State {
        let outcome = Workspace::load(manifest_dir).expect("plans and generates");
        let LoadOutcome::Planned(workspace) = outcome else {
            panic!("the temp crate has a `.rsx` crate root");
        };
        let mut state = State::new();
        state.workspace = Some(*workspace);
        state
    }

    /// M5 (issue #9 Gate 3 review): after a successful re-plan, the
    /// triggering document's own version must be the one the editor just
    /// sent (`didChange`'s own `version`), not the pre-edit version
    /// `Workspace::replan` otherwise restores for every document —
    /// otherwise a conformant client discards the `publishDiagnostics`
    /// this function sends because its version no longer matches.
    #[test]
    fn replan_and_resync_publishes_with_the_triggering_edits_version() {
        let tmp = temp_crate_dir("m5");
        let src = tmp.join("src");
        fs::create_dir_all(&src).expect("creating src dir");
        fs::write(src.join("main.rsx"), "fn main() {}\n").expect("writing crate root");
        fs::write(src.join("newmod.rsx"), "pub fn f() {}\n").expect("writing the new module file");

        let mut state = load_state(&tmp);
        let main_rsx_uri = state
            .workspace
            .as_ref()
            .unwrap()
            .rsx
            .keys()
            .find(|uri| uri.ends_with("main.rsx"))
            .expect("main.rsx is a known unit")
            .clone();
        // Mark the document "open" at some earlier version, as `didOpen`
        // would have.
        state
            .workspace
            .as_mut()
            .unwrap()
            .rsx
            .get_mut(&main_rsx_uri)
            .unwrap()
            .version = 3;

        let (connection, client) = Connection::memory();
        let edited_text = "mod newmod;\nfn main() {}\n".to_string();
        replan_and_resync(
            &mut state,
            &connection,
            main_rsx_uri.clone(),
            edited_text,
            4,
        );

        assert_eq!(
            state.workspace.as_ref().unwrap().rsx[&main_rsx_uri].version,
            4,
            "the triggering document's version must be the edit's own version, not the pre-edit one"
        );

        let mut saw_matching_publish = false;
        while let Ok(msg) = client.receiver.try_recv() {
            if let Message::Notification(note) = msg {
                if note.method == "textDocument/publishDiagnostics" {
                    if let Ok(params) =
                        serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(note.params)
                    {
                        if params.uri.as_str().ends_with("main.rsx") {
                            assert_eq!(params.version, Some(4));
                            saw_matching_publish = true;
                        }
                    }
                }
            }
        }
        assert!(
            saw_matching_publish,
            "expected a publishDiagnostics for main.rsx with version 4"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    /// F17 (c), #31: rust-analyzer's check-on-save diagnostics arrive only
    /// on save. Opening the `.rsx` file afterwards (the editor sends the
    /// same text it already has on disk) used to regenerate the unit,
    /// clear the stored rust-analyzer diagnostics and publish only
    /// Outou's own set, which was empty until the next save.
    #[test]
    fn opening_a_file_with_unchanged_text_keeps_the_stored_rust_analyzer_diagnostics() {
        let tmp = temp_crate_dir("f17c");
        let src = tmp.join("src");
        fs::create_dir_all(&src).expect("creating src dir");
        fs::write(src.join("main.rsx"), "fn main() {}\n").expect("writing crate root");

        let mut state = load_state(&tmp);
        let (main_rsx_uri, generated_uri) = {
            let workspace = state.workspace.as_ref().unwrap();
            let rsx = workspace.rsx.keys().next().unwrap().clone();
            let generated = workspace.rsx_to_generated[&rsx].clone();
            (rsx, generated)
        };
        let (connection, client) = Connection::memory();
        let ra_version_before =
            state.workspace.as_ref().unwrap().generated[&generated_uri].ra_version;

        // rust-analyzer publishes for the generated file while the editor
        // has not opened the `.rsx` file at all.
        diagnostics::handle_ra_publish(
            &connection,
            state.workspace.as_mut().unwrap(),
            lsp_types::PublishDiagnosticsParams {
                uri: uri::to_lsp(&outou_sourcemap::Uri::new(&generated_uri)),
                diagnostics: vec![lsp_types::Diagnostic {
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: "a flycheck error".to_string(),
                    ..Default::default()
                }],
                version: None,
            },
        );
        let _ = client.receiver.try_iter().count();

        handle_rsx_change(
            &mut state,
            &connection,
            main_rsx_uri.clone(),
            "fn main() {}\n".to_string(),
            1,
        );

        let published = last_published_messages(&client, "main.rsx");
        assert_eq!(
            published,
            vec!["a flycheck error".to_string()],
            "the first publish on open must carry the stored rust-analyzer diagnostics"
        );
        assert_eq!(
            state.workspace.as_ref().unwrap().generated[&generated_uri].ra_version,
            ra_version_before,
            "an unchanged-text open must not advance the version sent to rust-analyzer"
        );

        // A real edit still drops them: they describe the old generated text.
        handle_rsx_change(
            &mut state,
            &connection,
            main_rsx_uri,
            "fn main() { let _x = 1; }\n".to_string(),
            2,
        );
        assert!(last_published_messages(&client, "main.rsx").is_empty());

        fs::remove_dir_all(&tmp).ok();
    }

    fn main_rsx_key(state: &State) -> String {
        state
            .workspace
            .as_ref()
            .unwrap()
            .rsx
            .keys()
            .find(|uri| uri.ends_with("main.rsx"))
            .expect("main.rsx is a known unit")
            .clone()
    }

    /// The last `publishDiagnostics` for a URI ending `suffix`, whole.
    fn last_published(client: &Connection, suffix: &str) -> Option<Vec<lsp_types::Diagnostic>> {
        let mut last = None;
        while let Ok(msg) = client.receiver.try_recv() {
            if let Message::Notification(note) = msg {
                if note.method != "textDocument/publishDiagnostics" {
                    continue;
                }
                let params: lsp_types::PublishDiagnosticsParams =
                    serde_json::from_value(note.params).unwrap();
                if params.uri.as_str().ends_with(suffix) {
                    last = Some(params.diagnostics);
                }
            }
        }
        last
    }

    /// #31: `mod x;` with no `x.rsx` used to reach only stderr. The edit
    /// that adds it must publish an Error on the `mod` item, next to the
    /// file's other diagnostics (here rust-analyzer's), and editing the
    /// declaration away must clear it.
    #[test]
    fn a_mod_without_a_file_is_published_on_its_declaration_and_clears_when_removed() {
        let tmp = temp_crate_dir("plan-error-edit");
        let src = tmp.join("src");
        fs::create_dir_all(&src).expect("creating src dir");
        fs::write(src.join("main.rsx"), "fn main() {}\n").expect("writing crate root");

        let mut state = load_state(&tmp);
        let main_rsx_uri = main_rsx_key(&state);
        let generated_uri =
            state.workspace.as_ref().unwrap().rsx_to_generated[&main_rsx_uri].clone();
        let (connection, client) = Connection::memory();

        handle_rsx_change(
            &mut state,
            &connection,
            main_rsx_uri.clone(),
            "fn main() {}\nmod missing;\n".to_string(),
            2,
        );
        // The failing edit regenerates the unit, which drops rust-analyzer's
        // diagnostics for the old text; this one is for the new text.
        diagnostics::handle_ra_publish(
            &connection,
            state.workspace.as_mut().unwrap(),
            lsp_types::PublishDiagnosticsParams {
                uri: uri::to_lsp(&outou_sourcemap::Uri::new(&generated_uri)),
                diagnostics: vec![lsp_types::Diagnostic {
                    severity: Some(lsp_types::DiagnosticSeverity::ERROR),
                    message: "a flycheck error".to_string(),
                    ..Default::default()
                }],
                version: None,
            },
        );
        let shown = last_published(&client, "main.rsx").expect("main.rsx is republished");
        let plan_error = shown
            .iter()
            .find(|d| d.message.contains("missing"))
            .expect("the unresolved module is published");
        assert_eq!(
            plan_error.severity,
            Some(lsp_types::DiagnosticSeverity::ERROR)
        );
        assert_eq!(plan_error.source.as_deref(), Some("outou"));
        assert_eq!(plan_error.range.start, lsp_types::Position::new(1, 0));
        assert!(
            !plan_error.message.contains("generated"),
            "{}",
            plan_error.message
        );
        assert!(
            shown.iter().any(|d| d.message == "a flycheck error"),
            "the plan error must not replace the other sources: {shown:?}"
        );

        handle_rsx_change(
            &mut state,
            &connection,
            main_rsx_uri,
            "fn main() {}\n".to_string(),
            3,
        );
        let shown = last_published(&client, "main.rsx").expect("main.rsx is republished");
        assert!(
            !shown.iter().any(|d| d.message.contains("missing")),
            "removing the declaration clears the plan error: {shown:?}"
        );
        assert!(state.workspace.as_ref().unwrap().plan_error.is_none());

        fs::remove_dir_all(&tmp).ok();
    }

    /// The error follows the file that declares the module, not the one
    /// being edited, and is cleared there once the module file exists.
    #[test]
    fn a_plan_error_is_shown_on_the_declaring_file_and_clears_when_the_module_appears() {
        let tmp = temp_crate_dir("plan-error-declaring");
        let src = tmp.join("src");
        fs::create_dir_all(&src).expect("creating src dir");
        fs::write(src.join("main.rsx"), "mod a;\nfn main() {}\n").expect("writing root");
        fs::write(src.join("a.rsx"), "pub fn a() {}\n").expect("writing a");

        let mut state = load_state(&tmp);
        let a_rsx_uri = state
            .workspace
            .as_ref()
            .unwrap()
            .rsx
            .keys()
            .find(|uri| uri.ends_with("a.rsx"))
            .expect("a.rsx is a known unit")
            .clone();
        let (connection, client) = Connection::memory();

        handle_rsx_change(
            &mut state,
            &connection,
            a_rsx_uri.clone(),
            "mod b;\npub fn a() {}\n".to_string(),
            2,
        );
        let shown = last_published(&client, "a.rsx").expect("a.rsx is republished");
        assert!(shown.iter().any(|d| d.message.contains("`b`")), "{shown:?}");
        assert_eq!(
            state.workspace.as_ref().unwrap().plan_error_uri(),
            Some(a_rsx_uri.as_str())
        );

        fs::create_dir_all(src.join("a")).expect("creating a/");
        fs::write(src.join("a/b.rsx"), "pub fn b() {}\n").expect("writing b");
        let workspace = state.workspace.as_mut().unwrap();
        let previous = workspace.plan_error_uri().map(str::to_string);
        workspace.replan_from_disk().expect("b.rsx now resolves");
        resync_after_replan(
            &mut state,
            &connection,
            &HashMap::new(),
            previous.as_deref(),
        );
        let shown = last_published(&client, "a.rsx").expect("a.rsx is republished");
        assert!(shown.is_empty(), "{shown:?}");

        fs::remove_dir_all(&tmp).ok();
    }

    /// Messages of the last `publishDiagnostics` for a URI ending `suffix`.
    fn last_published_messages(client: &Connection, suffix: &str) -> Vec<String> {
        let mut last = None;
        while let Ok(msg) = client.receiver.try_recv() {
            if let Message::Notification(note) = msg {
                if note.method != "textDocument/publishDiagnostics" {
                    continue;
                }
                let params: lsp_types::PublishDiagnosticsParams =
                    serde_json::from_value(note.params).unwrap();
                if params.uri.as_str().ends_with(suffix) {
                    last = Some(params.diagnostics);
                }
            }
        }
        last.expect("a publishDiagnostics was sent")
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    /// L10: closing a `.rsx` document must reload it from disk and reset
    /// its version to `0`, so `Workspace::build_overlay` stops treating a
    /// closed (and possibly stale) in-memory buffer as an open editor
    /// document for future planning.
    #[test]
    fn handle_rsx_close_reloads_from_disk_and_resets_the_version() {
        let tmp = temp_crate_dir("l10");
        let src = tmp.join("src");
        fs::create_dir_all(&src).expect("creating src dir");
        fs::write(src.join("main.rsx"), "fn main() {}\n").expect("writing crate root");

        let mut state = load_state(&tmp);
        let main_rsx_uri = state
            .workspace
            .as_ref()
            .unwrap()
            .rsx
            .keys()
            .find(|uri| uri.ends_with("main.rsx"))
            .expect("main.rsx is a known unit")
            .clone();
        // Simulate an open buffer whose in-memory text has diverged from
        // disk (an edit never saved).
        {
            let workspace = state.workspace.as_mut().unwrap();
            workspace.rsx.insert(
                main_rsx_uri.clone(),
                documents::RsxDocument::new("fn main() { /* unsaved edit */ }\n".to_string(), 5),
            );
        }

        // Built from the workspace's own (canonicalized) key rather than
        // re-deriving a URI from `src.join("main.rsx")` directly: on
        // macOS `/tmp` is itself a symlink, so an uncanonicalized path
        // would not round-trip to the same URI `Workspace::load` used as
        // this document's key.
        let lsp_uri = crate::uri::to_lsp(&outou_sourcemap::Uri::new(main_rsx_uri.clone()));
        let (connection, _client) = Connection::memory();
        handle_rsx_close(&mut state, &connection, &lsp_uri);

        let doc = &state.workspace.as_ref().unwrap().rsx[&main_rsx_uri];
        assert_eq!(
            doc.version, 0,
            "a closed document's version must reset to 0"
        );
        assert_eq!(
            doc.line_index.text(),
            "fn main() {}\n",
            "a closed document must be reloaded from disk, not keep its unsaved buffer"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    /// L14: a save blocked by *another* file's syntax error must produce a
    /// `window/showMessage` (Warning) naming that file.
    #[test]
    fn notify_save_blocked_by_another_file_warns_with_the_broken_files_name() {
        let (connection, client) = Connection::memory();
        let error = outou_cli::build::EmitError::SyntaxErrors {
            path: PathBuf::from("/app/src/components.rsx"),
            rendered: "unexpected '}'".to_string(),
        };

        notify_save_blocked_by_another_file(&connection, "file:///app/src/main.rsx", &error);

        let msg = client
            .receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("a window/showMessage notification was sent");
        match msg {
            lsp_server::Message::Notification(note) => {
                assert_eq!(note.method, "window/showMessage");
                assert_eq!(note.params["type"], 2, "MessageType::WARNING is 2");
                let message = note.params["message"].as_str().unwrap();
                assert!(
                    message.contains("components.rsx"),
                    "message must name the broken file: {message}"
                );
            }
            other => panic!("expected a notification, got {other:?}"),
        }
    }

    /// L14: saving the very file that is itself broken must not also pop
    /// up a notice — its own Outou syntax diagnostic already says so,
    /// right where the user is looking.
    #[test]
    fn notify_save_blocked_by_another_file_is_silent_for_the_saved_file_itself() {
        let (connection, client) = Connection::memory();
        let error = outou_cli::build::EmitError::SyntaxErrors {
            path: PathBuf::from("/app/src/main.rsx"),
            rendered: "unexpected '}'".to_string(),
        };

        notify_save_blocked_by_another_file(
            &connection,
            outou_sourcemap::file_uri(Path::new("/app/src/main.rsx")).as_str(),
            &error,
        );

        assert!(
            client.receiver.try_recv().is_err(),
            "no notice should be sent when the broken file is the one just saved"
        );
    }

    /// A non-`SyntaxErrors` `EmitError` (an unreadable file, say) is not
    /// this notice's case; it must not send anything.
    #[test]
    fn notify_save_blocked_by_another_file_ignores_other_emit_error_variants() {
        let (connection, client) = Connection::memory();
        let error = outou_cli::build::EmitError::ReadSource {
            path: PathBuf::from("/app/src/components.rsx"),
            source: std::io::Error::other("boom"),
        };

        notify_save_blocked_by_another_file(&connection, "file:///app/src/main.rsx", &error);

        assert!(client.receiver.try_recv().is_err());
    }
}
