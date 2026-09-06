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
    dispatch_client_notification, dispatch_client_request, dispatch_client_response,
    dispatch_ra_message, State,
};
use crate::documents::{LoadOutcome, Workspace};
use crate::ra::RaClient;
use crate::uri;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the language server: waits for `initialize`, sets up rust-analyzer
/// (unless the crate is in degraded mode, or has no `.rsx` root open at
/// all), and dispatches everything after that until `shutdown`/`exit` or
/// the connection closes.
///
/// `resolve_ra_binary` is called at most once, and only if
/// [`load_workspace`] finds a real `.rsx` crate root that needs
/// rust-analyzer (issue #9 Gate 3 review, S4): a machine with no
/// `rust-analyzer` on `PATH` must still be able to run `outou-lsp`
/// syntax-only against a crate with no `.rsx` root (or none open yet),
/// rather than failing before `initialize` even starts.
pub fn run(resolve_ra_binary: impl Fn() -> Result<String, String>) -> ExitCode {
    let (connection, io_threads) = Connection::stdio();
    let code = match main_loop(&connection, &resolve_ra_binary) {
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

fn main_loop(
    connection: &Connection,
    resolve_ra_binary: &impl Fn() -> Result<String, String>,
) -> Result<(), ServerError> {
    let (initialize_id, params_value) = connection.initialize_start()?;
    let params: InitializeParams =
        serde_json::from_value(params_value.clone()).map_err(ServerError::InitializeParams)?;

    let root = resolve_root(&params, &params_value);
    let mut state = State::new();
    apply_client_capabilities(&mut state, &params);
    load_workspace(&mut state, &root, resolve_ra_binary, &params);

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
                Ok(Message::Response(response)) => dispatch_client_response(&mut state, response),
                Err(_) => return Ok(()),
            },
            recv(ra_receiver) -> msg => match msg {
                Ok(message) => dispatch_ra_message(&mut state, connection, message),
                Err(_) => {
                    eprintln!("outou-lsp: rust-analyzer exited; continuing with Outou syntax diagnostics only");
                    state.ra = None;
                    // S5 (issue #9 Gate 3 review, MEDIUM-11): every
                    // request still waiting on a response from the
                    // rust-analyzer that just exited would otherwise hang
                    // the editor forever.
                    crate::dispatch::fail_all_pending(&mut state, connection);
                }
            },
        }
    }
}

/// Records the three capability flags [`crate::dispatch::State`] needs to
/// decide whether a rust-analyzer -> client request
/// (`crate::dispatch::handle_ra_request`) can be forwarded to the editor
/// at all, rather than always answered locally (issue #9 Gate 3 review,
/// S2/S7).
fn apply_client_capabilities(state: &mut State, params: &InitializeParams) {
    let capabilities = serde_json::to_value(&params.capabilities).unwrap_or(json!({}));
    state.client_supports_configuration = capabilities
        .pointer("/workspace/configuration")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    state.client_supports_work_done_progress = capabilities
        .pointer("/window/workDoneProgress")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    state.client_supports_dynamic_registration = any_dynamic_registration(&capabilities);
}

/// Whether `value` contains a `"dynamicRegistration": true` anywhere,
/// searched recursively: `client/registerCapability` is only meaningful
/// to forward when at least one specific capability opted into dynamic
/// registration (LSP has no single umbrella flag for "this client
/// supports `client/registerCapability`" the way it does for
/// `workspace.configuration`/`window.workDoneProgress`).
fn any_dynamic_registration(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            map.get("dynamicRegistration") == Some(&Value::Bool(true))
                || map.values().any(any_dynamic_registration)
        }
        Value::Array(items) => items.iter().any(any_dynamic_registration),
        _ => false,
    }
}

/// Resolves the crate at `root` (if any) and, unless it is degraded,
/// generates every unit and spawns/initializes rust-analyzer, filling in
/// `state.workspace`/`state.ra`. Every failure degrades gracefully rather
/// than aborting: a missing crate root, a planning error, rust-analyzer
/// not being found at all, or rust-analyzer failing to spawn, all leave
/// the server running with Outou syntax diagnostics only, per the
/// architecture note ("if `src/main.rsx`/`lib.rsx` is absent, run in a
/// degraded mode").
///
/// `resolve_ra_binary` (`main::resolve_rust_analyzer`) is called only in
/// the branch that actually found a `.rsx` crate root (issue #9 Gate 3
/// review, S4): a crate with no `.rsx` root — or no workspace root at all
/// — never needs rust-analyzer, so it must never be the reason `outou-lsp`
/// cannot start syntax-only.
fn load_workspace<F: Fn() -> Result<String, String>>(
    state: &mut State,
    root: &Option<std::path::PathBuf>,
    resolve_ra_binary: &F,
    params: &InitializeParams,
) {
    let Some(root) = root else {
        eprintln!("outou-lsp: no workspace root given; running in degraded mode");
        return;
    };
    match Workspace::load(root) {
        Ok(LoadOutcome::Planned(boxed_workspace)) => {
            let workspace = *boxed_workspace;
            write_missing_generated_files(&workspace);
            match resolve_ra_binary() {
                Ok(ra_binary) => match RaClient::spawn(&ra_binary, &workspace.manifest_dir) {
                    Ok(mut ra) => {
                        if setup_rust_analyzer(&mut ra, &workspace, params) {
                            state.ra = Some(ra);
                        } else {
                            eprintln!(
                                "outou-lsp: rust-analyzer setup did not complete; continuing with Outou syntax diagnostics only"
                            );
                            ra.shutdown();
                        }
                    }
                    Err(e) => {
                        eprintln!("outou-lsp: {e}; continuing with Outou syntax diagnostics only");
                    }
                },
                Err(message) => {
                    eprintln!(
                        "outou-lsp: {message}; continuing with Outou syntax diagnostics only"
                    );
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

/// Resolves the workspace root from `initialize`'s `workspaceFolders`
/// first, then `rootUri`, then `rootPath` (issue #9 Gate 3 review, S4):
/// the latter two are deprecated in favor of `workspaceFolders` (LSP
/// 3.6+), but a client that only ever sends one of them is still legal
/// per the LSP spec, and must not be treated the same as "no root at
/// all" (which would run this server in degraded mode for a perfectly
/// valid client).
///
/// `raw` is `initialize`'s params as raw JSON, used only for `rootUri`/
/// `rootPath`: both fields are `#[deprecated]` on [`InitializeParams`]
/// itself, and reading them through the typed field would turn into a
/// hard `-D warnings` failure (`AGENTS.md` forbids silencing that with
/// `#[allow(deprecated)]`) the moment this server is compiled with a
/// lint level that catches it. `workspaceFolders` is not deprecated, so
/// it is still read from the typed `params`.
fn resolve_root(params: &InitializeParams, raw: &Value) -> Option<std::path::PathBuf> {
    if let Some(path) = params
        .workspace_folders
        .as_ref()
        .and_then(|folders| folders.first())
        .and_then(|folder| uri::to_path(&folder.uri))
    {
        return Some(path);
    }
    if let Some(path) = raw
        .pointer("/rootUri")
        .and_then(Value::as_str)
        .and_then(uri::outou_uri_str_to_path)
    {
        return Some(path);
    }
    // `rootPath` (deprecated before `rootUri` even existed) is a plain
    // filesystem path, not a URI.
    raw.pointer("/rootPath")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
}

/// ADR 0009 layout (b) needs the generated `[[bin]]`/`[lib]` target to
/// exist on disk for Cargo's own crate graph, even before the user ever
/// runs `outou build`: if any unit's generated file is missing, this
/// writes every unit once, through the exact same transactional Strict
/// path `outou build` itself uses (`outou_cli::build::emit::emit`) —
/// never Recovery-mode placeholder text (issue #9 Gate 3 review,
/// M1/CRITICAL-1): Recovery mode exists so the *editor overlay* stays
/// analyzable while a file is half-typed, not so half-typed text can
/// become the crate's real `[[bin]]` target on disk. If Strict fails (the
/// crate has a real syntax error before `outou build` has ever run), this
/// writes nothing and logs — rust-analyzer simply sees a crate whose bin
/// target is missing, the same state as "`outou build` was never run".
/// An already-complete set of generated files (however stale) is left
/// alone, matching the previous behavior for that case.
fn write_missing_generated_files(workspace: &Workspace) {
    let Some(plan) = &workspace.plan else {
        return;
    };
    let any_missing = plan.units.iter().any(|unit| !unit.generated_file.exists());
    if !any_missing {
        return;
    }
    if let Err(e) = outou_cli::build::emit::emit(plan) {
        eprintln!(
            "outou-lsp: not writing initial generated files (syntax errors prevent generation): {e}"
        );
    }
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
        // Pinned rather than negotiated (M7, issue #9 Gate 3 review,
        // HIGH-6): every position this server computes itself
        // (`crate::mapping`) counts UTF-16 code units, matching
        // `outou_sourcemap::LineIndex`'s own contract, so advertising
        // anything else here would make this server's own math wrong
        // regardless of what rust-analyzer does.
        position_encoding: Some(lsp_types::PositionEncodingKind::UTF16),
        ..Default::default()
    }
}

/// Sends rust-analyzer its own `initialize`/`initialized` handshake
/// (forwarding the editor's capabilities and `initializationOptions`,
/// plus `checkOnSave`/`cargo.buildScripts.enable`, per the architecture
/// note) and `didOpen`s every generated unit's current (Recovery-mode)
/// text as its overlay.
fn setup_rust_analyzer(
    ra: &mut RaClient,
    workspace: &Workspace,
    params: &InitializeParams,
) -> bool {
    let mut capabilities = serde_json::to_value(&params.capabilities).unwrap_or(json!({}));
    strip_link_support(&mut capabilities);
    force_utf16_position_encoding(&mut capabilities);
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
    let Some(response) = response else {
        eprintln!("outou-lsp: rust-analyzer exited during initialize");
        return false;
    };
    let result = match response.response_result {
        Ok(result) => result,
        Err(e) => {
            eprintln!(
                "outou-lsp: rust-analyzer's initialize failed ({}): {}; continuing with Outou syntax diagnostics only",
                e.code, e.message
            );
            return false;
        }
    };
    // M7 (issue #9 Gate 3 review, HIGH-6): this server forced
    // `general.positionEncodings: ["utf-16"]` above, so rust-analyzer
    // should always agree — but a client capability rewrite that a
    // future rust-analyzer version stops honoring must not silently
    // mis-map every position rather than fail loudly. Confirmed live:
    // driving the same rust-analyzer with `["utf-8", "utf-16"]`
    // negotiated `utf-8` and produced confidently *wrong* (not null)
    // hover/definition answers for any line containing non-ASCII text.
    if !ra_uses_utf16_positions(&result) {
        eprintln!(
            "outou-lsp: rust-analyzer negotiated position encoding {:?} instead of utf-16; refusing to use it to avoid mis-mapped positions",
            result.pointer("/capabilities/positionEncoding")
        );
        return false;
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
    true
}

/// Forces `general.positionEncodings` to `["utf-16"]` in the capabilities
/// object sent to rust-analyzer, overwriting whatever the editor
/// advertised (M7, issue #9 Gate 3 review, HIGH-6): this server's own
/// position math (`crate::mapping`, `outou_sourcemap::LineIndex`) is
/// always UTF-16, so rust-analyzer negotiating anything else would make
/// every mapped position wrong in a way that looks like a plausible
/// answer rather than an obvious failure — see
/// [`ra_uses_utf16_positions`], which then confirms rust-analyzer agreed.
fn force_utf16_position_encoding(capabilities: &mut Value) {
    if !capabilities.is_object() {
        *capabilities = json!({});
    }
    let object = capabilities
        .as_object_mut()
        .expect("just ensured it is an object");
    let general = object.entry("general").or_insert_with(|| json!({}));
    if !general.is_object() {
        *general = json!({});
    }
    general
        .as_object_mut()
        .expect("just ensured it is an object")
        .insert("positionEncodings".to_string(), json!(["utf-16"]));
}

/// Whether rust-analyzer's `initialize` result confirms it will use
/// UTF-16 positions: either it did not advertise `positionEncoding` at
/// all (the LSP 3.17 default, per spec, is `utf-16`), or it advertised
/// exactly `"utf-16"`.
fn ra_uses_utf16_positions(initialize_result: &Value) -> bool {
    match initialize_result
        .pointer("/capabilities/positionEncoding")
        .and_then(Value::as_str)
    {
        None => true,
        Some(encoding) => encoding == "utf-16",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_utf16_position_encoding_overwrites_a_client_provided_list() {
        let mut capabilities = json!({ "general": { "positionEncodings": ["utf-8", "utf-16"] } });
        force_utf16_position_encoding(&mut capabilities);
        assert_eq!(
            capabilities["general"]["positionEncodings"],
            json!(["utf-16"])
        );
    }

    #[test]
    fn force_utf16_position_encoding_creates_missing_sections() {
        let mut capabilities = json!({});
        force_utf16_position_encoding(&mut capabilities);
        assert_eq!(
            capabilities["general"]["positionEncodings"],
            json!(["utf-16"])
        );
    }

    #[test]
    fn ra_uses_utf16_positions_accepts_an_absent_field_per_the_lsp_default() {
        assert!(ra_uses_utf16_positions(&json!({ "capabilities": {} })));
    }

    #[test]
    fn ra_uses_utf16_positions_accepts_an_explicit_utf16() {
        assert!(ra_uses_utf16_positions(
            &json!({ "capabilities": { "positionEncoding": "utf-16" } })
        ));
    }

    /// M7's decisive case: rust-analyzer 1.98.1, driven directly with
    /// `general.positionEncodings: ["utf-8", "utf-16"]`, negotiated
    /// `utf-8` — this must be refused, not silently accepted.
    #[test]
    fn ra_uses_utf16_positions_rejects_utf8() {
        assert!(!ra_uses_utf16_positions(
            &json!({ "capabilities": { "positionEncoding": "utf-8" } })
        ));
    }

    #[test]
    fn any_dynamic_registration_finds_a_nested_true() {
        let capabilities = json!({
            "textDocument": { "definition": { "dynamicRegistration": true } }
        });
        assert!(any_dynamic_registration(&capabilities));
    }

    #[test]
    fn any_dynamic_registration_is_false_with_none_set() {
        let capabilities = json!({
            "textDocument": { "definition": { "dynamicRegistration": false } },
            "workspace": {},
        });
        assert!(!any_dynamic_registration(&capabilities));
    }

    /// S4: `workspaceFolders[0]` wins even when `rootUri`/`rootPath` are
    /// also present.
    #[test]
    fn resolve_root_prefers_workspace_folders_over_root_uri_and_root_path() {
        let params: InitializeParams = serde_json::from_value(json!({
            "capabilities": {},
            "workspaceFolders": [{ "uri": "file:///a", "name": "a" }],
        }))
        .unwrap();
        let raw = json!({ "rootUri": "file:///b", "rootPath": "/c" });
        assert_eq!(
            resolve_root(&params, &raw),
            Some(std::path::PathBuf::from("/a"))
        );
    }

    /// S4: a client with no `workspaceFolders` at all (legal per the LSP
    /// spec, just deprecated) must still resolve its root from `rootUri`
    /// rather than being treated as having no root.
    #[test]
    fn resolve_root_falls_back_to_root_uri_without_workspace_folders() {
        let params: InitializeParams =
            serde_json::from_value(json!({ "capabilities": {} })).unwrap();
        let raw = json!({ "rootUri": "file:///b", "rootPath": "/c" });
        assert_eq!(
            resolve_root(&params, &raw),
            Some(std::path::PathBuf::from("/b"))
        );
    }

    /// S4: with neither `workspaceFolders` nor `rootUri`, `rootPath` (a
    /// plain filesystem path, not a URI) is the last fallback.
    #[test]
    fn resolve_root_falls_back_to_root_path_as_a_last_resort() {
        let params: InitializeParams =
            serde_json::from_value(json!({ "capabilities": {} })).unwrap();
        let raw = json!({ "rootPath": "/c" });
        assert_eq!(
            resolve_root(&params, &raw),
            Some(std::path::PathBuf::from("/c"))
        );
    }

    #[test]
    fn resolve_root_is_none_when_nothing_is_given() {
        let params: InitializeParams =
            serde_json::from_value(json!({ "capabilities": {} })).unwrap();
        assert_eq!(resolve_root(&params, &json!({})), None);
    }

    /// S4's decisive case: a client that sends only `rootUri` (no
    /// `workspaceFolders`) for a real `.rsx` crate root must have that
    /// root planned, not land in degraded mode the way "no root at all"
    /// does.
    #[test]
    fn a_root_uri_only_client_plans_a_real_rsx_workspace() {
        let tmp = std::env::temp_dir().join(format!(
            "outou-lsp-test-server-s4-{}-{}",
            std::process::id(),
            line!()
        ));
        let src = tmp.join("src");
        std::fs::create_dir_all(&src).expect("creating src dir");
        std::fs::write(src.join("main.rsx"), "fn main() {}\n").expect("writing crate root");

        let params: InitializeParams =
            serde_json::from_value(json!({ "capabilities": {} })).unwrap();
        let raw = json!({ "rootUri": uri::path_to_lsp(&tmp).as_str() });
        let root = resolve_root(&params, &raw).expect("a rootUri-only client resolves a root");

        match crate::documents::Workspace::load(&root).expect("loading the resolved root") {
            LoadOutcome::Planned(_) => {}
            LoadOutcome::Degraded { manifest_dir } => panic!(
                "a valid rootUri-only client must not land in degraded mode (searched {})",
                manifest_dir.display()
            ),
        }

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn apply_client_capabilities_reads_the_two_named_flags() {
        let mut state = crate::dispatch::State::new();
        let params: InitializeParams = serde_json::from_value(json!({
            "capabilities": {
                "workspace": { "configuration": true },
                "window": { "workDoneProgress": true },
            }
        }))
        .unwrap();
        apply_client_capabilities(&mut state, &params);
        assert!(state.client_supports_configuration);
        assert!(state.client_supports_work_done_progress);
    }
}
