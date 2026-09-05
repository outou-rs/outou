//! Request/notification dispatch: everything that happens once
//! `crate::server`'s handshake is done. Requests this server intercepts
//! (`textDocument/{hover,completion,definition}`) are rewritten to the
//! generated file and position before being forwarded to rust-analyzer,
//! and the response mapped back before being relayed to the editor
//! (`crate::mapping`, `crate::response`); everything else is forwarded
//! transparently.
//!
//! Split by direction of traffic: [`requests`] handles requests the
//! editor sends this server, [`notifications`] handles `.rsx`
//! `didOpen`/`didChange`/`didSave`/`didClose`, and [`responses`] handles
//! everything coming back from rust-analyzer (its responses, its own
//! requests to the editor, and its notifications).

use std::collections::HashMap;

use lsp_server::RequestId;

use crate::documents::{self, Workspace};
use crate::ra::RaClient;

mod notifications;
mod requests;
mod responses;

pub(crate) use notifications::dispatch_client_notification;
pub(crate) use requests::{dispatch_client_request, fail_all_pending};
pub(crate) use responses::{dispatch_client_response, dispatch_ra_message};

/// One rust-analyzer request this server is waiting on a response for.
struct Pending {
    client_id: RequestId,
    kind: PendingKind,
    /// [`State::epoch`] at the moment this request was sent (S3, issue #9
    /// Gate 3 review): a re-plan invalidates every previously-generated
    /// unit's positions, so a response that arrives after the workspace
    /// has moved on to a later epoch is answered `RequestCancelled`
    /// rather than mapped against state that may no longer describe the
    /// file the editor is now showing.
    epoch: u64,
}

enum PendingKind {
    Hover {
        generated_uri: lsp_types::Uri,
    },
    Definition,
    Completion {
        generated_uri: lsp_types::Uri,
        /// The generated-file position this request was sent for, so the
        /// response mapping can drop any item whose returned edit range
        /// does not contain it (issue #9 Gate 3 review, M4/HIGH-8: a
        /// wrongly mapped `textEdit` corrupts the buffer if accepted).
        cursor: lsp_types::Position,
        /// The original `.rsx` position the editor actually asked about,
        /// so the response mapping can re-check cursor containment a
        /// *second* time, after mapping each item's edit back to `.rsx`
        /// coordinates (issue #9 Gate 3 review, H1): `cursor` above only
        /// catches a mismatch in *generated* coordinates; a mapping that
        /// reverse-maps to the wrong `.rsx` location entirely (e.g. an
        /// element name whose mapping has more than one source always
        /// resolving to the first — a closing tag's own occurrence
        /// resolving to its opening tag) passes that check while still
        /// landing on the wrong place, and is only caught here.
        rsx_cursor: lsp_types::Position,
    },
    /// Forwarded verbatim; the response is relayed with no mapping.
    Forward,
}

/// Everything the dispatch loop needs, for one `initialize`d root.
pub(crate) struct State {
    pub(crate) workspace: Option<Workspace>,
    /// `.rsx` documents kept only in degraded mode (no crate root), for
    /// which there is no [`Workspace`] to hold them.
    pub(crate) degraded_docs: HashMap<String, documents::RsxDocument>,
    pub(crate) ra: Option<RaClient>,
    pending: HashMap<RequestId, Pending>,
    /// One rust-analyzer -> client request this server forwarded rather
    /// than answering itself (issue #9 Gate 3 review, M9/S2's
    /// `client/registerCapability`/`window/workDoneProgress/create`
    /// forwarding): keyed by the freshly allocated id sent to the editor,
    /// valued by rust-analyzer's own original id, so the editor's
    /// eventual response can be routed back to the right rust-analyzer
    /// request.
    reverse_pending: HashMap<RequestId, RequestId>,
    /// Counter for [`State::reverse_pending`]'s editor-facing ids,
    /// starting well above any id a real editor is likely to have
    /// allocated on its own (they conventionally start at 1 and count
    /// up), to make an accidental collision with the editor's own
    /// request-id namespace unlikely without requiring this server to
    /// track every id the editor has ever used.
    next_forwarded_id: i32,
    /// Whether the editor's `initialize` capabilities advertised
    /// `workspace.configuration` support — LSP 3.6+'s
    /// `WorkspaceClientCapabilities::configuration`.
    pub(crate) client_supports_configuration: bool,
    /// Whether the editor's `initialize` capabilities advertised
    /// `window.workDoneProgress` support.
    pub(crate) client_supports_work_done_progress: bool,
    /// Whether *any* of the editor's `initialize` capabilities advertised
    /// `dynamicRegistration: true` anywhere — the umbrella condition for
    /// `client/registerCapability` being meaningful to forward at all
    /// (LSP has no single top-level flag for this; it is declared
    /// per-capability).
    pub(crate) client_supports_dynamic_registration: bool,
    /// Bumped by [`notifications::replan_and_resync`] every time a
    /// crate-wide re-plan succeeds. See [`Pending::epoch`].
    epoch: u64,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            workspace: None,
            degraded_docs: HashMap::new(),
            ra: None,
            pending: HashMap::new(),
            reverse_pending: HashMap::new(),
            next_forwarded_id: 1_000_000_000,
            client_supports_configuration: false,
            client_supports_work_done_progress: false,
            client_supports_dynamic_registration: false,
            epoch: 0,
        }
    }

    fn next_forwarded_client_id(&mut self) -> RequestId {
        let id = self.next_forwarded_id;
        self.next_forwarded_id += 1;
        RequestId::from(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_forwarded_client_id_never_repeats() {
        let mut state = State::new();
        let a = state.next_forwarded_client_id();
        let b = state.next_forwarded_client_id();
        assert_ne!(a, b);
    }
}
