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
// Usage: see usage() below, or run with --help.

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
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
  "definition-load-user",
  "definition-user-card",
  "completion-member",
  "completion-component",
  "completion-prop",
  "diagnostic-type-error",
  "diagnostic-syntax-error",
];

function usage() {
  return `outou-lsp-client: headless client for outou-lsp (Gate 3, issue #9)

  node outou-lsp-client.mjs --root <crate dir> --outou-lsp <binary> --probe <name>
                            [--ra <rust-analyzer binary>] [--timeout <ms>] [--settle <ms>]

  --root       crate directory (contains Cargo.toml and src/main.rsx);
               examples/phase0-app is Gate 3's fixed target program.
  --outou-lsp  path to the outou-lsp binary under test.
  --ra         rust-analyzer binary outou-lsp should spawn (defaults to
               OUTOU_RUST_ANALYZER or PATH, same as outou-lsp itself, via
               the OUTOU_RUST_ANALYZER env var set for the child).
  --probe      one of: ${PROBES.join(", ")}
  --settle     ms to wait for diagnostics/indexing to settle (default 12000)
  --timeout    overall ms budget for the whole run (default 60000)

Every probe opens examples/phase0-app's src/main.rsx (some also edit it,
in memory, before requesting), waits --settle ms for rust-analyzer to
index, issues the request(s) the probe is named for, and prints one JSON
document with the result(s), diagnostics collected so far, and per-request
latency in milliseconds (latencyMs), matching ra-client.mjs's shape.`;
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
const settleMs = Number(args.settle ?? 12000);

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
function handle(message) {
  if (message.method === undefined && message.id !== undefined && pending.has(message.id)) {
    const { res, rej } = pending.get(message.id);
    pending.delete(message.id);
    message.error ? rej(message.error) : res(message.result);
  } else if (message.method === "textDocument/publishDiagnostics") {
    diagnostics[message.params.uri] = message.params.diagnostics;
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

// outou-lsp's own readiness depends on the rust-analyzer child it spawned
// indexing the crate; there is no `$/progress` forwarded to us to watch
// (this server does not proxy progress notifications), so — like
// ra-client.mjs's fallback — this simply waits a fixed settle window
// before issuing requests.
await new Promise((r) => setTimeout(r, settleMs));

await runProbe(args.probe);

// Give flycheck a further moment for probes that rely on it.
await new Promise((r) => setTimeout(r, 2000));
result.diagnostics = diagnostics;
finish({});

async function timeRequest(key, method, params) {
  const t = Date.now();
  try {
    result[key] = await request(method, params);
  } catch (err) {
    result[key] = null;
    result[`${key}Error`] = err;
  }
  result.latencyMs[key] = Date.now() - t;
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

async function runProbe(probe) {
  switch (probe) {
    case "hover-user": {
      // `let user = load_user();` — hover anywhere inside `user`.
      const pos = positionOf(originalText, "let user = load_user();");
      await timeRequest("hover", "textDocument/hover", {
        textDocument: { uri: mainUri },
        position: { line: pos.line, character: pos.character + 6 },
      });
      break;
    }
    case "definition-load-user": {
      const pos = positionOf(originalText, "load_user()", 0);
      await timeRequest("definition", "textDocument/definition", {
        textDocument: { uri: mainUri },
        position: { line: pos.line, character: pos.character + 2 },
      });
      break;
    }
    case "definition-user-card": {
      const pos = positionOf(originalText, "<UserCard");
      await timeRequest("definition", "textDocument/definition", {
        textDocument: { uri: mainUri },
        position: { line: pos.line, character: pos.character + 3 },
      });
      break;
    }
    case "completion-member": {
      const text = originalText.replace(
        "let user = load_user();",
        "let user = load_user();\n    user.",
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "user.");
      await timeRequest("completion", "textDocument/completion", {
        textDocument: { uri: mainUri },
        position: { line, character: 9 },
      });
      break;
    }
    case "completion-component": {
      const text = originalText.replace(
        '<Greeting name="Outou" />',
        '<Greeting name="Outou" />\n        <UserC',
      );
      change(2, text);
      const line = text.split("\n").findIndex((l) => l.trim() === "<UserC");
      await timeRequest("completion", "textDocument/completion", {
        textDocument: { uri: mainUri },
        position: { line, character: 14 },
      });
      break;
    }
    case "completion-prop": {
      // A partially-typed prop *value* (`user={us}`), which is where the
      // Week 1 spike's own "prop completion" probe landed too
      // (`spikes/rust-analyzer/README.md`'s prop-completion row): typing
      // `<UserCard us` alone leaves the tag unclosed, which Outou's parser
      // recovers as a broken element (a placeholder call, not a struct
      // literal) rather than something rust-analyzer can complete props
      // against — the tag has to stay well-formed for the value position
      // inside it to reach rust-analyzer as ordinary Rust.
      const text = originalText.replace(
        "<UserCard user={user.unwrap()} />",
        "<UserCard user={us} />",
      );
      change(2, text);
      const lineText = text.split("\n").find((l) => l.includes("user={us}"));
      const line = text.split("\n").indexOf(lineText);
      const character = lineText.indexOf("{us}") + 3;
      await timeRequest("completion", "textDocument/completion", {
        textDocument: { uri: mainUri },
        position: { line, character },
      });
      break;
    }
    case "diagnostic-type-error": {
      const text = originalText.replace(
        "let user = load_user();",
        "let user: u32 = load_user();",
      );
      const saveStart = Date.now();
      change(2, text);
      notify("textDocument/didSave", { textDocument: { uri: mainUri }, text });
      await new Promise((r) => setTimeout(r, 6000));
      result.latencyMs.didSaveToDiagnostics = Date.now() - saveStart;
      break;
    }
    case "diagnostic-syntax-error": {
      const text = originalText.replace("<h1>Hello {name}</h1>", "<div cl");
      const changeStart = Date.now();
      change(2, text);
      await new Promise((r) => setTimeout(r, 1500));
      result.latencyMs.didChangeToDiagnostics = Date.now() - changeStart;
      // The broken element is in `Greeting`, unrelated to `load_user` in
      // `App` — definition there must still work (recovery keeps the
      // rest of the file analyzable).
      const pos = positionOf(text, "load_user()", 0);
      await timeRequest("definitionDespiteSyntaxError", "textDocument/definition", {
        textDocument: { uri: mainUri },
        position: { line: pos.line, character: pos.character + 2 },
      });
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
