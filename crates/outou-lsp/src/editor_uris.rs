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
//!   targets a `.rsx` file, remembering `normalized -> editor spelling`
//!   whenever the two differ;
//! - outbound ([`EditorUris::restore_outgoing`], run by the thread
//!   [`spawn_outbound`] starts, so every message the server sends goes
//!   through it in order): turns a remembered normalized URI back into the
//!   editor's spelling in `publishDiagnostics` and in the `uri` fields and
//!   `changes` keys of responses (definition, references, rename).
//!
//! When the editor already uses the normalized spelling nothing is
//! remembered and every rewrite is the identity.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use lsp_server::{Message, Notification};
use serde_json::Value;

use crate::uri::normalize_file_uri;

/// Private notification the main loop sends through the outbound channel
/// after a `didClose` was dispatched. The outbound thread consumes it (it
/// never reaches the editor), which keeps the table cleanup ordered after
/// the diagnostics that `didClose` itself published.
const FORGET_METHOD: &str = "$/outou/forgetEditorUri";

/// `normalized URI -> the spelling the editor used`, only for the URIs
/// where they differ. Cheap to clone; clones share the table.
#[derive(Clone, Default)]
pub struct EditorUris {
    aliases: Arc<Mutex<HashMap<String, String>>>,
}

impl EditorUris {
    fn table(&self) -> MutexGuard<'_, HashMap<String, String>> {
        // A poisoned table only means another thread panicked mid-insert;
        // the map itself is still usable.
        self.aliases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Rewrites the `.rsx` document URI of an editor request or
    /// notification to its normalized form. Everything else is untouched.
    pub fn normalize_incoming(&self, message: &mut Message) {
        let params = match message {
            Message::Request(request) => &mut request.params,
            Message::Notification(notification) => &mut notification.params,
            Message::Response(_) => return,
        };
        let Some(slot) = params.pointer_mut("/textDocument/uri") else {
            return;
        };
        let Some(editor_uri) = slot.as_str() else {
            return;
        };
        if !editor_uri.ends_with(".rsx") {
            return;
        }
        let normalized = normalize_file_uri(editor_uri);
        if normalized != editor_uri {
            self.table()
                .insert(normalized.clone(), editor_uri.to_string());
        }
        *slot = Value::String(normalized);
    }

    /// Restores the editor's spelling in a message about to be sent to it.
    pub fn restore_outgoing(&self, message: &mut Message) {
        let table = self.table();
        if table.is_empty() {
            return;
        }
        match message {
            Message::Notification(notification)
                if notification.method == "textDocument/publishDiagnostics" =>
            {
                restore_in_value(&table, &mut notification.params);
            }
            Message::Response(response) => {
                if let Ok(result) = &mut response.response_result {
                    restore_in_value(&table, result);
                }
            }
            _ => {}
        }
    }

    /// Drops the alias of a closed document.
    fn forget(&self, normalized: &str) {
        self.table().remove(normalized);
    }

    /// The message that makes the outbound thread [`forget`](Self::forget)
    /// `message`'s document, if `message` is a `.rsx` `didClose`.
    pub fn forget_after(message: &Message) -> Option<Message> {
        let Message::Notification(notification) = message else {
            return None;
        };
        if notification.method != "textDocument/didClose" {
            return None;
        }
        let uri = notification.params.pointer("/textDocument/uri")?.as_str()?;
        uri.ends_with(".rsx").then(|| {
            Message::Notification(Notification {
                method: FORGET_METHOD.to_string(),
                params: serde_json::json!({ "uri": uri }),
            })
        })
    }

    #[cfg(test)]
    pub fn alias_count(&self) -> usize {
        self.table().len()
    }
}

/// Replaces `uri`/`targetUri` string fields and the keys of a `changes`
/// object that are a remembered normalized URI.
fn restore_in_value(table: &HashMap<String, String>, value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                match child {
                    Value::String(text) if key == "uri" || key == "targetUri" => {
                        if let Some(editor_uri) = table.get(text.as_str()) {
                            *text = editor_uri.clone();
                        }
                    }
                    Value::Object(changes) if key == "changes" => {
                        let renamed: serde_json::Map<String, Value> = std::mem::take(changes)
                            .into_iter()
                            .map(|(uri, edits)| (table.get(&uri).cloned().unwrap_or(uri), edits))
                            .collect();
                        *changes = renamed;
                    }
                    _ => restore_in_value(table, child),
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| restore_in_value(table, item)),
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
        for mut message in receiver {
            if let Message::Notification(note) = &message {
                if note.method == FORGET_METHOD {
                    if let Some(uri) = note.params.get("uri").and_then(Value::as_str) {
                        uris.forget(uri);
                    }
                    continue;
                }
            }
            uris.restore_outgoing(&mut message);
            if to_editor.send(message).is_err() {
                return;
            }
        }
    });
    (sender, handle)
}

#[cfg(test)]
mod tests;
