#!/usr/bin/env node
// Gate 3 (issue #9 Gate 3 review, S8): honestly measures `outou-lsp`'s own
// proxy overhead, in the same warm session, rather than repeating
// `docs/phase0.md`'s provisional "not perceptible" budget line without
// ever having measured it. Two long-lived sessions are driven against the
// same generated file with the same rust-analyzer configuration
// (`checkOnSave`/`cargo.buildScripts.enable`, forced `utf-16` positions):
//
//   - through `outou-lsp` itself, on `src/main.rsx` (its own positions) —
//     the path a real editor takes;
//   - directly against rust-analyzer, on the already-generated
//     `src/.generated/crate-root.rs` (the exact same file `outou-lsp`
//     opens as its overlay) — no proxy in between.
//
// Both sides warm up first (a hover request retried until it resolves
// usefully, i.e. rust-analyzer has actually finished indexing this cold
// crate), then issue N identical hover/definition/completion requests
// sequentially and record each one's real round-trip latency. The
// reported overhead is the *difference*, not either side's own latency
// number, since a cold vs. warm indexing state would otherwise dominate
// both and hide the one number this exists to measure.
//
// Usage:
//
//   node gate3-overhead.mjs --root <crate dir> --outou-lsp <binary> --ra <binary> [--n 10] [--timeout 120000]
//
// `--root` must already contain a built crate: `Cargo.toml`, `src/main.rsx`,
// and `src/.generated/` from a prior `outou build --manifest-dir <root>`
// (`cargo run -p outou-cli -- build --manifest-dir <root>`) — this script
// only measures, it does not plan or generate. A fresh temporary copy of
// `examples/phase0-app` (never the repository tree itself, matching
// `crates/outou-lsp/tests/gate3.rs`'s own `fresh_copy`/`seed_build`) is the
// expected input; see that test's module doc comment for why.
//
// Prints one JSON document to stdout: `{ operation: { outouLsp: { samples,
// medianMs, p95Ms }, direct: { samples, medianMs, p95Ms }, overheadMs:
// { median, p95 } } }` for `hover`, `definition` and `completion`, plus a
// pre-formatted Markdown table on stderr for pasting into
// `docs/gate3-results.md` (mirroring `gate3-latency-table.mjs`'s own
// convention of being the one place that table is generated from).

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

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
    out[name] = argv[++i];
  }
  return out;
}

function usage() {
  return `gate3-overhead: measures outou-lsp's own proxy overhead against rust-analyzer directly (issue #9 Gate 3 review, S8)

  node gate3-overhead.mjs --root <crate dir> --outou-lsp <binary> --ra <binary> [--n 10] [--timeout 120000]

  --root       a crate directory already built with \`outou build\` (contains
               Cargo.toml, src/main.rsx and src/.generated/) — always a
               fresh temp copy of examples/phase0-app, never the repository
               tree itself.
  --outou-lsp  path to the outou-lsp binary under test.
  --ra         rust-analyzer binary to drive, both through outou-lsp
               (via OUTOU_RUST_ANALYZER) and directly.
  --n          number of repeated requests per operation, after warm-up
               (default 10).
  --timeout    per-warm-up ms budget (default 120000; cold cargo check on
               a full dependency graph can take minutes).`;
}

const args = parseArgs(process.argv.slice(2));
if (args.help || !args.root || !args["outou-lsp"] || !args.ra) {
  console.log(usage());
  process.exit(args.help ? 0 : 2);
}

const root = resolve(args.root);
const n = Number(args.n ?? 10);
const timeoutMs = Number(args.timeout ?? 120000);

/// A minimal JSON-RPC-over-stdio client, reused for both the `outou-lsp`
/// and the direct rust-analyzer sessions — both speak the identical
/// Content-Length framing (`outou-lsp-client.mjs`/`ra-client.mjs`'s own
/// framing code, factored out here since this script drives two
/// long-lived sessions side by side rather than one).
class LspProcess {
  constructor(command, procArgs, env) {
    this.child = spawn(command, procArgs, { stdio: ["pipe", "pipe", "inherit"], env });
    this.buffer = Buffer.alloc(0);
    this.pending = new Map();
    this.nextId = 1;
    this.child.stdout.on("data", (chunk) => this.onData(chunk));
  }

  onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const length = Number(
        /Content-Length: (\d+)/.exec(this.buffer.subarray(0, headerEnd).toString())[1],
      );
      if (this.buffer.length < headerEnd + 4 + length) return;
      const message = JSON.parse(this.buffer.subarray(headerEnd + 4, headerEnd + 4 + length).toString());
      this.buffer = this.buffer.subarray(headerEnd + 4 + length);
      this.handle(message);
    }
  }

  send(message) {
    const body = JSON.stringify({ jsonrpc: "2.0", ...message });
    this.child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
  }

  /// Sends a request and resolves with `{ result, elapsedMs }` —
  /// `elapsedMs` is this call's own real round-trip time, the thing every
  /// measurement in this script actually reads.
  request(method, params) {
    const id = this.nextId++;
    const start = Date.now();
    this.send({ id, method, params });
    return new Promise((res, rej) => this.pending.set(id, { res, rej, start }));
  }

  notify(method, params) {
    this.send({ method, params });
  }

  handle(message) {
    if (message.method === undefined && message.id !== undefined && this.pending.has(message.id)) {
      const { res, rej, start } = this.pending.get(message.id);
      this.pending.delete(message.id);
      const elapsedMs = Date.now() - start;
      message.error ? rej(message.error) : res({ result: message.result, elapsedMs });
    } else if (message.method !== undefined && message.id !== undefined) {
      // A request from the server (`window/workDoneProgress/create`,
      // `client/registerCapability`, ...): both sessions advertise
      // `window.workDoneProgress` below, so answer generically with
      // `null` rather than hanging the server waiting for a reply.
      this.send({ id: message.id, result: null });
    }
    // Notifications ($/progress, publishDiagnostics, ...) are not needed
    // by this script: readiness here is "the answer itself looks right",
    // exactly like `outou-lsp-client.mjs`'s own `requestUntilReady`.
  }

  /// Sends `shutdown`/`exit`, per LSP's own required teardown sequence,
  /// before killing the process — skipping it (a bare `SIGTERM`) makes
  /// both `outou-lsp` and rust-analyzer log an alarming, harmless
  /// "client exited without proper shutdown sequence" panic to stderr on
  /// every run of this script.
  async shutdown() {
    try {
      await Promise.race([
        this.request("shutdown", null),
        new Promise((res) => setTimeout(res, 2000)),
      ]);
      this.notify("exit", null);
      await new Promise((res) => setTimeout(res, 200));
    } catch {
      // Falls through to kill() below regardless.
    }
    this.kill();
  }

  kill() {
    try {
      this.child.kill();
    } catch {
      // already exited
    }
  }
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

function isUsefulHover(value) {
  if (value === null || value === undefined) return false;
  const text = JSON.stringify(value.contents ?? value);
  return !text.includes("{unknown}") && !text.includes("{error}");
}

function isUsefulDefinition(value) {
  if (value === null || value === undefined) return false;
  return Array.isArray(value) ? value.length > 0 : true;
}

function hasCompletionItems(value) {
  const items = Array.isArray(value) ? value : value?.items;
  return Array.isArray(items) && items.length > 0;
}

/// Retries `fn` (a zero-arg function returning the `{result, elapsedMs}`
/// promise a `request()` call gives) until `isReady(result)` accepts one,
/// or `timeoutMsLocal` elapses. Used only for warm-up: every *measured*
/// request below is sent exactly once, with no retry, so its own
/// `elapsedMs` is the honest cost of one round trip once warm.
async function warmUp(fn, isReady, timeoutMsLocal) {
  const start = Date.now();
  let lastError = null;
  while (Date.now() - start < timeoutMsLocal) {
    try {
      const { result } = await fn();
      if (isReady(result)) return;
    } catch (err) {
      lastError = err;
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(
    `warmUp: timed out after ${timeoutMsLocal}ms${lastError ? ` (last error: ${JSON.stringify(lastError)})` : ""}`,
  );
}

async function measureN(times, fn) {
  const samples = [];
  for (let i = 0; i < times; i++) {
    const { elapsedMs } = await fn();
    samples.push(elapsedMs);
  }
  return samples;
}

function median(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/// Nearest-rank p95. Honest caveat, not hidden: with `n` around 10 this
/// is close to (often exactly) the sample maximum — reported anyway,
/// since that is still a real measured number, not a guess, and the raw
/// `samples` array is included so a reader can judge the spread directly
/// rather than trust one summary statistic computed from very few points.
function p95(samples) {
  const sorted = [...samples].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.ceil(0.95 * sorted.length) - 1);
  return sorted[idx];
}

const RA_INIT_OPTIONS = {
  checkOnSave: true,
  cargo: { buildScripts: { enable: true } },
};

/// Common capabilities both sessions send: `general.positionEncodings`
/// pinned to `utf-16` (matching `outou-lsp`'s own
/// `force_utf16_position_encoding`) so both sides are asking rust-analyzer
/// to do the same work, and `window.workDoneProgress` so a
/// `window/workDoneProgress/create` request never hangs either process
/// waiting for an answer this script never sends otherwise.
function capabilities() {
  return {
    textDocument: {
      publishDiagnostics: {},
      hover: {},
      definition: {},
      completion: { completionItem: { snippetSupport: false } },
      synchronization: { didSave: true },
    },
    workspace: {},
    window: { workDoneProgress: true },
    general: { positionEncodings: ["utf-16"] },
  };
}

/// Session 1: through `outou-lsp`, on `src/main.rsx`'s own positions —
/// the path a real editor takes.
async function measureThroughOutouLsp() {
  const mainRsxPath = resolve(root, "src/main.rsx");
  const mainUri = pathToFileURL(mainRsxPath).href;
  const originalText = readFileSync(mainRsxPath, "utf8");

  const env = { ...process.env, OUTOU_RUST_ANALYZER: resolve(args.ra) };
  const proc = new LspProcess(args["outou-lsp"], [], env);
  try {
    await proc.request("initialize", {
      processId: process.pid,
      capabilities: capabilities(),
      workspaceFolders: [{ uri: pathToFileURL(root).href, name: "gate3-overhead" }],
      initializationOptions: {},
    });
    proc.notify("initialized", {});
    proc.notify("textDocument/didOpen", {
      textDocument: { uri: mainUri, languageId: "outou-rsx", version: 1, text: originalText },
    });

    const hoverPos = positionOf(originalText, "let user = load_user();");
    const hoverParams = {
      textDocument: { uri: mainUri },
      position: { line: hoverPos.line, character: hoverPos.character + 6 },
    };
    const definitionPos = positionOf(originalText, "load_user(", 0);
    const definitionParams = {
      textDocument: { uri: mainUri },
      position: { line: definitionPos.line, character: definitionPos.character + 2 },
    };

    await warmUp(() => proc.request("textDocument/hover", hoverParams), isUsefulHover, timeoutMs);
    const hover = await measureN(n, () => proc.request("textDocument/hover", hoverParams));
    const definition = await measureN(n, () => proc.request("textDocument/definition", definitionParams));

    const completionText = originalText.replace(
      "let user = load_user();",
      "let user = load_user();\n    user.",
    );
    proc.notify("textDocument/didChange", {
      textDocument: { uri: mainUri, version: 2 },
      contentChanges: [{ text: completionText }],
    });
    const completionLine = completionText.split("\n").findIndex((l) => l.trim() === "user.");
    const completionParams = {
      textDocument: { uri: mainUri },
      position: { line: completionLine, character: 9 },
    };
    await warmUp(
      () => proc.request("textDocument/completion", completionParams),
      hasCompletionItems,
      timeoutMs,
    );
    const completion = await measureN(n, () => proc.request("textDocument/completion", completionParams));

    return { hover, definition, completion };
  } finally {
    await proc.shutdown();
  }
}

/// Session 2: directly against rust-analyzer, on the already-generated
/// `src/.generated/crate-root.rs` — the exact file `outou-lsp` opens as
/// its own overlay for this same crate, minus the proxy.
async function measureDirect() {
  const generatedPath = resolve(root, "src/.generated/crate-root.rs");
  const generatedUri = pathToFileURL(generatedPath).href;
  const originalText = readFileSync(generatedPath, "utf8");

  const proc = new LspProcess(args.ra, [], process.env);
  try {
    await proc.request("initialize", {
      processId: process.pid,
      rootUri: pathToFileURL(root).href,
      capabilities: capabilities(),
      initializationOptions: RA_INIT_OPTIONS,
    });
    proc.notify("initialized", {});
    proc.notify("textDocument/didOpen", {
      textDocument: { uri: generatedUri, languageId: "rust", version: 1, text: originalText },
    });

    const hoverPos = positionOf(originalText, "let user = load_user();");
    const hoverParams = {
      textDocument: { uri: generatedUri },
      position: { line: hoverPos.line, character: hoverPos.character + 6 },
    };
    const definitionPos = positionOf(originalText, "load_user(", 0);
    const definitionParams = {
      textDocument: { uri: generatedUri },
      position: { line: definitionPos.line, character: definitionPos.character + 2 },
    };

    await warmUp(() => proc.request("textDocument/hover", hoverParams), isUsefulHover, timeoutMs);
    const hover = await measureN(n, () => proc.request("textDocument/hover", hoverParams));
    const definition = await measureN(n, () => proc.request("textDocument/definition", definitionParams));

    const completionText = originalText.replace(
      "let user = load_user();",
      "let user = load_user();\n    user.",
    );
    proc.notify("textDocument/didChange", {
      textDocument: { uri: generatedUri, version: 2 },
      contentChanges: [{ text: completionText }],
    });
    const completionLine = completionText.split("\n").findIndex((l) => l.trim() === "user.");
    const completionParams = {
      textDocument: { uri: generatedUri },
      position: { line: completionLine, character: 9 },
    };
    await warmUp(
      () => proc.request("textDocument/completion", completionParams),
      hasCompletionItems,
      timeoutMs,
    );
    const completion = await measureN(n, () => proc.request("textDocument/completion", completionParams));

    return { hover, definition, completion };
  } finally {
    await proc.shutdown();
  }
}

const outouLsp = await measureThroughOutouLsp();
const direct = await measureDirect();

const operations = ["hover", "definition", "completion"];
const report = {};
let table = "| Operation | outou-lsp median (ms) | outou-lsp p95 (ms) | direct median (ms) | direct p95 (ms) | overhead, median (ms) | overhead, p95 (ms) |\n";
table += "|---|---|---|---|---|---|---|\n";
for (const op of operations) {
  const lspSamples = outouLsp[op];
  const directSamples = direct[op];
  const lspMedian = median(lspSamples);
  const lspP95 = p95(lspSamples);
  const directMedian = median(directSamples);
  const directP95 = p95(directSamples);
  report[op] = {
    outouLsp: { samples: lspSamples, medianMs: lspMedian, p95Ms: lspP95 },
    direct: { samples: directSamples, medianMs: directMedian, p95Ms: directP95 },
    overheadMs: { median: lspMedian - directMedian, p95: lspP95 - directP95 },
  };
  table += `| \`${op}\` | ${lspMedian} | ${lspP95} | ${directMedian} | ${directP95} | ${(lspMedian - directMedian).toFixed(1)} | ${(lspP95 - directP95).toFixed(1)} |\n`;
}

console.log(JSON.stringify({ n, root, ...report }, null, 2));
console.error(`\nn=${n} samples per operation, per session:\n\n${table}`);
