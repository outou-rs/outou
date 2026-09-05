#!/usr/bin/env node
// Headless LSP client for the rust-analyzer feasibility spike.
//
// Speaks JSON-RPC over stdio to rust-analyzer, opens a generated Rust file
// with *overlay* content (the editor buffer), and asks for completion, hover
// and definition at a position. Diagnostics are collected until rust-analyzer
// reports that its initial load and flycheck are done. Optionally sends a
// second overlay (`--overlay2`) via `textDocument/didChange` to exercise the
// "editor buffer, not build.rs" claim, and maps diagnostics back to `App.rsx`
// through a hand-written source map (`--source-map`).
//
// Usage: see usage() below, or run with --help.
//
// Positions are 0-based, like LSP. Output is one JSON document on stdout.

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
import { mapRange } from "./source-map.mjs";

// Boolean flags take no value; everything else consumes the next argv slot.
const BOOLEAN_FLAGS = new Set(["help", "no-default-features"]);
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
function usage() {
  return `ra-client: headless rust-analyzer client for the Outou Week 1 spike

  node ra-client.mjs --root <fixture dir> --file <generated .rs> --line N --char M
                     [--overlay <buffer file>] [--ra <rust-analyzer binary>]
                     [--timeout <ms>] [--settle <ms>]
                     [--cargo-features <a,b,c>] [--no-default-features]
                     [--overlay2 <buffer file>] [--line2 N] [--char2 M]
                     [--source-map <source-map.json>]

  --file               path of the generated Rust file rust-analyzer should
                       see (src/.generated/App.rs for variant (b); the
                       OUT_DIR path for (a))
  --overlay            file whose contents are sent as the open buffer
                       instead of what is on disk ("editor overlay" #1).
  --line/--char        0-based cursor position for hover/completion/definition
  --cargo-features     comma-separated list -> initializationOptions.cargo.features
  --no-default-features  boolean flag -> initializationOptions.cargo.noDefaultFeatures
  --overlay2           after the first hover/completion/definition round,
                       send this file's contents via textDocument/didChange
                       (version 2, full replacement) and re-run
                       hover/completion/definition as hover2/completion2/definition2.
  --line2/--char2      position for the overlay2 round; defaults to --line/--char.
  --source-map         path to the spike's source-map.json; diagnostics for
                       --file are mapped back to source spans as
                       result.mappedDiagnostics.
  --settle             ms to wait for flycheck diagnostics after requests (default 5000)
  --timeout            overall ms budget for the whole run (default 60000)

Readiness: after 'initialized', the client waits for rust-analyzer's
'$/progress' notifications to report the indexing token (rustAnalyzer/
cachePriming, title "Indexing") as ended, recorded as latencyMs.ready. As a
debounced fallback, if every observed progress token has ended and stays
that way for ~2s (in case indexing has not started reporting yet), that also
counts as ready. The hover-retry loop is a bounded fallback for servers that
never report progress. completion/definition are requested once, right
after the first hover attempt succeeds or the retry loop is exhausted, so
they are "warm" relative to hover's retries.

Prints one JSON document with hover, completion, definition, diagnostics,
serverInfo and per-request latency in milliseconds. When --overlay2 or
--source-map are given, hover2/completion2/definition2 and mappedDiagnostics
are added; the base fields are unchanged otherwise.`;
}

const args = parseArgs(process.argv.slice(2));
if (args.help || !args.root || !args.file || args.line === undefined || args.char === undefined) {
  console.log(usage());
  process.exit(args.help ? 0 : 2);
}

const root = resolve(args.root);
const file = resolve(root, args.file);
const text = readFileSync(args.overlay ? resolve(args.overlay) : file, "utf8");
const position = { line: Number(args.line), character: Number(args.char) };
const timeoutMs = Number(args.timeout ?? 60000);
const uri = pathToFileURL(file).href;

const ra = spawn(args.ra ?? "rust-analyzer", [], { stdio: ["pipe", "pipe", "inherit"] });
const diagnostics = {};
const pending = new Map();
let nextId = 1;
let buffer = Buffer.alloc(0);

// --- readiness tracking (`$/progress`) -------------------------------------
// rust-analyzer reports long-running work (indexing, build script fetch,
// flycheck, ...) as work-done progress with fixed token names. Indexing is
// reported under the token `rustAnalyzer/cachePriming` with title "Indexing"
// (rust-analyzer 1.98.1). We consider the server ready once that token
// reports `end`, or, failing that, once every progress token we have seen
// has stayed ended for a short debounce window (other tokens, such as
// `rustAnalyzer/Fetching`, can end within ~0.5s while indexing is still
// running). The bounded hover-retry loop below is the fallback if no
// progress notification arrives at all.
const progressTokens = new Map(); // token -> "begin" | "end"
let indexingToken = null;
let readyAt = null;
let readyResolve;
const readyPromise = new Promise((res) => {
  readyResolve = res;
});
const ALL_ENDED_DEBOUNCE_MS = 2000;
let allEndedTimer = null;

function onProgress({ token, value }) {
  if (!value || (value.kind !== "begin" && value.kind !== "end")) return;
  progressTokens.set(token, value.kind);
  if (value.kind === "begin" && indexingToken === null && (value.title === "Indexing" || String(token).includes("Indexing"))) {
    indexingToken = token;
  }
  checkReady();
}
function checkReady() {
  if (readyAt !== null) return;
  const indexingEnded = indexingToken !== null && progressTokens.get(indexingToken) === "end";
  if (indexingEnded) {
    markReady();
    return;
  }
  // Debounced fallback: every time we observe a progress event, reset a
  // timer; only treat "every known token has ended" as readiness once it
  // has held for ALL_ENDED_DEBOUNCE_MS without a new token starting.
  if (allEndedTimer !== null) {
    clearTimeout(allEndedTimer);
    allEndedTimer = null;
  }
  allEndedTimer = setTimeout(() => {
    allEndedTimer = null;
    if (readyAt !== null) return;
    const allEnded = progressTokens.size > 0 && [...progressTokens.values()].every((s) => s === "end");
    if (allEnded) markReady();
  }, ALL_ENDED_DEBOUNCE_MS);
}
function markReady() {
  if (readyAt !== null) return;
  readyAt = Date.now();
  readyResolve();
}

ra.stdout.on("data", (chunk) => {
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
  ra.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}
function request(method, params) {
  const id = nextId++;
  send({ id, method, params });
  return new Promise((res, rej) => pending.set(id, { res, rej }));
}
function notify(method, params) {
  send({ method, params });
}
// rust-analyzer can transiently reject a request with -32801
// ("ContentModified") right after a didChange, while it is still catching
// up to the new buffer. Retry those instead of treating them as failures.
async function requestRetryingContentModified(method, params, attempts = 5, delayMs = 300, onAttempt) {
  for (let i = 0; ; i++) {
    onAttempt?.(i);
    try {
      return await request(method, params);
    } catch (err) {
      if (err?.code !== -32801 || i >= attempts - 1) throw err;
      await new Promise((r) => setTimeout(r, delayMs));
    }
  }
}
function handle(message) {
  if (message.method === undefined && message.id !== undefined && pending.has(message.id)) {
    const { res, rej } = pending.get(message.id);
    pending.delete(message.id);
    message.error ? rej(message.error) : res(message.result);
  } else if (message.method === "textDocument/publishDiagnostics") {
    diagnostics[message.params.uri] = message.params.diagnostics;
  } else if (message.method === "$/progress") {
    onProgress(message.params);
  } else if (message.method === "window/workDoneProgress/create") {
    send({ id: message.id, result: null });
  } else if (message.method === "client/registerCapability") {
    send({ id: message.id, result: null });
  }
}

const started = Date.now();
let finished = false;
const timer = setTimeout(() => finish({ error: `timeout after ${timeoutMs} ms` }), timeoutMs);
const result = { file: uri, position, latencyMs: {} };

ra.on("error", (e) => finish({ error: `failed to spawn rust-analyzer: ${e.message}` }));
ra.on("exit", (code, sig) => {
  for (const { rej } of pending.values()) rej({ code: -32000, message: `rust-analyzer exited (code ${code}, signal ${sig})` });
  pending.clear();
  finish({ error: `rust-analyzer exited (code ${code}, signal ${sig})` });
});

const cargo = { buildScripts: { enable: true } };
if (args["cargo-features"]) {
  cargo.features = args["cargo-features"]
    .split(",")
    .map((f) => f.trim())
    .filter(Boolean);
}
if (args["no-default-features"]) {
  cargo.noDefaultFeatures = true;
}

const initStart = Date.now();
const initResult = await request("initialize", {
  processId: process.pid,
  rootUri: pathToFileURL(root).href,
  capabilities: {
    textDocument: {
      publishDiagnostics: {},
      synchronization: { dynamicRegistration: false, willSave: false, willSaveWaitUntil: false, didSave: false },
    },
    workspace: {},
    window: { workDoneProgress: true },
  },
  initializationOptions: { checkOnSave: true, cargo },
});
result.serverInfo = initResult?.serverInfo ?? null;
result.textDocumentSyncKind = initResult?.capabilities?.textDocumentSync ?? null;
notify("initialized", {});
notify("textDocument/didOpen", { textDocument: { uri, languageId: "rust", version: 1, text } });

// Wait for rust-analyzer to report it is done loading (bounded), then fall
// back to the hover-retry loop regardless of whether readiness was observed.
const readyTimeoutMs = Math.min(Math.floor(timeoutMs / 2), 30000);
await Promise.race([readyPromise, new Promise((r) => setTimeout(r, readyTimeoutMs))]);
result.ready = readyAt !== null;
result.latencyMs.ready = (readyAt ?? Date.now()) - initStart;

// rust-analyzer can answer before the crate graph is fully loaded (or
// transiently reject a request with "content modified" while it catches up);
// retry hover a bounded number of times as a fallback if readiness was never
// observed (or was a false positive).
for (let attempt = 0; attempt < 30; attempt++) {
  const t = Date.now();
  try {
    result.hover = await request("textDocument/hover", { textDocument: { uri }, position });
    delete result.hoverError;
  } catch (err) {
    result.hover = null;
    result.hoverError = err;
  }
  result.latencyMs.hover = Date.now() - t;
  if (result.hover) {
    result.hoverAttempts = attempt + 1;
    result.latencyMs.didOpenToFirstHover = Date.now() - initStart;
    break;
  }
  await new Promise((r) => setTimeout(r, 1000));
}
result.hoverAttempts ??= 30;
result.latencyMs.didOpenToFirstHover ??= Date.now() - initStart;
const pending1 = [];
timeAndStore("completion", "textDocument/completion", { textDocument: { uri }, position }, pending1);
timeAndStore("definition", "textDocument/definition", { textDocument: { uri }, position }, pending1);
await Promise.all(pending1);

if (args.overlay2) {
  await runOverlay2();
}

// Give flycheck a moment to publish diagnostics.
await new Promise((r) => setTimeout(r, Number(args.settle ?? 5000)));

if (args["source-map"]) {
  result.mappedDiagnostics = mapFileDiagnostics(args["source-map"], diagnostics[uri] ?? []);
}
finish({});

async function runOverlay2() {
  const overlay2Text = readFileSync(resolve(args.overlay2), "utf8");
  const position2 = {
    line: Number(args.line2 ?? args.line),
    character: Number(args.char2 ?? args.char),
  };
  result.position2 = position2;

  const changeSentAt = Date.now();
  // A content change with no `range` is a full-document replacement, valid
  // for both TextDocumentSyncKind.Full and .Incremental.
  notify("textDocument/didChange", {
    textDocument: { uri, version: 2 },
    contentChanges: [{ text: overlay2Text }],
  });
  let hover2Attempts = 0;
  try {
    result.hover2 = await requestRetryingContentModified(
      "textDocument/hover",
      { textDocument: { uri }, position: position2 },
      5,
      300,
      () => hover2Attempts++,
    );
  } catch (err) {
    result.hover2 = null;
    result.hover2Error = err;
  }
  result.hover2Attempts = hover2Attempts;
  result.latencyMs.overlayChangeToHover = Date.now() - changeSentAt;

  const pending2 = [];
  timeAndStore(
    "completion2",
    "textDocument/completion",
    { textDocument: { uri }, position: position2 },
    pending2,
    requestRetryingContentModified,
  );
  timeAndStore(
    "definition2",
    "textDocument/definition",
    { textDocument: { uri }, position: position2 },
    pending2,
    requestRetryingContentModified,
  );
  await Promise.all(pending2);
}

function mapFileDiagnostics(sourceMapPath, fileDiagnostics) {
  const sourceMap = JSON.parse(readFileSync(resolve(sourceMapPath), "utf8"));
  return fileDiagnostics.map((d) => ({
    message: d.message,
    severity: d.severity,
    generated: d.range,
    ...mapRange(sourceMap, d.range),
  }));
}

function timeAndStore(key, method, params, pendingList, requestFn = request) {
  const t = Date.now();
  pendingList.push(
    requestFn(method, params)
      .then((r) => {
        result[key] = r;
      })
      .catch((err) => {
        result[key] = null;
        result[`${key}Error`] = err;
      })
      .finally(() => {
        result.latencyMs[key] = Date.now() - t;
      }),
  );
}
function finish(extra) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  result.diagnostics = diagnostics;
  result.totalMs = Date.now() - started;
  console.log(JSON.stringify({ ...result, ...extra }, null, 2));
  try {
    request("shutdown", null)
      .catch(() => {})
      .finally(() => {
        notify("exit", null);
        ra.kill();
      });
  } catch {
    ra.kill();
  }
  setTimeout(() => process.exit(extra.error ? 1 : 0), 500);
}
