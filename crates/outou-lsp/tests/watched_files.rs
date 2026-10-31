//! `workspace/didChangeWatchedFiles` (#31, F17 (d)): edits made outside the
//! editor (`git checkout`, a formatter run from a terminal) to `.rsx` files
//! that are not open must reach the workspace without a save.
//!
//! Drives the real `outou-lsp` binary over stdio. No rust-analyzer is needed:
//! the server runs with Outou's own syntax diagnostics when it cannot start
//! one, and those are what these tests look at.

#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(20);
const VALID_WIDGET: &str = "pub fn widget() {}\n";
const BROKEN_WIDGET: &str = "pub fn widget() { let _x = <div>; }\n";
const WATCH_CREATED: u32 = 1;
const WATCH_CHANGED: u32 = 2;
const WATCH_DELETED: u32 = 3;

struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    root: PathBuf,
    next_id: u32,
    next_sentinel: u32,
}

impl Client {
    fn start(supports_watch_registration: bool) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "outou-lsp-watched-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
             [[bin]]\nname = \"app\"\npath = \"src/.generated/crate-root.rs\"\n",
        )
        .unwrap();
        fs::write(dir.join("src/main.rsx"), "mod widget;\nfn main() {}\n").unwrap();
        fs::write(dir.join("src/widget.rsx"), VALID_WIDGET).unwrap();
        let root = dir.canonicalize().unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_outou-lsp"))
            .env("OUTOU_RUST_ANALYZER", "/nonexistent/rust-analyzer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, messages) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Some(message) = read_frame(&mut reader) {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });

        let mut client = Self {
            child,
            stdin,
            messages,
            root,
            next_id: 1,
            next_sentinel: 0,
        };
        let workspace = if supports_watch_registration {
            json!({"didChangeWatchedFiles": {"dynamicRegistration": true}})
        } else {
            json!({})
        };
        let init = json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "processId": null,
            "capabilities": {"workspace": workspace},
            "workspaceFolders": [{"uri": client.file_uri(""), "name": "app"}]}});
        client.send(&init);
        client.wait_for(|m| m["id"] == 0 && m.get("result").is_some());
        client.notify("initialized", json!({}));
        client
    }

    fn file_uri(&self, relative: &str) -> String {
        let path = self.root.join(relative);
        format!(
            "file://{}",
            path.display().to_string().trim_end_matches('/')
        )
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn send(&mut self, body: &Value) {
        let text = body.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{text}", text.len()).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc":"2.0","method":method,"params":params}));
    }

    fn did_open(&mut self, relative: &str, text: &str, version: i32) {
        let uri = self.file_uri(relative);
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "outou-rsx", "version": version, "text": text}}),
        );
    }

    fn watched(&mut self, changes: &[(&str, u32)]) {
        let changes: Vec<Value> = changes
            .iter()
            .map(|(relative, kind)| json!({"uri": self.file_uri(relative), "type": kind}))
            .collect();
        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": changes }),
        );
    }

    /// Next message satisfying `matches`, skipping others; panics on timeout.
    fn wait_for(&self, matches: impl Fn(&Value) -> bool) -> Value {
        self.try_wait_for(matches, TIMEOUT)
            .expect("timed out waiting for a message")
    }

    fn try_wait_for(&self, matches: impl Fn(&Value) -> bool, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.checked_duration_since(Instant::now())?;
            let message = self.messages.recv_timeout(remaining).ok()?;
            if matches(&message) {
                return Some(message);
            }
        }
    }

    /// The diagnostics of the next `publishDiagnostics` for `relative`.
    fn next_diagnostics(&self, relative: &str) -> Vec<Value> {
        let uri = self.file_uri(relative);
        let message = self.wait_for(|m| is_publish_for(m, &uri));
        message["params"]["diagnostics"].as_array().unwrap().clone()
    }

    /// Opens `main.rsx` and waits for its diagnostics. Messages are handled
    /// in order, so everything the server sent because of earlier
    /// notifications has already been seen when this returns. Returns every
    /// message received on the way.
    fn flush(&mut self) -> Vec<Value> {
        let uri = self.file_uri("src/main.rsx");
        self.did_open("src/main.rsx", "mod widget;\nfn main() {}\n", 1);
        let mut seen = Vec::new();
        loop {
            let message = self.wait_for(|_| true);
            let done = is_publish_for(&message, &uri);
            seen.push(message);
            if done {
                return seen;
            }
        }
    }

    /// Opens a fresh file no `mod` reaches and waits for its diagnostics.
    /// Messages are handled in order, so everything the server sent because
    /// of earlier notifications has been seen when this returns (unlike
    /// [`Self::flush`], whose `main.rsx` publish may come from a re-plan).
    /// Returns every message received on the way.
    fn sync(&mut self) -> Vec<Value> {
        self.next_sentinel += 1;
        let relative = format!("src/sentinel{}.rsx", self.next_sentinel);
        let uri = self.file_uri(&relative);
        self.did_open(&relative, "", 1);
        let mut seen = Vec::new();
        loop {
            let message = self.wait_for(|_| true);
            let done = is_publish_for(&message, &uri);
            seen.push(message);
            if done {
                return seen;
            }
        }
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn shutdown(mut self) {
        let id = self.id();
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":"shutdown"}));
        self.send(&json!({"jsonrpc":"2.0","method":"exit"}));
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn is_publish_for(message: &Value, uri: &str) -> bool {
    message["method"] == "textDocument/publishDiagnostics" && message["params"]["uri"] == uri
}

fn is_watch_registration(message: &Value) -> bool {
    message["method"] == "client/registerCapability"
}

fn read_frame(reader: &mut BufReader<impl Read>) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length?];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

/// Answers the server's registration request and returns the watchers.
fn accept_registration(client: &mut Client) -> Vec<Value> {
    let request = client.wait_for(is_watch_registration);
    let registrations = request["params"]["registrations"].as_array().unwrap();
    assert_eq!(registrations.len(), 1, "{request}");
    assert_eq!(
        registrations[0]["method"], "workspace/didChangeWatchedFiles",
        "{request}"
    );
    let watchers = registrations[0]["registerOptions"]["watchers"]
        .as_array()
        .unwrap()
        .clone();
    client.send(&json!({"jsonrpc":"2.0","id":request["id"],"result":null}));
    watchers
}

#[test]
fn registers_the_watchers_when_the_client_supports_dynamic_registration() {
    let mut client = Client::start(true);
    let watchers = accept_registration(&mut client);
    let globs: Vec<&str> = watchers
        .iter()
        .map(|w| w["globPattern"].as_str().unwrap())
        .collect();
    assert!(globs.contains(&"**/*.rsx"), "{globs:?}");
    assert!(globs.contains(&"**/Cargo.toml"), "{globs:?}");
    client.shutdown();
}

#[test]
fn registers_nothing_when_the_client_does_not_support_dynamic_registration() {
    let mut client = Client::start(false);
    let seen = client.flush();
    assert!(
        !seen.iter().any(is_watch_registration),
        "unexpected registration: {seen:?}"
    );
    client.shutdown();
}

#[test]
fn an_external_syntax_error_in_a_closed_file_is_published_and_fixing_it_clears_it() {
    let mut client = Client::start(true);
    accept_registration(&mut client);

    write(&client.path("src/widget.rsx"), BROKEN_WIDGET);
    client.watched(&[("src/widget.rsx", WATCH_CHANGED)]);
    let broken = client.next_diagnostics("src/widget.rsx");
    assert!(!broken.is_empty(), "the syntax error must be published");

    write(&client.path("src/widget.rsx"), VALID_WIDGET);
    client.watched(&[("src/widget.rsx", WATCH_CHANGED)]);
    assert!(client.next_diagnostics("src/widget.rsx").is_empty());
    client.shutdown();
}

#[test]
fn deleting_a_file_clears_its_diagnostics() {
    let mut client = Client::start(true);
    accept_registration(&mut client);

    write(&client.path("src/widget.rsx"), BROKEN_WIDGET);
    client.watched(&[("src/widget.rsx", WATCH_CHANGED)]);
    assert!(!client.next_diagnostics("src/widget.rsx").is_empty());

    fs::remove_file(client.path("src/widget.rsx")).unwrap();
    client.watched(&[("src/widget.rsx", WATCH_DELETED)]);
    assert!(client.next_diagnostics("src/widget.rsx").is_empty());
    client.shutdown();
}

#[test]
fn recreating_a_deleted_file_brings_its_diagnostics_back() {
    let mut client = Client::start(true);
    accept_registration(&mut client);

    fs::remove_file(client.path("src/widget.rsx")).unwrap();
    client.watched(&[("src/widget.rsx", WATCH_DELETED)]);
    write(&client.path("src/widget.rsx"), BROKEN_WIDGET);
    client.watched(&[("src/widget.rsx", WATCH_CREATED)]);
    let broken = client.next_diagnostics("src/widget.rsx");
    let broken = if broken.is_empty() {
        client.next_diagnostics("src/widget.rsx")
    } else {
        broken
    };
    assert!(!broken.is_empty());
    client.shutdown();
}

#[test]
fn a_change_event_for_a_file_open_in_the_editor_is_ignored() {
    let mut client = Client::start(true);
    accept_registration(&mut client);
    client.did_open("src/widget.rsx", VALID_WIDGET, 1);
    assert!(client.next_diagnostics("src/widget.rsx").is_empty());

    write(&client.path("src/widget.rsx"), BROKEN_WIDGET);
    client.watched(&[("src/widget.rsx", WATCH_CHANGED)]);
    let widget_uri = client.file_uri("src/widget.rsx");
    let seen = client.flush();
    assert!(
        !seen.iter().any(|m| is_publish_for(m, &widget_uri)),
        "the open buffer is authoritative; the disk change must be ignored: {seen:?}"
    );
    client.shutdown();
}

fn publishes_for<'a>(seen: &'a [Value], uri: &str) -> Vec<&'a Value> {
    seen.iter().filter(|m| is_publish_for(m, uri)).collect()
}

fn is_empty_publish(message: &Value) -> bool {
    message["params"]["diagnostics"]
        .as_array()
        .is_some_and(|d| d.is_empty())
}

#[test]
fn a_deleted_then_created_file_in_one_batch_keeps_its_diagnostics() {
    let mut client = Client::start(true);
    accept_registration(&mut client);

    write(&client.path("src/widget.rsx"), BROKEN_WIDGET);
    client.watched(&[
        ("src/widget.rsx", WATCH_DELETED),
        ("src/widget.rsx", WATCH_CREATED),
    ]);
    let seen = client.sync();
    let widget_uri = client.file_uri("src/widget.rsx");
    let last = *publishes_for(&seen, &widget_uri)
        .last()
        .expect("the re-plan republishes the file");
    assert!(
        !is_empty_publish(last),
        "the file exists again; it must not be forgotten: {seen:?}"
    );

    write(
        &client.path("src/widget.rsx"),
        "pub fn widget() { let _y = <p>; }\n",
    );
    client.watched(&[("src/widget.rsx", WATCH_CHANGED)]);
    let publish = client.wait_for(|m| is_publish_for(m, &widget_uri));
    assert!(!is_empty_publish(&publish), "{publish}");
    client.shutdown();
}

#[test]
fn closing_a_file_deleted_while_open_clears_it_and_a_recreation_is_seen() {
    let mut client = Client::start(true);
    accept_registration(&mut client);
    client.did_open("src/widget.rsx", VALID_WIDGET, 1);
    assert!(client.next_diagnostics("src/widget.rsx").is_empty());

    fs::remove_file(client.path("src/widget.rsx")).unwrap();
    client.watched(&[("src/widget.rsx", WATCH_DELETED)]);
    let uri = client.file_uri("src/widget.rsx");
    client.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );
    let seen = client.sync();
    assert!(
        publishes_for(&seen, &uri)
            .iter()
            .any(|m| is_empty_publish(m)),
        "closing the deleted file must clear its diagnostics: {seen:?}"
    );

    write(&client.path("src/widget.rsx"), BROKEN_WIDGET);
    client.watched(&[("src/widget.rsx", WATCH_CREATED)]);
    let publish = client.wait_for(|m| is_publish_for(m, &uri) && !is_empty_publish(m));
    assert!(!is_empty_publish(&publish));
    client.shutdown();
}

#[test]
fn a_created_rsx_file_outside_the_crate_causes_no_republish() {
    let mut client = Client::start(true);
    accept_registration(&mut client);
    let outside = client
        .root
        .parent()
        .unwrap()
        .join(format!("outou-outside-{}.rsx", std::process::id()));
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes": [{"uri": format!("file://{}", outside.display()), "type": WATCH_CREATED}]}),
    );
    let seen = client.sync();
    let republished = seen.iter().any(|m| {
        is_publish_for(m, &client.file_uri("src/main.rsx"))
            || is_publish_for(m, &client.file_uri("src/widget.rsx"))
    });
    assert!(!republished, "{seen:?}");
    client.shutdown();
}

#[test]
fn a_changed_manifest_triggers_a_replan() {
    let mut client = Client::start(true);
    accept_registration(&mut client);
    client.watched(&[("Cargo.toml", WATCH_CHANGED)]);
    let seen = client.sync();
    assert!(
        !publishes_for(&seen, &client.file_uri("src/main.rsx")).is_empty(),
        "a re-plan republishes main.rsx: {seen:?}"
    );
    client.shutdown();
}

#[test]
fn a_new_file_reached_by_an_existing_mod_is_picked_up() {
    let mut client = Client::start(true);
    accept_registration(&mut client);
    client.did_open("src/main.rsx", "mod widget;\nmod fresh;\nfn main() {}\n", 1);
    client.sync();

    write(&client.path("src/fresh.rsx"), BROKEN_WIDGET);
    client.watched(&[("src/fresh.rsx", WATCH_CREATED)]);
    let uri = client.file_uri("src/fresh.rsx");
    let publish = client.wait_for(|m| is_publish_for(m, &uri) && !is_empty_publish(m));
    assert!(!is_empty_publish(&publish));
    client.shutdown();
}

#[test]
fn a_change_event_for_a_file_that_is_gone_clears_it() {
    let mut client = Client::start(true);
    accept_registration(&mut client);
    fs::remove_file(client.path("src/widget.rsx")).unwrap();
    client.watched(&[("src/widget.rsx", WATCH_CHANGED)]);
    let seen = client.sync();
    let uri = client.file_uri("src/widget.rsx");
    assert!(
        publishes_for(&seen, &uri)
            .iter()
            .any(|m| is_empty_publish(m)),
        "{seen:?}"
    );
    client.shutdown();
}
