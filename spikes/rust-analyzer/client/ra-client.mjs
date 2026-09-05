#!/usr/bin/env node
// Headless LSP client for the rust-analyzer feasibility spike.
//
// Speaks JSON-RPC over stdio to rust-analyzer, opens a generated Rust file
// with *overlay* content (the editor buffer), and asks for completion, hover
// and definition at a position. Diagnostics are collected until rust-analyzer
// reports that its initial load and flycheck are done.
//
// Usage:
//   node ra-client.mjs --root <fixture dir> --file <generated .rs> \
//        [--overlay <file with buffer contents>] --line N --char M \
//        [--ra rust-analyzer] [--timeout ms]
//
// Positions are 0-based, like LSP. Output is one JSON document on stdout.

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

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
function handle(message) {
  if (message.id !== undefined && pending.has(message.id)) {
    const { res, rej } = pending.get(message.id);
    pending.delete(message.id);
    message.error ? rej(message.error) : res(message.result);
  } else if (message.method === "textDocument/publishDiagnostics") {
    diagnostics[message.params.uri] = message.params.diagnostics;
  } else if (message.method === "window/workDoneProgress/create") {
    send({ id: message.id, result: null });
  } else if (message.method === "client/registerCapability") {
    send({ id: message.id, result: null });
  }
}

const started = Date.now();
const timer = setTimeout(() => finish({ error: `timeout after ${timeoutMs} ms` }), timeoutMs);

await request("initialize", {
  processId: process.pid,
  rootUri: pathToFileURL(root).href,
  capabilities: { textDocument: { publishDiagnostics: {} }, workspace: {} },
  initializationOptions: { checkOnSave: true, cargo: { buildScripts: { enable: true } } },
});
notify("initialized", {});
notify("textDocument/didOpen", { textDocument: { uri, languageId: "rust", version: 1, text } });

// rust-analyzer answers before the crate graph is loaded; wait a little and
// retry until hover returns something or the timeout hits.
const result = { file: uri, position, latencyMs: {} };
for (let attempt = 0; attempt < 30; attempt++) {
  const t = Date.now();
  result.hover = await request("textDocument/hover", { textDocument: { uri }, position });
  result.latencyMs.hover = Date.now() - t;
  if (result.hover) break;
  await new Promise((r) => setTimeout(r, 1000));
}
timeAndStore("completion", "textDocument/completion", { textDocument: { uri }, position });
timeAndStore("definition", "textDocument/definition", { textDocument: { uri }, position });
await Promise.all(result.pending ?? []);
// Give flycheck a moment to publish diagnostics.
await new Promise((r) => setTimeout(r, Number(args.settle ?? 5000)));
finish({});

function timeAndStore(key, method, params) {
  const t = Date.now();
  result.pending = result.pending ?? [];
  result.pending.push(
    request(method, params).then((r) => {
      result[key] = r;
      result.latencyMs[key] = Date.now() - t;
    }),
  );
}
function finish(extra) {
  clearTimeout(timer);
  delete result.pending;
  result.diagnostics = diagnostics;
  result.totalMs = Date.now() - started;
  console.log(JSON.stringify({ ...result, ...extra }, null, 2));
  try {
    request("shutdown", null).finally(() => {
      notify("exit", null);
      ra.kill();
    });
  } catch {
    ra.kill();
  }
  setTimeout(() => process.exit(extra.error ? 1 : 0), 500);
}
function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--help" || a === "-h") out.help = true;
    else if (a.startsWith("--")) out[a.slice(2)] = argv[++i];
  }
  return out;
}
function usage() {
  return `ra-client: headless rust-analyzer client for the Outou Week 1 spike

  node ra-client.mjs --root <fixture dir> --file <generated .rs> --line N --char M
                     [--overlay <buffer file>] [--ra <rust-analyzer binary>]
                     [--timeout <ms>] [--settle <ms>]

  --file     path of the generated Rust file rust-analyzer should see
             (src/.generated/App.rs for variant (b); the OUT_DIR path for (a))
  --overlay  file whose contents are sent as the open buffer instead of
             what is on disk. This is the "editor overlay" experiment.
  --line/--char  0-based cursor position for hover/completion/definition

Prints one JSON document with hover, completion, definition, diagnostics
and per-request latency in milliseconds.`;
}
