//! The main loop: waits for `initialize`, resolves the crate root and
//! plans it (`crate::documents::Workspace::load`), sets up rust-analyzer
//! (`crate::ra`), and then selects between the editor connection and
//! rust-analyzer's own message stream for the life of the session.
//! Per-message handling lives in `crate::dispatch`.
//!
//! Running two rust-analyzer instances on one project — the editor's own,
//! for `.rs` files, and this one, spawned per `.rsx` workspace — is the
//! documented Phase 0 arrangement (issue #9 architecture note; the Week 1
//! spike ran the same way).

use std::process::ExitCode;

use lsp_server::{Connection, Message, Response};
use lsp_types::{
    CompletionOptions, HoverProviderCapability, InitializeParams, OneOf, SaveOptions,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
};
use serde_json::{json, Value};

use crate::dispatch::{
    dispatch_client_notification, dispatch_client_request, dispatch_ra_message, State,
};
use crate::documents::{LoadOutcome, Workspace};
use crate::ra::RaClient;
use crate::uri;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the language server: waits for `initialize`, sets up rust-analyzer
/// (unless the crate is in degraded mode), and dispatches everything
/// after that until `shutdown`/`exit` or the connection closes.
pub fn run(ra_binary: String) -> ExitCode {
    let (connection, io_threads) = Connection::stdio();
    let code = match main_loop(&connection, &ra_binary) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("outou-lsp: {e}");
            ExitCode::FAILURE
        }
    };
    drop(connection);
    let _ = io_threads.join();
    code
}

#[derive(Debug, thiserror::Error)]
enum ServerError {
    #[error(transparent)]
    Protocol(#[from] lsp_server::ProtocolError),
    #[error("parsing `initialize` params: {0}")]
    InitializeParams(serde_json::Error),
}

fn main_loop(connection: &Connection, ra_binary: &str) -> Result<(), ServerError> {
    let (initialize_id, params_value) = connection.initialize_start()?;
    let params: InitializeParams =
        serde_json::from_value(params_value).map_err(ServerError::InitializeParams)?;

    let root = resolve_root(&params);
    let mut state = State::new();
    load_workspace(&mut state, &root, ra_binary, &params);

    connection.initialize_finish(
        initialize_id,
        json!({
            "capabilities": build_server_capabilities(),
            "serverInfo": { "name": "outou-lsp", "version": VERSION },
        }),
    )?;

    loop {
        let ra_receiver = state
            .ra
            .as_ref()
            .map(|ra| ra.receiver.clone())
            .unwrap_or_else(crossbeam_channel::never);
        crossbeam_channel::select! {
            recv(connection.receiver) -> msg => match msg {
                Ok(Message::Request(req)) => {
                    if connection.handle_shutdown(&req)? {
                        if let Some(mut ra) = state.ra.take() {
                            ra.shutdown();
                        }
                        return Ok(());
                    }
                    dispatch_client_request(&mut state, connection, req);
                }
                Ok(Message::Notification(note)) => dispatch_client_notification(&mut state, connection, note),
                Ok(Message::Response(_)) => {}
                Err(_) => return Ok(()),
            },
            recv(ra_receiver) -> msg => match msg {
                Ok(message) => dispatch_ra_message(&mut state, connection, message),
                Err(_) => {
                    eprintln!("outou-lsp: rust-analyzer exited; continuing with Outou syntax diagnostics only");
                    state.ra = None;
                }
            },
        }
    }
}

/// Resolves the crate at `root` (if any) and, unless it is degraded,
/// generates every unit and spawns/initializes rust-analyzer, filling in
/// `state.workspace`/`state.ra`. Every failure degrades gracefully rather
/// than aborting: a missing crate root, a planning error, or rust-analyzer
/// failing to spawn all leave the server running with Outou syntax
/// diagnostics only, per the architecture note ("if `src/main.rsx`/
/// `lib.rsx` is absent, run in a degraded mode").
fn load_workspace(
    state: &mut State,
    root: &Option<std::path::PathBuf>,
    ra_binary: &str,
    params: &InitializeParams,
) {
    let Some(root) = root else {
        eprintln!("outou-lsp: no workspace root given; running in degraded mode");
        return;
    };
    match Workspace::load(root) {
        Ok(LoadOutcome::Planned(boxed_workspace)) => {
            let workspace = *boxed_workspace;
            if let Err(e) = write_missing_generated_files(&workspace) {
                eprintln!("outou-lsp: writing initial generated files: {e}");
            }
            match RaClient::spawn(ra_binary, &workspace.manifest_dir) {
                Ok(mut ra) => {
                    setup_rust_analyzer(&mut ra, &workspace, params);
                    state.ra = Some(ra);
                }
                Err(e) => {
                    eprintln!("outou-lsp: {e}; continuing with Outou syntax diagnostics only");
                }
            }
            state.workspace = Some(workspace);
        }
        Ok(LoadOutcome::Degraded { manifest_dir }) => {
            eprintln!(
                "outou-lsp: no `.rsx` crate root under {}; running in degraded mode (Outou syntax diagnostics only)",
                manifest_dir.display()
            );
        }
        Err(e) => {
            eprintln!("outou-lsp: planning the crate failed: {e}; running in degraded mode");
        }
    }
}

/// Resolves the workspace root from `initialize`'s `workspaceFolders`.
/// `rootUri`/`rootPath` are both deprecated in favor of it (LSP 3.6+) and
/// are not read here.
fn resolve_root(params: &InitializeParams) -> Option<std::path::PathBuf> {
    params
        .workspace_folders
        .as_ref()
        .and_then(|folders| folders.first())
        .and_then(|folder| uri::to_path(&folder.uri))
}

/// ADR 0009 layout (b) needs the generated `[[bin]]`/`[lib]` target to
/// exist on disk for Cargo's own crate graph, even before the user ever
/// runs `outou build`: if a unit's generated file is missing, this writes
/// its (Recovery-mode) text once at startup. An existing file — possibly
/// stale, definitely not necessarily in sync with the buffer
/// rust-analyzer is about to be told to use as an overlay — is left
/// alone.
fn write_missing_generated_files(workspace: &Workspace) -> std::io::Result<()> {
    for unit in workspace.generated.values() {
        let path = &unit.planned.generated_file;
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &unit.text)?;
    }
    Ok(())
}

/// `save` is requested (with `includeText`) so that
/// [`crate::dispatch::dispatch_client_notification`]'s `didSave` handling
/// can write the freshest generated text to disk and forward the save to
/// rust-analyzer's own `checkOnSave`/flycheck: per the Week 1 spike
/// (`docs/ra-spike-results.md`), rust-analyzer's *native* diagnostics
/// never report semantic (type-mismatch) errors, only flycheck does, and
/// flycheck reads the file from disk — the in-memory overlay this server
/// otherwise keeps rust-analyzer on is not enough for that one criterion.
fn build_server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(
                    SaveOptions {
                        include_text: Some(true),
                    }
                    .into(),
                ),
                ..Default::default()
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some([".", "<", ":"].into_iter().map(str::to_string).collect()),
            ..Default::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

/// Sends rust-analyzer its own `initialize`/`initialized` handshake
/// (forwarding the editor's capabilities and `initializationOptions`,
/// plus `checkOnSave`/`cargo.buildScripts.enable`, per the architecture
/// note) and `didOpen`s every generated unit's current (Recovery-mode)
/// text as its overlay.
fn setup_rust_analyzer(ra: &mut RaClient, workspace: &Workspace, params: &InitializeParams) {
    let mut capabilities = serde_json::to_value(&params.capabilities).unwrap_or(json!({}));
    strip_link_support(&mut capabilities);
    let init_options = build_ra_init_options(params.initialization_options.clone());

    let id = ra.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": uri::path_to_lsp(&workspace.manifest_dir),
            "capabilities": capabilities,
            "initializationOptions": init_options,
        }),
    );
    let response = ra.wait_for_response(&id, |other| {
        // rust-analyzer sending a request of its own before it has even
        // answered `initialize` is unlikely, but acknowledge anything
        // that shows up rather than deadlock waiting for a response that
        // depends on us replying first.
        if let Message::Request(req) = other {
            ra.send(Message::Response(Response::new_ok(req.id, Value::Null)));
        }
    });
    if response.is_none() {
        eprintln!("outou-lsp: rust-analyzer exited during initialize");
        return;
    }
    ra.notify("initialized", json!({}));

    for unit in workspace.generated.values() {
        ra.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": unit.generated_uri.as_str(),
                    "languageId": "rust",
                    "version": 1,
                    "text": unit.text,
                }
            }),
        );
    }
}

/// Removes `linkSupport` from the definition/typeDefinition/declaration/
/// implementation capabilities before forwarding them to rust-analyzer,
/// so its `textDocument/definition` responses are always plain
/// `Location`/`Location[]`, never `LocationLink[]` — this server's
/// response mapping (`crate::response::map_definition_response`) only
/// has to handle the one shape.
fn strip_link_support(capabilities: &mut Value) {
    let Some(text_document) = capabilities.get_mut("textDocument") else {
        return;
    };
    for key in [
        "definition",
        "typeDefinition",
        "declaration",
        "implementation",
    ] {
        if let Some(section) = text_document.get_mut(key).and_then(Value::as_object_mut) {
            section.remove("linkSupport");
        }
    }
}

fn build_ra_init_options(client_options: Option<Value>) -> Value {
    let mut options = match client_options {
        Some(Value::Object(map)) => Value::Object(map),
        _ => json!({}),
    };
    let object = options
        .as_object_mut()
        .expect("just constructed as an object");
    object.insert("checkOnSave".to_string(), json!(true));
    let cargo = object.entry("cargo").or_insert_with(|| json!({}));
    if !cargo.is_object() {
        *cargo = json!({});
    }
    let cargo_object = cargo.as_object_mut().expect("just ensured it is an object");
    let build_scripts = cargo_object
        .entry("buildScripts")
        .or_insert_with(|| json!({}));
    if !build_scripts.is_object() {
        *build_scripts = json!({});
    }
    build_scripts
        .as_object_mut()
        .expect("just ensured it is an object")
        .insert("enable".to_string(), json!(true));
    options
}
