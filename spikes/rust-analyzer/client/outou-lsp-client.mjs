#!/usr/bin/env node
// Headless LSP client for Gate 3 (issue #9): drives `outou-lsp` itself,
// rather than rust-analyzer directly (that is what `ra-client.mjs` does,
// for the Week 1 spike). The two servers speak different shapes of the
// same protocol — `outou-lsp` wants `workspaceFolders` (not `rootUri`),
// `.rsx` documents with language id `outou-rsx`, and positions in the
// `.rsx` file itself, never the generated Rust — so this is a sibling
// script rather than a `ra-client.mjs` flag, reusing the same
// Content-Length framing and `$/progress` readiness pattern.
//
// Gate 3's target program is fixed (docs/phase0.md, examples/phase0-app),
// so each probe is a named scenario against that program's `src/main.rsx`
// rather than a generic --line/--char pair: most probes need to *edit*
// the buffer first (an incomplete tag, a type error) and know where the
// edit landed, which is easier to keep correct as JavaScript than to
// re-derive from raw line/column flags on every invocation.
//
// `outou-lsp` does not forward rust-analyzer's own `$/progress`
// notifications to its client (issue #9 Gate 3 review, S6 — still a
// TODO(phase0)), so this client has no readiness signal to watch besides
// the requests it actually cares about. Rather than a single fixed
// "settle" sleep before every request (the Week 1 spike's own
// last-resort fallback, and what this script used to do
// unconditionally), `requestUntilReady` below retries the *specific*
// request with a short interval until it gets an answer that looks ready
// or a bounded timeout elapses — usually much faster than a fixed sleep
// once rust-analyzer is warm, and just as safe when it is not.
//
// Usage: see usage() below, or run with --help.

import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const BOOLEAN_FLAGS = new Set(["help"]);
function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "-h") {
      out.help = true;
      continue;
    }
    if (!a.startsWith("--")) continue;
    const name = a.slice(2);
    out[name] = BOOLEAN_FLAGS.has(name) ? true : argv[++i];
  }
  return out;
}

const PROBES = [
  "hover-user",
  "hover-nonascii",
  "hover-element-tag",
  "definition-load-user",
  "definition-user-card",
  "completion-member",
  "completion-tag-component",
  "completion-tag-element",
  "completion-prop-name",
  "completion-attr-value",
  "completion-prop-value",
  "diagnostic-type-error",
  "diagnostic-syntax-error",
  "diagnostic-missing-prop",
  "stale-diagnostics-cleared",
  "save-with-syntax-error",
  "startup-broken-source",
];

function usage() {
  return `outou-lsp-client: headless client for outou-lsp (Gate 3, issue #9)

  node outou-lsp-client.mjs --root <crate dir> --outou-lsp <binary> --probe <name>
                            [--ra <rust-analyzer binary>] [--timeout <ms>]

  --root       crate directory (contains Cargo.toml and src/main.rsx) —
               always a fresh temp copy of examples/phase0-app for Gate 3
               (never the repository tree itself: some probes save a
               deliberately broken buffer to disk).
  --outou-lsp  path to the outou-lsp binary under test.
  --ra         rust-analyzer binary outou-lsp should spawn (defaults to
               OUTOU_RUST_ANALYZER or PATH, same as outou-lsp itself, via
               the OUTOU_RUST_ANALYZER env var set for the child).
  --probe      one of: ${PROBES.join(", ")}
  --timeout    overall ms budget for the whole run (default 60000)

Every probe opens <root>/src/main.rsx (some also edit and/or save it),
waits for the specific answer it needs (bounded retries/polling, not a
fixed sleep — see the module doc comment), issues the request(s) the
probe is named for, and prints one JSON document with the result(s),
diagnostics collected so far, and per-request latency in milliseconds
(latencyMs) — latency here is real: either a plain request/response round
trip, or time-to-first-matching-diagnostic, never a sleep duration.`;
}

const args = parseArgs(process.argv.slice(2));
if (args.help || !args.root || !args["outou-lsp"] || !PROBES.includes(args.probe)) {
  console.log(usage());
  process.exit(args.help ? 0 : 2);
}

const root = resolve(args.root);
const mainRsxPath = resolve(root, "src/main.rsx");
const mainUri = pathToFileURL(mainRsxPath).href;
const originalText = readFileSync(mainRsxPath, "utf8");
const timeoutMs = Number(args.timeout ?? 60000);

const env = { ...process.env };
if (args.ra) env.OUTOU_RUST_ANALYZER = resolve(args.ra);

const server = spawn(args["outou-lsp"], [], { stdio: ["pipe", "pipe", "inherit"], env });
const diagnostics = {};
const pending = new Map();
let nextId = 1;
let buffer = Buffer.alloc(0);

server.stdout.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd < 0) return;
    const length = Number(/Content-Length: (\d+)/.exec(buffer.subarray(0, headerEnd).toString())[1]);
    if (buffer.length < headerEnd + 4 + length) return;
    const message = JSON.parse(buffer.subarray(headerEnd + 4, headerEnd + 4 + length).toString());
    buffer = buffer.subarray(headerEnd + 4 + length);
    handle(message);
  }
});
function send(message) {
  const body = JSON.stringify({ jsonrpc: "2.0", ...message });
  server.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}
function request(method, params) {
  const id = nextId++;
  send({ id, method, params });
  return new Promise((res, rej) => pending.set(id, { res, rej }));
}
function notify(method, params) {
  send({ method, params });
}
// Last time each uri's diagnostics were (re)published, used by
// `waitForQuiescence` below: rust-analyzer's `checkOnSave` flycheck can
// publish more than once for a single save (an initial pass, then a
// fuller one with cascading errors), so "the diagnostic I'm waiting for
// showed up" does not mean "no more publishes are coming" — a probe that
// reverts the buffer right after the first sighting can still race a
// second, late publish for the *pre-revert* content.
const lastDiagnosticsUpdate = {};
function handle(message) {
  if (message.method === undefined && message.id !== undefined && pending.has(message.id)) {
    const { res, rej } = pending.get(message.id);
    pending.delete(message.id);
    message.error ? rej(message.error) : res(message.result);
  } else if (message.method === "textDocument/publishDiagnostics") {
    diagnostics[message.params.uri] = message.params.diagnostics;
    lastDiagnosticsUpdate[message.params.uri] = Date.now();
  } else if (message.method !== undefined && message.id !== undefined) {
    // A request from the server (there are none outou-lsp sends today,
    // but answer generically rather than silently hanging it forever).
    send({ id: message.id, result: null });
  }
}

const started = Date.now();
let finished = false;
const timer = setTimeout(() => finish({ error: `timeout after ${timeoutMs} ms` }), timeoutMs);
const result = { probe: args.probe, file: mainUri, latencyMs: {} };

server.on("error", (e) => finish({ error: `failed to spawn outou-lsp: ${e.message}` }));
server.on("exit", (code, sig) => {
  for (const { rej } of pending.values()) {
    rej({ code: -32000, message: `outou-lsp exited (code ${code}, signal ${sig})` });
  }
  pending.clear();
  if (!finished) finish({ error: `outou-lsp exited (code ${code}, signal ${sig})` });
});

const initStart = Date.now();
const initResult = await request("initialize", {
  processId: process.pid,
  capabilities: {
    textDocument: {
      publishDiagnostics: {},
      hover: {},
      definition: {},
      completion: { completionItem: { snippetSupport: false } },
      synchronization: { didSave: true },
    },
    workspace: {},
  },
  workspaceFolders: [{ uri: pathToFileURL(root).href, name: "gate3" }],
  initializationOptions: {},
});
result.serverInfo = initResult?.serverInfo ?? null;
result.latencyMs.initialize = Date.now() - initStart;
notify("initialized", {});
notify("textDocument/didOpen", {
  textDocument: { uri: mainUri, languageId: "outou-rsx", version: 1, text: originalText },
});

await runProbe(args.probe);

result.diagnostics = diagnostics;
finish({});

/// Waits for a request to come back with an answer that `isReady`
/// accepts, retrying every `intervalMs` up to `timeoutMs` total. Records
/// the *real* elapsed time as `latencyMs[key]` — not a sleep duration —
/// so a warm rust-analyzer answering on the first try is reported
/// honestly fast, and a cold one is reported honestly slow, rather than
/// both being hidden behind one fixed wait (issue #9 Gate 3 review,
/// M8(ii)/MEDIUM-12).
async function requestUntilReady(key, method, params, isReady, opts = {}) {
  const timeoutMsLocal = opts.timeoutMs ?? 30000;
  const intervalMs = opts.intervalMs ?? 400;
  const start = Date.now();
  let lastValue = null;
  let lastError = null;
  while (Date.now() - start < timeoutMsLocal) {
    try {
      lastValue = await request(method, params);
      lastError = null;
      if (isReady(lastValue)) {
        result[key] = lastValue;
        result.latencyMs[key] = Date.now() - start;
        return lastValue;
      }
    } catch (err) {
      lastError = err;
    }
    await new Promise((r) => setTimeout(r, intervalMs));
  }
  result[key] = lastValue;
  if (lastError) result[`${key}Error`] = lastError;
  result.latencyMs[key] = Date.now() - start;
  return lastValue;
}

/// Waits for a diagnostic matching `predicate` to appear for `uri`,
/// polling the notifications already collected in `diagnostics` (updated
/// asynchronously as `textDocument/publishDiagnostics` notifications
/// arrive) rather than sleeping a fixed duration. Returns the matching
/// diagnostic (or `null` on timeout) and records the real elapsed time.
async function waitForDiagnostic(key, uri, predicate, timeoutMsLocal = 20000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMsLocal) {
    const match = (diagnostics[uri] ?? []).find(predicate);
    if (match) {
      result.latencyMs[key] = Date.now() - start;
      return match;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  result.latencyMs[key] = Date.now() - start;
  return null;
}

/// Waits until `uri` has gone at least `quietMs` with no new
/// `publishDiagnostics` notification, up to `timeoutMsLocal` total.
/// rust-analyzer's flycheck can publish more than once for one save (an
/// initial pass, then a fuller one with cascading errors); a probe that
/// only waits for the *first* matching diagnostic before moving on can
/// race a second, late publish for content it has already moved past.
async function waitForQuiescence(uri, quietMs = 1500, timeoutMsLocal = 20000) {
  const start = Date.now();
  // If nothing has published yet, treat "now" as the last update so a
  // probe that calls this before any diagnostic exists doesn't return
  // immediately.
  if (lastDiagnosticsUpdate[uri] === undefined) lastDiagnosticsUpdate[uri] = Date.now();
  while (Date.now() - start < timeoutMsLocal) {
    const sinceUpdate = Date.now() - lastDiagnosticsUpdate[uri];
    if (sinceUpdate >= quietMs) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
}

function positionOf(text, needle, occurrence = 0) {
  const lines = text.split("\n");
  let count = 0;
  for (let l = 0; l < lines.length; l++) {
    let idx = -1;
    while ((idx = lines[l].indexOf(needle, idx + 1)) !== -1) {
      if (count === occurrence) return { line: l, character: idx };
      count++;
    }
  }
  throw new Error(`positionOf: ${JSON.stringify(needle)} not found`);
}

function change(version, text) {
  notify("textDocument/didChange", {
    textDocument: { uri: mainUri, version },
    contentChanges: [{ text }],
  });
}

/// Writes `text` to `main.rsx` on disk and then sends `didSave` — in
/// that order, matching what a real editor does (LSP's `didSave` is a
/// notification that the file *has already been written*; the protocol
/// itself never writes the file, the editor does). This test client
/// used to send `didSave` without ever touching disk, which happened to
/// be invisible while `crate::dispatch::handle_rsx_save` wrote from its
/// own in-memory buffer text — but M1's fix (issue #9 Gate 3 review)
/// re-plans and re-emits *from disk* on save, exactly like a real
/// `outou build` would, so a probe that never writes the buffer to disk
/// was silently testing against the file's stale, previous contents.
function saveToDisk(version, text) {
  writeFileSync(mainRsxPath, text);
  change(version, text);
  notify("textDocument/didSave", { textDocument: { uri: mainUri }, text });
}

function hasNonNullResult(value) {
  return value !== null && value !== undefined;
}

/// A hover is only "ready" once rust-analyzer has actually indexed the
/// crate: a non-`null` hover with `{unknown}`/`{error}` in its contents
/// is rust-analyzer answering *before* indexing has caught up, not a
/// useful result — accepting it as "ready" (as a bare `!== null` check
/// would) made this client stop retrying on the first low-quality
/// answer instead of waiting for the real one.
function isUsefulHover(value) {
  if (!hasNonNullResult(value)) return false;
  const text = JSON.stringify(value.contents ?? value);
  return !text.includes("{unknown}") && !text.includes("{error}");
}

/// Likewise, an empty `[]` (or `null`) definition result is what
/// rust-analyzer answers before indexing has found anything, not a
/// meaningful "no definition" — Gate 3's probes always have a real
/// definition to find.
function isUsefulDefinition(value) {
  if (!hasNonNullResult(value)) return false;
  return Array.isArray(value) ? value.length > 0 : true;
}

function hasCompletionItems(value) {
  const items = Array.isArray(value) ? value : value?.items;
  return Array.isArray(items) && items.length > 0;
}

async function runProbe(probe) {
  switch (probe) {
    case "hover-user": {
      const pos = positionOf(originalText, "let user = load_user();");
      await requestUntilReady(
        "hover",
        "textDocument/hover",
        { textDocument: { uri: mainUri }, position: { line: pos.line, character: pos.character + 6 } },
        isUsefulHover,
      );
      break;
    }
    case "hover-nonascii": {
      // M7 (issue #9 Gate 3 review, HIGH-6): a preceding non-ASCII
      // literal (each character here is one UTF-16 code unit but three
      // UTF-8 bytes) must not shift the mapped position if — and only
      // if — this server actually forces rust-analyzer onto UTF-16
      // positions end to end, not just claim to in its own math.
      const text = originalText.replace(
        "let user = load_user();",
        'let _u = "あああ"; let user = load_user();',
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.includes('let _u = "あああ"'));
      const character = text.split("\n")[line].indexOf("user") + 1;
      await requestUntilReady(
        "hover",
        "textDocument/hover",
        { textDocument: { uri: mainUri }, position: { line, character } },
        isUsefulHover,
      );
      break;
    }
    case "hover-element-tag": {
      // M4 (issue #9 Gate 3 review, HIGH-9(d)): hovering an HTML element
      // name must not leak `dioxus_html::elements` / `use dioxus::
      // prelude::*;`.
      const pos = positionOf(originalText, '<main class="app">');
      await requestUntilReady(
        "hover",
        "textDocument/hover",
        { textDocument: { uri: mainUri }, position: { line: pos.line, character: pos.character + 1 } },
        () => true,
      );
      break;
    }
    case "definition-load-user": {
      const pos = positionOf(originalText, "load_user()", 0);
      await requestUntilReady(
        "definition",
        "textDocument/definition",
        { textDocument: { uri: mainUri }, position: { line: pos.line, character: pos.character + 2 } },
        isUsefulDefinition,
      );
      break;
    }
    case "definition-user-card": {
      const pos = positionOf(originalText, "<UserCard");
      await requestUntilReady(
        "definition",
        "textDocument/definition",
        { textDocument: { uri: mainUri }, position: { line: pos.line, character: pos.character + 3 } },
        isUsefulDefinition,
      );
      break;
    }
    case "completion-member": {
      const text = originalText.replace(
        "let user = load_user();",
        "let user = load_user();\n    user.",
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "user.");
      await requestUntilReady(
        "completion",
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line, character: 9 } },
        hasCompletionItems,
      );
      break;
    }
    case "completion-tag-component": {
      // M3 (issue #9 Gate 3 review): an Outou-native answer for a
      // partial *component* tag name, never forwarded to rust-analyzer.
      const text = originalText.replace(
        '<Greeting name="Outou" />',
        '<Greeting name="Outou" />\n        <UserC',
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "<UserC");
      await requestUntilReady(
        "completion",
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line, character: 14 } },
        hasCompletionItems,
      );
      break;
    }
    case "completion-tag-element": {
      // M3: same, for a partial *HTML element* tag name.
      const text = originalText.replace(
        '<Greeting name="Outou" />',
        '<Greeting name="Outou" />\n        <di',
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "<di");
      await requestUntilReady(
        "completion",
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line, character: 11 } },
        hasCompletionItems,
      );
      break;
    }
    case "completion-prop-name": {
      // M3: a partially-typed *attribute name* on a known component —
      // the shape `docs/gate3-results.md` previously (incorrectly)
      // documented as unreachable.
      const text = originalText.replace(
        "<UserCard user={user.unwrap()} />",
        "<UserCard us",
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "<UserCard us");
      const character = text.split("\n")[line].indexOf("<UserCard us") + "<UserCard us".length;
      await requestUntilReady(
        "completion",
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line, character } },
        hasCompletionItems,
      );
      break;
    }
    case "completion-attr-value": {
      // M3/M4: an HTML element's attribute *value* position — the worst
      // leak in the original review (a zero-width edit at the wrong
      // column, and `dioxus_*::` module names as labels).
      const text = originalText.replace(
        '<Greeting name="Outou" />',
        '<Greeting name="Outou" />\n        <div class=',
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "<div class=");
      const character = text.split("\n")[line].indexOf("<div class=") + "<div class=".length;
      await requestUntilReady(
        "completion",
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line, character } },
        () => true,
      );
      break;
    }
    case "completion-prop-value": {
      // A partially-typed prop *value* (`user={us}`) — a well-formed
      // tag, so the position reaches rust-analyzer as ordinary Rust
      // (unlike the attribute-*name* shape, which M3 now answers
      // locally).
      const text = originalText.replace(
        "<UserCard user={user.unwrap()} />",
        "<UserCard user={us} />",
      );
      change(2, text);
      const lineText = text.split("\n").find((l) => l.includes("user={us}"));
      const line = text.split("\n").indexOf(lineText);
      const character = lineText.indexOf("{us}") + 3;
      await requestUntilReady(
        "completion",
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line, character } },
        hasCompletionItems,
      );
      break;
    }
    case "diagnostic-type-error": {
      const text = originalText.replace(
        "let user = load_user();",
        "let user: u32 = load_user();",
      );
      saveToDisk(2, text);
      await waitForDiagnostic(
        "didSaveToDiagnostics",
        mainUri,
        (d) => (d.message ?? "").includes("mismatched types"),
        180000, // cold `cargo check` compiling the full dependency graph can take minutes
      );
      break;
    }
    case "diagnostic-syntax-error": {
      const text = originalText.replace("<h1>Hello {name}</h1>", "<div cl");
      change(2, text);
      await waitForDiagnostic(
        "didChangeToDiagnostics",
        mainUri,
        (d) => d.source === "outou",
        10000,
      );
      // The broken element is in `Greeting`, unrelated to `load_user` in
      // `App` — definition there must still work (recovery keeps the
      // rest of the file analyzable).
      const pos = positionOf(text, "load_user()", 0);
      await requestUntilReady(
        "definitionDespiteSyntaxError",
        "textDocument/definition",
        { textDocument: { uri: mainUri }, position: { line: pos.line, character: pos.character + 2 } },
        isUsefulDefinition,
      );
      break;
    }
    case "diagnostic-missing-prop": {
      // M5 (issue #9 Gate 3 review): a hard compile error (a missing
      // required prop) synthesized entirely inside the `rsx!` expansion
      // — no direct source span — must still reach the user as an
      // ERROR, not be dropped or silently downgraded to a hint.
      const text = originalText.replace(
        "<UserCard user={user.unwrap()} />",
        "<UserCard />",
      );
      saveToDisk(2, text);
      await waitForDiagnostic(
        "missingPropToDiagnostics",
        mainUri,
        (d) => (d.message ?? "").includes("missing a required property"),
        180000, // cold `cargo check` compiling the full dependency graph can take minutes
      );
      break;
    }
    case "stale-diagnostics-cleared": {
      // M6: introduce a type error, save, and wait for flycheck to
      // actually report it *and settle* (`waitForQuiescence`) — rust-
      // analyzer's `checkOnSave` can publish more than once for a single
      // save (an initial pass, then a fuller one with cascading errors),
      // so waiting only for the first sighting risks reverting while a
      // second, late publish for the *pre-revert* content is still in
      // flight, which would make this probe flaky rather than test M6 at
      // all. Only once diagnostics have gone quiet is the buffer
      // reverted via `didChange` alone (no save), and the very next
      // `publishDiagnostics` for this file checked for stale rustc
      // diagnostics.
      const broken = originalText.replace(
        "let user = load_user();",
        "let user: u32 = load_user();",
      );
      saveToDisk(2, broken);
      const introduced = await waitForDiagnostic(
        "typeErrorIntroduced",
        mainUri,
        (d) => (d.message ?? "").includes("mismatched types"),
        180000, // cold `cargo check` compiling the full dependency graph can take minutes
      );
      result.typeErrorIntroduced = introduced;
      await waitForQuiescence(mainUri, 2000, 180000);

      change(3, originalText);
      // No new save: rust-analyzer's own native diagnostics never report
      // semantic errors (Week 1 spike finding), so nothing but this
      // server's own `regenerate`-triggered republish is expected here;
      // a short bounded wait is enough for that synchronous path to
      // reach this client over the pipe.
      await new Promise((r) => setTimeout(r, 1000));
      result.diagnosticsAfterRevert = diagnostics[mainUri] ?? [];
      break;
    }
    case "save-with-syntax-error": {
      // M1 (issue #9 Gate 3 review, CRITICAL-1): saving a half-typed
      // buffer must never replace the crate's real `[[bin]]` target with
      // Recovery-mode placeholder text. The filesystem assertion itself
      // (byte-identical `.generated/crate-root.rs` before/after) is made
      // by the Rust test driving this script, against the same temp
      // copy; this probe just performs the edit and save and gives the
      // (non-)write a moment to happen.
      const text = originalText.replace('<h1>Hello {name}</h1>', "<div cl");
      saveToDisk(2, text);
      await waitForDiagnostic(
        "saveWithSyntaxErrorDiagnostics",
        mainUri,
        (d) => d.source === "outou",
        10000,
      );
      await new Promise((r) => setTimeout(r, 500));
      break;
    }
    case "startup-broken-source": {
      // M1: the Rust test driving this script pre-corrupts `src/main.rsx`
      // on disk and removes `.generated/` *before* spawning outou-lsp for
      // this probe, so the `didOpen` above already sent the broken text
      // as it exists on disk. Nothing to edit here — just wait for the
      // startup-time Outou syntax diagnostic and let the Rust test check
      // that no generated file was created.
      await waitForDiagnostic(
        "startupDiagnostics",
        mainUri,
        (d) => d.source === "outou",
        10000,
      );
      break;
    }
    default:
      throw new Error(`unknown probe: ${probe}`);
  }
}

function finish(extra) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  result.totalMs = Date.now() - started;
  console.log(JSON.stringify({ ...result, ...extra }, null, 2));
  try {
    request("shutdown", null)
      .catch(() => {})
      .finally(() => {
        notify("exit", null);
        server.kill();
      });
  } catch {
    server.kill();
  }
  setTimeout(() => process.exit(extra.error ? 1 : 0), 500);
}
