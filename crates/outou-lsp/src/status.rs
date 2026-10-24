//! What the editor is told about rust-analyzer's readiness and health, in
//! Outou vocabulary only (issue #21).
//!
//! Two independent signals, neither relayed raw:
//!
//! - `experimental/serverStatus` (`health` != `ok`) becomes at most one
//!   `window/showMessage` per health state ([`next_health_notice`]).
//! - `$/progress` and `window/workDoneProgress/create` keep flowing to the
//!   editor, but only for tokens rust-analyzer itself created
//!   ([`ProgressTokens`]): those get an opaque `outou/progress/<n>` token
//!   (kept in both directions, so `window/workDoneProgress/cancel` can be
//!   mapped back) and a rewritten title and message
//!   ([`sanitize_progress_params`]): the upstream title is rust-analyzer's
//!   own ("Indexing", "cargo check", "Roots Scanned") and its message
//!   names crates and paths. A token the *editor* allocated passes
//!   untouched.

use std::collections::{HashMap, HashSet};

use lsp_types::{MessageType, ShowMessageParams};
use serde_json::{json, Value};

use crate::translate;

/// Method rust-analyzer uses for its experimental status extension.
pub(crate) const SERVER_STATUS_METHOD: &str = "experimental/serverStatus";

/// Longest upstream detail line appended to a health notice.
const MAX_DETAIL_CHARS: usize = 200;

/// A health state worth telling the user about (`ok` is not one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    Warning,
    Error,
}

/// Declares, in the capabilities sent *to rust-analyzer*, that this
/// server (as rust-analyzer's client) accepts `experimental/serverStatus`.
/// Without it rust-analyzer never sends the notification. This says
/// nothing about what the real editor supports.
pub(crate) fn declare_server_status(capabilities: &mut Value) {
    if !capabilities.is_object() {
        *capabilities = json!({});
    }
    let Some(root) = capabilities.as_object_mut() else {
        return;
    };
    let experimental = root.entry("experimental").or_insert_with(|| json!({}));
    if !experimental.is_object() {
        *experimental = json!({});
    }
    if let Some(map) = experimental.as_object_mut() {
        map.insert("serverStatusNotification".to_string(), Value::Bool(true));
    }
}

/// What one `experimental/serverStatus` notification says about health.
enum Reported {
    Ok,
    Problem(Health),
    /// Missing, `null`, not a string, or a value this code does not know:
    /// neither a problem nor a recovery, so the notice state is untouched.
    Unknown,
}

fn parse_health(params: &Value) -> Reported {
    match params.get("health").and_then(Value::as_str) {
        Some("ok") => Reported::Ok,
        Some("warning") => Reported::Problem(Health::Warning),
        Some("error") => Reported::Problem(Health::Error),
        _ => Reported::Unknown,
    }
}

/// Whether `text` may be shown to the editor: no backend marker, no
/// generated path, no rust-analyzer name. Checked on a normalized form
/// (lower case, `-` as `_`) and on a compact one with every
/// non-alphanumeric removed, so `DIOXUS_CORE`, `RSX!`, `dioxus-html` and
/// `rust analyzer` are all refused; when in doubt the detail is dropped.
fn is_clean(text: &str) -> bool {
    let normalized = text.to_lowercase().replace('-', "_");
    let compact: String = normalized.chars().filter(|c| c.is_alphanumeric()).collect();
    !translate::contains_backend_marker(text)
        && !normalized.contains(".generated")
        && !normalized.contains("rust_analyzer")
        && !["dioxus", "rsx", "rustanalyzer", "propsbuilder"]
            .iter()
            .any(|needle| compact.contains(needle))
}

fn detail_line(params: &Value) -> Option<String> {
    let line = params
        .get("message")
        .and_then(Value::as_str)?
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    let line: String = line.chars().take(MAX_DETAIL_CHARS).collect();
    is_clean(&line).then_some(line)
}

/// Decides what to do with one `experimental/serverStatus` notification.
///
/// `notified` is the health state already announced (`None` after `ok`
/// or before any announcement). Returns the new such state and, when the
/// state just changed to a non-`ok` one, the message to show. Only an
/// explicit `ok` re-arms; a repeat of the same state is silent; a status
/// without a recognizable `health` changes nothing and says nothing.
pub(crate) fn next_health_notice(
    notified: Option<Health>,
    params: &Value,
) -> (Option<Health>, Option<ShowMessageParams>) {
    let health = match parse_health(params) {
        Reported::Ok => return (None, None),
        Reported::Unknown => return (notified, None),
        Reported::Problem(health) => health,
    };
    if notified == Some(health) {
        return (notified, None);
    }
    let (typ, lead) = match health {
        Health::Warning => (
            MessageType::WARNING,
            "Outou: the project loaded with problems, so some editor features may be incomplete",
        ),
        Health::Error => (
            MessageType::ERROR,
            "Outou: the project could not be loaded, so editor features may be unavailable",
        ),
    };
    let message = match detail_line(params) {
        Some(detail) => format!("{lead} ({detail})"),
        None => lead.to_string(),
    };
    (Some(health), Some(ShowMessageParams { typ, message }))
}

/// Round-trip map for the work-done progress tokens rust-analyzer creates.
///
/// Only tokens announced by rust-analyzer's own
/// `window/workDoneProgress/create` are touched. Each gets an opaque,
/// collision-free `outou/progress/<n>` token for the editor and the
/// pair is remembered both ways. Tokens are LSP `string | integer`, so
/// they are keyed by their JSON text.
#[derive(Debug, Default)]
pub(crate) struct ProgressTokens {
    next: u64,
    /// rust-analyzer token -> editor token.
    to_editor: HashMap<String, Value>,
    /// editor token -> rust-analyzer token.
    to_upstream: HashMap<String, Value>,
    /// Tokens of a `create` answered locally (editor without
    /// work-done progress): their `$/progress` is dropped.
    suppressed: HashSet<String>,
}

impl ProgressTokens {
    /// Registers the token of a `create` that is being forwarded to the
    /// editor and returns the token to forward in its place.
    pub(crate) fn map_created(&mut self, upstream: &Value) -> Value {
        let key = upstream.to_string();
        if let Some(existing) = self.to_editor.get(&key) {
            return existing.clone();
        }
        let editor = Value::String(format!("outou/progress/{}", self.next));
        self.next += 1;
        self.to_editor.insert(key.clone(), editor.clone());
        self.to_upstream
            .insert(editor.to_string(), upstream.clone());
        editor
    }

    /// Records the token of a `create` answered locally, so its progress
    /// is not relayed.
    pub(crate) fn suppress(&mut self, upstream: &Value) {
        self.suppressed.insert(upstream.to_string());
    }

    /// The rust-analyzer token behind an editor token (for
    /// `window/workDoneProgress/cancel`); an unknown token is returned as is.
    pub(crate) fn upstream_for(&self, editor: &Value) -> Value {
        self.to_upstream
            .get(&editor.to_string())
            .cloned()
            .unwrap_or_else(|| editor.clone())
    }

    /// Decides what to do with a `$/progress` from rust-analyzer: the
    /// params to relay to the editor, or `None` to drop it. A mapped token
    /// is renamed and its payload rewritten (and forgotten on `end`); a
    /// suppressed one is dropped; any other token belongs to the editor
    /// and is relayed untouched.
    pub(crate) fn route_progress(&mut self, mut params: Value) -> Option<Value> {
        let key = params.get("token")?.to_string();
        if self.suppressed.contains(&key) {
            if is_end(&params) {
                self.suppressed.remove(&key);
            }
            return None;
        }
        let Some(editor) = self.to_editor.get(&key).cloned() else {
            return Some(params);
        };
        if is_end(&params) {
            self.to_editor.remove(&key);
            self.to_upstream.remove(&editor.to_string());
        }
        params["token"] = editor;
        Some(sanitize_progress_params(params))
    }
}

fn is_end(params: &Value) -> bool {
    params.pointer("/value/kind").and_then(Value::as_str) == Some("end")
}

fn outou_title(upstream: &str) -> &'static str {
    let lower = upstream.to_ascii_lowercase();
    if lower.contains("index") {
        "Outou: indexing"
    } else if lower.contains("fetch") {
        "Outou: fetching dependencies"
    } else if lower.contains("scan") || lower.contains("root") {
        "Outou: scanning files"
    } else if lower.contains("build") {
        "Outou: building"
    } else if lower.contains("check") || lower.contains("cargo") || lower.contains("clippy") {
        "Outou: checking"
    } else {
        "Outou: working"
    }
}

/// Keeps only a leading `done/total` counter of an upstream progress
/// message; the rest names crates or paths.
fn progress_counter(message: &str) -> Option<String> {
    let head = message.split_whitespace().next()?;
    let (done, total) = head.split_once('/')?;
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    (numeric(done) && numeric(total)).then(|| head.to_string())
}

/// Rewrites the payload of a rust-analyzer `$/progress` (the token is
/// [`ProgressTokens`]' business): `title` is replaced by an Outou title
/// and `message` reduced to its `done/total` counter or removed. Only a
/// `begin` / `report` / `end` object is touched.
pub(crate) fn sanitize_progress_params(mut params: Value) -> Value {
    let Some(value) = params.get_mut("value").and_then(Value::as_object_mut) else {
        return params;
    };
    if !matches!(
        value.get("kind").and_then(Value::as_str),
        Some("begin" | "report" | "end")
    ) {
        return params;
    }
    if let Some(title) = value.get("title").and_then(Value::as_str) {
        let rewritten = outou_title(title);
        value.insert("title".to_string(), Value::String(rewritten.to_string()));
    }
    if let Some(message) = value.remove("message") {
        if let Some(counter) = message.as_str().and_then(progress_counter) {
            value.insert("message".to_string(), Value::String(counter));
        }
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(health: &str, message: Option<&str>) -> Value {
        let mut v = json!({ "health": health, "quiescent": true });
        if let Some(m) = message {
            v["message"] = json!(m);
        }
        v
    }

    #[test]
    fn declare_server_status_sets_the_experimental_flag_and_keeps_siblings() {
        let mut caps = json!({ "experimental": { "other": 1 }, "window": {} });
        declare_server_status(&mut caps);
        assert_eq!(caps["experimental"]["serverStatusNotification"], true);
        assert_eq!(caps["experimental"]["other"], 1);
        let mut empty = Value::Null;
        declare_server_status(&mut empty);
        assert_eq!(empty["experimental"]["serverStatusNotification"], true);
    }

    #[test]
    fn warning_and_error_each_notify_once_with_the_right_severity() {
        let (state, msg) = next_health_notice(None, &status("warning", None));
        assert_eq!(state, Some(Health::Warning));
        assert_eq!(msg.expect("a notice").typ, MessageType::WARNING);
        let (state, msg) = next_health_notice(None, &status("error", None));
        assert_eq!(state, Some(Health::Error));
        assert_eq!(msg.expect("a notice").typ, MessageType::ERROR);
    }

    #[test]
    fn ok_is_silent() {
        let (state, msg) = next_health_notice(None, &status("ok", Some("fine")));
        assert_eq!(state, None);
        assert!(msg.is_none());
    }

    #[test]
    fn the_same_state_does_not_notify_again() {
        let (state, first) = next_health_notice(None, &status("error", None));
        assert!(first.is_some());
        let (state, second) = next_health_notice(state, &status("error", None));
        assert!(second.is_none());
        assert_eq!(state, Some(Health::Error));
    }

    #[test]
    fn returning_to_ok_rearms_the_notice() {
        let (state, _) = next_health_notice(None, &status("error", None));
        let (state, none) = next_health_notice(state, &status("ok", None));
        assert!(none.is_none());
        let (_, again) = next_health_notice(state, &status("error", None));
        assert!(again.is_some());
    }

    #[test]
    fn a_different_non_ok_state_notifies() {
        let (state, _) = next_health_notice(None, &status("warning", None));
        let (_, msg) = next_health_notice(state, &status("error", None));
        assert!(msg.is_some());
    }

    #[test]
    fn backend_vocabulary_in_the_upstream_message_never_reaches_the_editor() {
        for leaky in [
            "failed to expand rsx! macro",
            "dioxus_core failed",
            "error in src/.generated/main.rs",
            "rust-analyzer failed to load the workspace",
            "rust_analyzer::flycheck died",
            "GreetingPropsBuilder mismatch",
        ] {
            let (_, msg) = next_health_notice(None, &status("error", Some(leaky)));
            let text = msg.expect("a notice").message;
            assert!(!text.contains("rsx"), "{text}");
            assert!(!text.contains("dioxus"), "{text}");
            assert!(!text.contains(".generated"), "{text}");
            assert!(!text.to_lowercase().contains("rust-analyzer"), "{text}");
            assert!(!text.to_lowercase().contains("rust_analyzer"), "{text}");
            assert!(!text.contains("PropsBuilder"), "{text}");
            assert!(text.starts_with("Outou:"), "{text}");
        }
    }

    #[test]
    fn a_clean_upstream_detail_is_kept_in_the_notice() {
        let (_, msg) = next_health_notice(
            None,
            &status("error", Some("Failed to run `cargo metadata`\nmore")),
        );
        let text = msg.expect("a notice").message;
        assert!(text.contains("cargo metadata"), "{text}");
        assert!(!text.contains("more"), "{text}");
    }

    #[test]
    fn progress_title_is_rewritten_into_outou_vocabulary() {
        for (upstream, expected) in [
            ("Indexing", "Outou: indexing"),
            ("Roots Scanned", "Outou: scanning files"),
            ("Fetching", "Outou: fetching dependencies"),
            ("cargo check", "Outou: checking"),
            ("rust-analyzer: something odd", "Outou: working"),
        ] {
            let out = sanitize_progress_params(json!({
                "token": "rustAnalyzer/cachePriming",
                "value": { "kind": "begin", "title": upstream, "cancellable": false }
            }));
            assert_eq!(out["value"]["title"], expected);
            assert_eq!(out["value"]["kind"], "begin");
            assert_eq!(out["value"]["cancellable"], false);
        }
    }

    #[test]
    fn progress_message_keeps_only_the_counter() {
        let out = sanitize_progress_params(json!({
            "token": "t",
            "value": { "kind": "report", "message": "12/300 (dioxus_core)", "percentage": 4 }
        }));
        assert_eq!(out["value"]["message"], "12/300");
        assert_eq!(out["value"]["percentage"], 4);
        let out = sanitize_progress_params(json!({
            "token": "t",
            "value": { "kind": "report", "message": "outou_app(bin) rust-analyzer" }
        }));
        assert!(out["value"].get("message").is_none());
    }

    fn payload(kind: &str) -> Value {
        json!({ "kind": kind, "title": "rust-analyzer: Indexing", "message": "rust-analyzer 1/2 (dioxus)" })
    }

    #[test]
    fn no_progress_field_carries_backend_vocabulary() {
        let mut tokens = ProgressTokens::default();
        tokens.map_created(&json!("rust-analyzer/flycheck/0"));
        let out = tokens
            .route_progress(
                json!({ "token": "rust-analyzer/flycheck/0", "value": payload("begin") }),
            )
            .expect("relayed");
        let text = out.to_string().to_lowercase();
        assert!(!text.contains("rust-analyzer"), "{text}");
        assert!(!text.contains("dioxus"), "{text}");
    }

    #[test]
    fn distinct_upstream_tokens_get_distinct_opaque_tokens() {
        let mut tokens = ProgressTokens::default();
        let a = tokens.map_created(&json!("rustAnalyzer/x"));
        let b = tokens.map_created(&json!("rust-analyzer/x"));
        let c = tokens.map_created(&json!("outou/x"));
        let d = tokens.map_created(&json!(7));
        assert!(a != b && b != c && a != c && c != d);
        assert_eq!(tokens.map_created(&json!("rustAnalyzer/x")), a);
    }

    #[test]
    fn cancel_maps_back_and_unknown_tokens_pass_through() {
        let mut tokens = ProgressTokens::default();
        let editor = tokens.map_created(&json!("rust-analyzer/flycheck/0"));
        assert_eq!(
            tokens.upstream_for(&editor),
            json!("rust-analyzer/flycheck/0")
        );
        assert_eq!(
            tokens.upstream_for(&json!("client-token")),
            json!("client-token")
        );
    }

    #[test]
    fn end_forgets_the_mapping_and_unmapped_progress_is_untouched() {
        let mut tokens = ProgressTokens::default();
        let editor = tokens.map_created(&json!("rustAnalyzer/Fetching"));
        let out = tokens
            .route_progress(json!({ "token": "rustAnalyzer/Fetching", "value": { "kind": "end" } }))
            .expect("relayed");
        assert_eq!(out["token"], editor);
        assert_eq!(tokens.upstream_for(&editor), editor);
        let partial =
            json!({ "token": "client-partial", "value": [{ "title": "x", "message": "y" }] });
        assert_eq!(tokens.route_progress(partial.clone()), Some(partial));
    }

    #[test]
    fn suppressed_progress_is_dropped() {
        let mut tokens = ProgressTokens::default();
        tokens.suppress(&json!("rustAnalyzer/Indexing"));
        let params = json!({ "token": "rustAnalyzer/Indexing", "value": { "kind": "begin" } });
        assert_eq!(tokens.route_progress(params), None);
    }

    #[test]
    fn sanitize_skips_values_that_are_not_progress_objects() {
        let value = json!([{ "title": "Fix", "message": "x" }]);
        let params = json!({ "token": "t", "value": value });
        assert_eq!(sanitize_progress_params(params.clone()), params);
    }

    #[test]
    fn unknown_health_changes_nothing() {
        let (state, _) = next_health_notice(None, &status("error", None));
        for bad in [
            json!({}),
            json!({ "health": null }),
            json!({ "health": "degraded" }),
            json!({ "health": 3 }),
        ] {
            let (after, msg) = next_health_notice(state, &bad);
            assert_eq!(after, state);
            assert!(msg.is_none());
        }
    }

    #[test]
    fn spellings_of_backend_vocabulary_in_the_detail_are_all_refused() {
        for leaky in [
            "DIOXUS_CORE failed",
            "RSX! macro failed",
            "Dioxus failed to build",
            "failed to select a version for `dioxus-core`",
            "dioxus-html v0.7.10 build script failed",
            "Rust Analyzer crashed",
        ] {
            let (_, msg) = next_health_notice(None, &status("error", Some(leaky)));
            let text = msg.expect("a notice").message.to_lowercase();
            assert!(
                !text.contains("dioxus") && !text.contains("rsx") && !text.contains("analyzer"),
                "{text}"
            );
        }
    }
}
