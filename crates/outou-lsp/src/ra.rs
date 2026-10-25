//! Spawns rust-analyzer as a child process and speaks JSON-RPC to it.
//!
//! Reuses [`lsp_server::Message`]'s own `read`/`write` framing (the exact
//! `Content-Length` framing rust-analyzer speaks to *its* client) instead
//! of hand-rolling one: this crate is both a server (to the editor) and a
//! client (to rust-analyzer), and `Message` works over any `BufRead`/`Write`
//! regardless of which end holds it.
//!
//! Concurrency (issue #9 architecture note): one thread reads rust-analyzer's
//! stdout and pushes every parsed [`Message`] onto [`RaClient::receiver`];
//! the caller (the main loop in `crate::server`) is the only thread that
//! ever calls [`RaClient::send`], so writes need no extra synchronization
//! beyond the `Mutex` guarding the child's stdin handle itself. Outgoing
//! request ids are this client's own monotonically increasing counter,
//! independent of the editor's id namespace, precisely so that responses
//! can always be matched back to the right pending request even though the
//! two namespaces can otherwise collide (both commonly start at 1).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use crossbeam_channel::Receiver;
use lsp_server::{Message, Notification, Request, RequestId};
use serde_json::Value;

/// How long [`RaClient::wait_for_response`] waits for rust-analyzer's
/// `initialize`/`shutdown` responses before giving up (issue #9 Gate 3
/// review, S5/MEDIUM-11): neither wait had any deadline at all, so a
/// rust-analyzer that hung during startup or shutdown — rather than
/// cleanly exiting, which the existing "channel closed" path already
/// handled — wedged this server forever with no diagnostic.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Environment variable that turns rust-analyzer's standard error on.
///
/// By default that stream is drained and dropped: it carries rust-analyzer's
/// own log lines and the full text of every `textDocument/didSave` it is
/// sent, i.e. the generated Rust and its `.generated` paths, which Outou
/// keeps out of the user's sight (ledger row 54). A developer debugging
/// rust-analyzer itself sets this to anything other than empty or `0` and
/// gets the stream, unmodified, on this server's own stderr.
pub const RA_LOG_ENV: &str = "OUTOU_LSP_RA_LOG";

/// Whether `value` (the content of [`RA_LOG_ENV`], if set) asks for
/// rust-analyzer's standard error to be shown.
fn ra_log_requested(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty() && v != "0")
}

/// Reads `reader` (rust-analyzer's standard error) to the end, writing every
/// line to `sink` unchanged when `forward` is set and discarding it
/// otherwise. It always reads: an undrained pipe would block rust-analyzer
/// once the OS buffer fills.
fn drain_stderr(reader: impl Read, forward: bool, mut sink: impl Write) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) if forward => {
                let _ = sink.write_all(&line);
            }
            Ok(_) => {}
        }
    }
}

/// A running rust-analyzer child process and the channel its messages
/// arrive on.
pub struct RaClient {
    child: Child,
    stdin: Mutex<ChildStdin>,
    next_id: AtomicI32,
    /// Every [`Message`] rust-analyzer sends: responses to our requests,
    /// its own requests to us (`window/workDoneProgress/create`,
    /// `client/registerCapability`), and notifications
    /// (`textDocument/publishDiagnostics`, `$/progress`).
    pub receiver: Receiver<Message>,
}

/// Errors from [`RaClient::spawn`].
#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    /// The binary could not be started at all (not found, not executable).
    #[error("could not start rust-analyzer (`{binary}`): {source}")]
    Spawn {
        /// The binary path or name that was tried.
        binary: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
}

impl RaClient {
    /// Spawns `binary` (looked up on `PATH` if it is a bare name) with
    /// `current_dir` set to the crate root, and starts the background
    /// reader thread.
    pub fn spawn(binary: &str, current_dir: &Path) -> Result<Self, SpawnError> {
        let mut child = Command::new(binary)
            .current_dir(current_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| SpawnError::Spawn {
                binary: binary.to_string(),
                source,
            })?;

        let stdout = child.stdout.take().expect("stdout was piped");
        let stdin = child.stdin.take().expect("stdin was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let forward = ra_log_requested(std::env::var(RA_LOG_ENV).ok().as_deref());
        thread::Builder::new()
            .name("outou-lsp-ra-stderr".to_string())
            .spawn(move || drain_stderr(stderr, forward, std::io::stderr()))
            .expect("spawning the rust-analyzer stderr thread");
        let (sender, receiver) = crossbeam_channel::unbounded();

        thread::Builder::new()
            .name("outou-lsp-ra-reader".to_string())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                while let Ok(Some(message)) = Message::read(&mut reader) {
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            })
            .expect("spawning the rust-analyzer reader thread");

        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            next_id: AtomicI32::new(1),
            receiver,
        })
    }

    /// The next outgoing request id, unique for the life of this client.
    pub fn next_id(&self) -> RequestId {
        RequestId::from(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Writes one message to rust-analyzer's stdin.
    ///
    /// Errors are swallowed (logged to stderr): once the pipe is broken
    /// (the child died) there is nothing a caller could usefully do with
    /// the error beyond what the reader thread's channel closing already
    /// signals to the main loop.
    pub fn send(&self, message: Message) {
        let mut stdin = self
            .stdin
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Err(e) = message.write(&mut *stdin) {
            eprintln!("outou-lsp: writing to rust-analyzer: {e}");
        }
    }

    /// Sends a request with a freshly allocated id and returns it, so the
    /// caller can record what the response is for.
    pub fn request(&self, method: impl Into<String>, params: Value) -> RequestId {
        let id = self.next_id();
        self.send(Message::Request(Request {
            id: id.clone(),
            method: method.into(),
            params,
        }));
        id
    }

    /// Sends a notification.
    pub fn notify(&self, method: impl Into<String>, params: Value) {
        self.send(Message::Notification(Notification {
            method: method.into(),
            params,
        }));
    }

    /// Blocks until a [`Message::Response`] with `id` arrives, handling
    /// every other message with `on_other` along the way (used during
    /// startup, before the main event loop is running, to drive
    /// rust-analyzer's own `initialize` handshake while still answering
    /// its `window/workDoneProgress/create` and similar requests), or
    /// until [`HANDSHAKE_TIMEOUT`] elapses.
    ///
    /// Returns `None` if rust-analyzer's stdout closed before the
    /// response arrived (the child exited or crashed during startup), or
    /// if the deadline was reached first (issue #9 Gate 3 review,
    /// S5/MEDIUM-11: neither `initialize` nor `shutdown` had any deadline
    /// before this, so a rust-analyzer that hung — rather than exiting —
    /// wedged this server forever).
    pub fn wait_for_response(
        &self,
        id: &RequestId,
        mut on_other: impl FnMut(Message),
    ) -> Option<lsp_server::Response> {
        let deadline = std::time::Instant::now() + HANDSHAKE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(Message::Response(response)) if response.id == *id => return Some(response),
                Ok(other) => on_other(other),
                Err(_) => return None,
            }
        }
    }

    /// Best-effort graceful shutdown: `shutdown` request, `exit`
    /// notification, then kill if the process has not exited on its own
    /// shortly after. Bounded by [`HANDSHAKE_TIMEOUT`] via
    /// [`Self::wait_for_response`] — a rust-analyzer that never answers
    /// `shutdown` no longer prevents this server from exiting.
    pub fn shutdown(&mut self) {
        let id = self.request("shutdown", Value::Null);
        let _ = self.wait_for_response(&id, |_| {});
        self.notify("exit", Value::Null);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for RaClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOISE: &[u8] = b"WARN overly long loop turn\nNotification { text: \"// Generated by Outou from src/a.rsx\" }\nno trailing newline";

    #[test]
    fn ra_log_requested_only_for_a_non_empty_value_other_than_zero() {
        assert!(!ra_log_requested(None));
        assert!(!ra_log_requested(Some("")));
        assert!(!ra_log_requested(Some("0")));
        assert!(ra_log_requested(Some("1")));
        assert!(ra_log_requested(Some("true")));
    }

    #[test]
    fn drain_stderr_discards_everything_by_default() {
        let mut sink = Vec::new();
        drain_stderr(NOISE, false, &mut sink);
        assert!(sink.is_empty());
    }

    #[test]
    fn drain_stderr_forwards_bytes_unchanged_when_asked() {
        let mut sink = Vec::new();
        drain_stderr(NOISE, true, &mut sink);
        assert_eq!(sink, NOISE);
    }
}
