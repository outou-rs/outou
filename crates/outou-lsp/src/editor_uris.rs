//! The editor's own spelling of `.rsx` document URIs.
//!
//! Internally every `.rsx` document is identified by its normalized URI
//! (`crate::uri::normalize_file_uri`: symbolic links resolved, one
//! percent-encoding), because the workspace, the source maps and
//! rust-analyzer all speak real paths. An editor that opened the
//! workspace through a symbolic link (or `/tmp` on macOS) however sends,
//! and expects back, the link-side spelling: a `publishDiagnostics` for
//! the real-path URI would land on a different resource and never show on
//! the open tab.
//!
//! [`EditorUris`] therefore sits on the two edges of the server:
//!
//! - inbound ([`EditorUris::normalize_incoming`]): rewrites
//!   `params.textDocument.uri` of every request and notification that
//!   targets a `.rsx` file, remembering the set of spellings the document
//!   is open under (the same file can be open as both the real path and
//!   the link) and which spelling each request used;
//! - outbound ([`EditorUris::restore_outgoing`], run by the thread
//!   [`spawn_outbound`] starts, so every message the server sends goes
//!   through it in order): `publishDiagnostics` goes to every spelling the
//!   document is open under (or to the real path when it is open under
//!   none); a spelling that had diagnostics but is no longer a target is
//!   cleared with one empty publish; the `uri` fields and `changes` keys
//!   of responses (definition, references, rename) use the spelling of the
//!   request they answer, else the most recently used one.
//!
//! A document is closed (reloaded from disk) only when the last spelling
//! closes. When the editor uses real paths only, every rewrite is the
//! identity.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use lsp_server::{Message, Notification, RequestId};
use serde_json::{json, Value};

use crate::uri::normalize_file_uri;

/// Private notification the main loop sends through the outbound channel
/// after a `didClose` was dispatched. The outbound thread consumes it (it
/// never reaches the editor), which keeps the table cleanup ordered after
/// the diagnostics that `didClose` itself published.
const FORGET_METHOD: &str = "$/outou/forgetEditorUri";

const PUBLISH_DIAGNOSTICS: &str = "textDocument/publishDiagnostics";

/// What the main loop does with a message after normalizing it.
#[derive(Default)]
pub struct Incoming {
    /// Send this through the outbound channel after dispatching the message.
    pub forget: Option<Message>,
    /// Do not dispatch the message: it closes one spelling of a document
    /// that is still open under another.
    pub swallow: bool,
}

/// Diagnostics currently displayed for one document.
#[derive(Default)]
struct Shown {
    /// Spellings that were last sent a non-empty diagnostic list.
    uris: BTreeSet<String>,
    /// The params of the last non-empty publish, to hand to the real path
    /// when the last spelling closes.
    last: Option<Value>,
}

#[derive(Default)]
struct Table {
    /// `normalized URI -> spellings it is open under`, least recently used
    /// first.
    open: HashMap<String, Vec<String>>,
    /// Spellings whose `didClose` was dispatched but whose forget marker the
    /// outbound thread has not processed yet; still diagnostic targets.
    closing: HashMap<String, Vec<String>>,
    /// `request id -> (normalized URI, spelling)` of `.rsx` requests.
    requests: HashMap<RequestId, (String, String)>,
    shown: HashMap<String, Shown>,
}

impl Table {
    fn targets(&self, normalized: &str) -> Vec<String> {
        let mut targets: Vec<String> = Vec::new();
        for list in [self.open.get(normalized), self.closing.get(normalized)]
            .into_iter()
            .flatten()
        {
            for spelling in list {
                if !targets.contains(spelling) {
                    targets.push(spelling.clone());
                }
            }
        }
        if targets.is_empty() {
            targets.push(normalized.to_string());
        }
        targets
    }

    fn latest_spelling(&self, normalized: &str) -> Option<&String> {
        self.open
            .get(normalized)
            .and_then(|list| list.last())
            .or_else(|| self.closing.get(normalized).and_then(|list| list.last()))
    }

    /// The messages that bring the displayed diagnostics of `normalized` in
    /// line with its current targets: one empty publish for every stale
    /// spelling, then `params` (when given) for every target.
    fn publish(&mut self, normalized: &str, params: Option<&Value>) -> Vec<Message> {
        let targets = self.targets(normalized);
        let shown = self.shown.entry(normalized.to_string()).or_default();
        let mut messages = Vec::new();
        let stale: Vec<String> = shown
            .uris
            .iter()
            .filter(|uri| !targets.contains(uri))
            .cloned()
            .collect();
        for uri in stale {
            shown.uris.remove(&uri);
            messages.push(publish_message(json!({ "uri": uri, "diagnostics": [] })));
        }
        if let Some(params) = params {
            let is_empty = params["diagnostics"]
                .as_array()
                .is_none_or(|list| list.is_empty());
            for target in targets {
                let mut copy = params.clone();
                copy["uri"] = Value::String(target.clone());
                messages.push(publish_message(copy));
                if is_empty {
                    shown.uris.remove(&target);
                } else {
                    shown.uris.insert(target);
                }
            }
            shown.last = (!is_empty).then(|| params.clone());
        }
        if shown.uris.is_empty() && shown.last.is_none() {
            self.shown.remove(normalized);
        }
        messages
    }

    /// After a spelling was forgotten: clear it and, if the diagnostics now
    /// have nowhere to show, put them on the remaining target.
    fn forget(&mut self, normalized: &str, spelling: &str) -> Vec<Message> {
        if let Some(list) = self.closing.get_mut(normalized) {
            list.retain(|s| s != spelling);
            if list.is_empty() {
                self.closing.remove(normalized);
            }
        }
        let last = self.shown.get(normalized).and_then(|s| s.last.clone());
        let mut messages = self.publish(normalized, None);
        if let Some(last) = last {
            let already: Vec<String> = self
                .shown
                .get(normalized)
                .map(|s| s.uris.iter().cloned().collect())
                .unwrap_or_default();
            let missing: Vec<String> = self
                .targets(normalized)
                .into_iter()
                .filter(|t| !already.contains(t))
                .collect();
            if !missing.is_empty() {
                let shown = self.shown.entry(normalized.to_string()).or_default();
                shown.last = Some(last.clone());
                for target in missing {
                    let mut copy = last.clone();
                    copy["uri"] = Value::String(target.clone());
                    messages.push(publish_message(copy));
                    shown.uris.insert(target);
                }
            }
        }
        messages
    }
}

fn publish_message(params: Value) -> Message {
    Message::Notification(Notification {
        method: PUBLISH_DIAGNOSTICS.to_string(),
        params,
    })
}

/// Per-document editor spellings. Cheap to clone; clones share the table.
#[derive(Clone, Default)]
pub struct EditorUris {
    table: Arc<Mutex<Table>>,
}

impl EditorUris {
    fn table(&self) -> MutexGuard<'_, Table> {
        // A poisoned table only means another thread panicked mid-update;
        // the maps are still usable.
        self.table
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Rewrites the `.rsx` document URI of an editor request or
    /// notification to its normalized form. Everything else is untouched.
    pub fn normalize_incoming(&self, message: &mut Message) -> Incoming {
        let (params, request_id, is_close) = match message {
            Message::Request(request) => (&mut request.params, Some(request.id.clone()), false),
            Message::Notification(note) => (
                &mut note.params,
                None,
                note.method == "textDocument/didClose",
            ),
            Message::Response(_) => return Incoming::default(),
        };
        let Some(slot) = params.pointer_mut("/textDocument/uri") else {
            return Incoming::default();
        };
        let Some(editor_uri) = slot.as_str() else {
            return Incoming::default();
        };
        if !editor_uri.ends_with(".rsx") {
            return Incoming::default();
        }
        let editor_uri = editor_uri.to_string();
        let normalized = normalize_file_uri(&editor_uri);
        *slot = Value::String(normalized.clone());
        if is_close {
            return self.note_close(&normalized, &editor_uri);
        }
        self.register(&normalized, &editor_uri);
        if let Some(id) = request_id {
            self.remember_request(id, &normalized, &editor_uri);
        }
        Incoming::default()
    }

    /// Notes that the document is open under `spelling` (and used it last).
    fn register(&self, normalized: &str, spelling: &str) {
        let mut table = self.table();
        let list = table.open.entry(normalized.to_string()).or_default();
        list.retain(|s| s != spelling);
        list.push(spelling.to_string());
    }

    fn remember_request(&self, id: RequestId, normalized: &str, spelling: &str) {
        self.table()
            .requests
            .insert(id, (normalized.to_string(), spelling.to_string()));
    }

    /// Notes a `didClose` of `spelling`. The spelling stays a diagnostics
    /// target until the returned marker has gone through the outbound thread.
    fn note_close(&self, normalized: &str, spelling: &str) -> Incoming {
        let mut table = self.table();
        let mut was_open = false;
        if let Some(list) = table.open.get_mut(normalized) {
            was_open = list.iter().any(|s| s == spelling);
            list.retain(|s| s != spelling);
            if list.is_empty() {
                table.open.remove(normalized);
            }
        }
        let still_open = table.open.contains_key(normalized);
        if was_open {
            table
                .closing
                .entry(normalized.to_string())
                .or_default()
                .push(spelling.to_string());
        }
        Incoming {
            forget: Some(Message::Notification(Notification {
                method: FORGET_METHOD.to_string(),
                params: json!({ "uri": normalized, "spelling": spelling }),
            })),
            swallow: still_open,
        }
    }

    /// The messages to send to the editor for `message`: usually the message
    /// itself with the editor's spelling restored; a `publishDiagnostics`
    /// fans out to every spelling and clears the ones no longer used.
    pub fn restore_outgoing(&self, message: Message) -> Vec<Message> {
        let mut table = self.table();
        match message {
            Message::Notification(note) if note.method == PUBLISH_DIAGNOSTICS => {
                let normalized = note.params["uri"].as_str().map(str::to_string);
                match normalized {
                    Some(uri) if uri.ends_with(".rsx") => table.publish(&uri, Some(&note.params)),
                    _ => vec![Message::Notification(note)],
                }
            }
            Message::Notification(note) if note.method == FORGET_METHOD => {
                let uri = note.params["uri"].as_str().unwrap_or_default();
                let spelling = note.params["spelling"].as_str().unwrap_or_default();
                table.forget(uri, spelling)
            }
            Message::Response(mut response) => {
                let answered = table.requests.remove(&response.id);
                if let Ok(result) = &mut response.response_result {
                    let lookup = |uri: &str| match &answered {
                        Some((normalized, spelling)) if normalized == uri => Some(spelling.clone()),
                        _ => table.latest_spelling(uri).cloned(),
                    };
                    restore_in_value(&lookup, result);
                }
                vec![Message::Response(response)]
            }
            other => vec![other],
        }
    }

    /// Number of `(document, spelling)` pairs whose spelling is not the
    /// normalized URI itself.
    #[cfg(test)]
    pub fn alias_count(&self) -> usize {
        let table = self.table();
        [&table.open, &table.closing]
            .into_iter()
            .flat_map(|map| map.iter())
            .flat_map(|(normalized, list)| list.iter().filter(move |s| *s != normalized))
            .count()
    }
}

/// Replaces `uri`/`targetUri` string fields and the keys of a `changes`
/// object that `lookup` knows a spelling for.
fn restore_in_value(lookup: &dyn Fn(&str) -> Option<String>, value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match child {
                    Value::String(text) if key == "uri" || key == "targetUri" => {
                        if let Some(editor_uri) = lookup(text) {
                            *text = editor_uri;
                        }
                    }
                    Value::Object(changes) if key == "changes" => {
                        let renamed: serde_json::Map<String, Value> = std::mem::take(changes)
                            .into_iter()
                            .map(|(uri, edits)| (lookup(&uri).unwrap_or(uri), edits))
                            .collect();
                        *changes = renamed;
                    }
                    _ => restore_in_value(lookup, child),
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| restore_in_value(lookup, item)),
        _ => {}
    }
}

/// Starts the thread that restores the editor's URI spelling in every
/// message before handing it to `to_editor`, preserving order. Returns the
/// sender the rest of the server must use instead of the real one, and the
/// thread to join once that sender is dropped.
pub fn spawn_outbound(
    to_editor: Sender<Message>,
    uris: EditorUris,
) -> (Sender<Message>, JoinHandle<()>) {
    let (sender, receiver): (Sender<Message>, Receiver<Message>) = crossbeam_channel::unbounded();
    let handle = std::thread::spawn(move || {
        for message in receiver {
            for outgoing in uris.restore_outgoing(message) {
                if to_editor.send(outgoing).is_err() {
                    return;
                }
            }
        }
    });
    (sender, handle)
}

#[cfg(test)]
mod tests;
