#!/usr/bin/env node
// A1 measurement harness (issue #17, `docs/alpha/issues/17-measurement.md`;
// method described in `docs/alpha.md`, "A1: measurement, in detail").
//
// A single script rather than four, because every mode below shares the
// same JSON-RPC-over-stdio plumbing `outou-lsp-client.mjs`/
// `gate3-overhead.mjs` already established (Content-Length framing,
// `window.workDoneProgress` capability, `publishDiagnostics` tracking) —
// factoring that once here, instead of a fourth near-copy of the same
// `LspProcess` class, is the DRY choice; the four *modes* below stay
// small and single-purpose. `crates/outou-lsp/tests/measure_a1.rs` is the
// orchestrator this script has no opinion about scheduling: it runs
// modes one at a time (never concurrently, except `rss --config
// two-process`'s own two cooperating processes, which is the point of
// that mode) and interprets/records their JSON.
//
// Modes (see each mode's own function doc for its exact protocol):
//
//   coldinit    cold `initialize` -> first usable hover/completion/
//               definition, one fresh process per run.
//   flycheck    `didSave` -> mapped semantic diagnostic latency, N
//               repeated edit/save/revert cycles on a warm session.
//   completion  same-session A/B forwarded-completion overhead
//               (`docs/gate3-results.md`, "Proxy overhead (S8)"),
//               generalized to several cursor positions so the caller can
//               bucket by observed candidate-list size.
//   rss         RSS/CPU sampling of one or two long-lived sessions over a
//               cold-indexing or scripted-editing phase.
//
// Every mode prints exactly one JSON document to stdout and nothing else
// (diagnostics go to stderr), so `measure_a1.rs` can capture and parse
// `child.stdout` directly, matching `run_probe` in `tests/gate3.rs`.
//
// Usage: node measure-a1.mjs --mode <mode> [mode-specific flags]. Run
// `node measure-a1.mjs --mode <mode> --help` for that mode's flags.

import { spawn, spawnSync, execSync, execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { loadavg } from "node:os";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const MODE_NAMES = ["coldinit", "flycheck", "completion", "rss"];
const CONFIG_NAMES = ["single", "two-process"];
// `cold`: sample from spawn until both sessions have settled. `cold+edit`:
// the same cold window, then (in the SAME process, no restart) the scripted
// edits; the result reports the two windows separately.
const PHASE_NAMES = ["cold", "cold+edit"];

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "-h" || a === "--help") {
      out.help = true;
      continue;
    }
    if (!a.startsWith("--")) {
      throw new Error(`unexpected argument ${JSON.stringify(a)}`);
    }
    const name = a.slice(2);
    const value = argv[++i];
    if (value === undefined || value.startsWith("--")) {
      throw new Error(`--${name} requires a value`);
    }
    out[name] = value;
  }
  return out;
}

function requirePositiveInteger(args, name) {
  if (args[name] === undefined) return;
  if (!/^[1-9][0-9]*$/.test(args[name])) {
    throw new Error(`--${name} must be a positive integer, got ${JSON.stringify(args[name])}`);
  }
}

function requireOneOf(args, name, allowed) {
  if (args[name] !== undefined && !allowed.includes(args[name])) {
    throw new Error(`unknown --${name} ${JSON.stringify(args[name])}; expected one of ${allowed.join(", ")}`);
  }
}

/// Validates every flag before anything is spawned, so a typo fails in
/// milliseconds instead of after a cold rust-analyzer start.
function validateArgs(args) {
  requireOneOf(args, "mode", MODE_NAMES);
  requireOneOf(args, "config", CONFIG_NAMES);
  requireOneOf(args, "phase", PHASE_NAMES);
  for (const name of ["n", "timeout", "interval-ms", "edits", "edit-delay-ms"]) {
    requirePositiveInteger(args, name);
  }
  if (args.site !== undefined) {
    if (args.site.trim() === "") throw new Error("--site must be non-empty");
    requireOneOf(args, "site", COMPLETION_SITES.map((s) => s.label));
  }
  for (const name of ["root", "ra"]) {
    if (!args[name]) throw new Error(`--${name} is required`);
  }
  const needsOutouLsp = args.mode !== "rss" || (args.config ?? "single") === "two-process";
  if (needsOutouLsp && !args["outou-lsp"]) throw new Error("--outou-lsp is required");
}

function usage() {
  return `measure-a1: A1 measurement harness (issue #17)

  node measure-a1.mjs --mode coldinit --root <dir> --outou-lsp <bin> --ra <bin> [--n 5] [--timeout 240000]
  node measure-a1.mjs --mode flycheck --root <dir> --outou-lsp <bin> --ra <bin> [--n 10]
  node measure-a1.mjs --mode completion --root <dir> --outou-lsp <bin> --ra <bin> [--n 10] [--site <label>]
  node measure-a1.mjs --mode rss --root <dir> --config <single|two-process> --phase <cold|cold+edit> --ra <bin> [--outou-lsp <bin>] [--interval-ms 500] [--edits 10] [--edit-delay-ms 500]

  --root must already contain a built crate (Cargo.toml, src/main.rsx,
  src/.generated/ from a prior \`outou build\`), matching
  \`crates/outou-lsp/tests/gate3.rs\`'s own fresh_copy/seed_build.`;
}

// ---------------------------------------------------------------------
// Shared JSON-RPC-over-stdio client (framing/capabilities reused from
// gate3-overhead.mjs; diagnostics/progress tracking reused from
// outou-lsp-client.mjs), extended with the pieces this harness needs:
// per-request elapsed time, diagnostics-by-uri with timestamps, and easy
// access to the underlying child's pid for `rss` mode.
// ---------------------------------------------------------------------
const DIAGNOSTIC_POLL_MS = 50;

class LspProcess {
  constructor(command, procArgs, env) {
    this.child = spawn(command, procArgs, { stdio: ["pipe", "pipe", "inherit"], env });
    this.buffer = Buffer.alloc(0);
    this.pending = new Map();
    this.nextId = 1;
    this.diagnostics = {};
    this.lastDiagnosticsUpdate = {};
    // Last `experimental/serverStatus` params (rust-analyzer's own
    // readiness signal; only a direct rust-analyzer session delivers it —
    // `outou-lsp` forwards only publishDiagnostics and `$/progress`).
    this.serverStatus = null;
    this.child.stdout.on("data", (chunk) => this.onData(chunk));
  }

  get pid() {
    return this.child.pid;
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
      const message = JSON.parse(
        this.buffer.subarray(headerEnd + 4, headerEnd + 4 + length).toString(),
      );
      this.buffer = this.buffer.subarray(headerEnd + 4 + length);
      this.handle(message);
    }
  }

  send(message) {
    const body = JSON.stringify({ jsonrpc: "2.0", ...message });
    this.child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
  }

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
    } else if (message.method === "experimental/serverStatus") {
      this.serverStatus = message.params;
    } else if (message.method === "textDocument/publishDiagnostics") {
      this.diagnostics[message.params.uri] = message.params.diagnostics;
      this.lastDiagnosticsUpdate[message.params.uri] = Date.now();
    } else if (message.method !== undefined && message.id !== undefined) {
      this.send({ id: message.id, result: null });
    }
  }

  async waitForDiagnostic(uri, predicate, timeoutMsLocal) {
    const start = Date.now();
    while (Date.now() - start < timeoutMsLocal) {
      const match = (this.diagnostics[uri] ?? []).find(predicate);
      if (match) return Date.now() - start;
      await sleep(DIAGNOSTIC_POLL_MS);
    }
    return null;
  }

  /// Waits (bounded) until no diagnostic for `uri` matches `predicate` —
  /// used after reverting an introduced error, so the *next*
  /// `waitForDiagnostic` call starts from a genuinely clean state rather
  /// than immediately matching a stale diagnostic still sitting in
  /// `this.diagnostics` from before the revert's own publish arrived
  /// (which would otherwise report a false "0ms" latency for the next
  /// measured save). Returns whether it cleared within the timeout.
  async waitForClear(uri, predicate, timeoutMsLocal) {
    const start = Date.now();
    while (Date.now() - start < timeoutMsLocal) {
      const stillPresent = (this.diagnostics[uri] ?? []).some(predicate);
      if (!stillPresent) return true;
      await sleep(DIAGNOSTIC_POLL_MS);
    }
    return false;
  }

  async shutdown() {
    try {
      await Promise.race([this.request("shutdown", null), sleep(2000)]);
      this.notify("exit", null);
      await sleep(200);
    } catch {
      // falls through to kill() below
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

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
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

function median(samples) {
  if (samples.length === 0) return null;
  const sorted = [...samples].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function max(samples) {
  return samples.length === 0 ? null : Math.max(...samples);
}

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
    // Makes rust-analyzer send `experimental/serverStatus` (quiescent flag).
    experimental: { serverStatusNotification: true },
    general: { positionEncodings: ["utf-16"] },
  };
}

const RA_INIT_OPTIONS = {
  checkOnSave: true,
  cargo: { buildScripts: { enable: true } },
  // `completion.autoimport.enable` (on by default) is set explicitly so it
  // cannot silently vary by rust-analyzer version/global config. It is NOT
  // what produces the 1000+ candidates bucket (that is a local 1200-variant
  // enum, see `COMPLETION_SITES`); it only keeps both sessions' import
  // behaviour identical.
  completion: { autoimport: { enable: true } },
};

function isUsefulHover(value) {
  if (value === null || value === undefined) return false;
  const text = JSON.stringify(value.contents ?? value);
  return !text.includes("{unknown}") && !text.includes("{error}");
}

function isUsefulDefinition(value) {
  if (value === null || value === undefined) return false;
  return Array.isArray(value) ? value.length > 0 : true;
}

function completionItems(value) {
  const items = Array.isArray(value) ? value : value?.items;
  return Array.isArray(items) ? items : [];
}

function hasCompletionItems(value) {
  return completionItems(value).length > 0;
}

async function requestWithRetry(proc, method, params, isReady, timeoutMsLocal) {
  const start = Date.now();
  let lastElapsed = null;
  let lastError = null;
  while (Date.now() - start < timeoutMsLocal) {
    try {
      const { result, elapsedMs } = await proc.request(method, params);
      lastElapsed = elapsedMs;
      if (isReady(result)) return { result, elapsedSinceStartMs: Date.now() - start };
    } catch (err) {
      lastError = err;
    }
    await sleep(300);
  }
  throw new Error(
    `requestWithRetry(${method}): timed out after ${timeoutMsLocal}ms (last elapsed ${lastElapsed}ms` +
      `${lastError ? `, last error: ${JSON.stringify(lastError)}` : ""})`,
  );
}

// ---------------------------------------------------------------------
// Settling: wait until rust-analyzer has stopped changing its answers
// before any sample is taken (A1 review M4). Sampling before that mixed a
// late background re-analysis (candidate count 1200 -> 1204 as core's
// blanket-impl methods become visible) into the measured distribution.
// ---------------------------------------------------------------------

/// A result must repeat this many requests in a row ...
const SETTLE_CONSECUTIVE = 5;
/// ... spanning at least this long, polling this often.
const SETTLE_MIN_SPAN_MS = 2000;
const SETTLE_POLL_MS = 400;
/// `experimental/serverStatus` must stay quiescent this long.
const QUIESCENT_HOLD_MS = 1000;

/// Direct rust-analyzer sessions only (outou-lsp does not forward
/// `experimental/serverStatus`): waits for `quiescent: true` to hold for
/// `QUIESCENT_HOLD_MS`. Throws a clear error on timeout, or if the server
/// never sent a status at all.
async function waitQuiescent(proc, timeoutMsLocal) {
  const start = Date.now();
  let quietSince = null;
  while (Date.now() - start < timeoutMsLocal) {
    const status = proc.serverStatus;
    if (status && status.quiescent === true) {
      quietSince ??= Date.now();
      if (Date.now() - quietSince >= QUIESCENT_HOLD_MS) {
        return { elapsedMs: Date.now() - start, health: status.health ?? null };
      }
    } else {
      quietSince = null;
    }
    await sleep(100);
  }
  const seen = proc.serverStatus === null ? "never received experimental/serverStatus" : `last status ${JSON.stringify(proc.serverStatus)}`;
  throw new Error(`waitQuiescent: rust-analyzer not quiescent after ${timeoutMsLocal}ms (${seen})`);
}

/// Repeats `method` until `signatureOf(result)` is non-null (ready) and
/// identical for `SETTLE_CONSECUTIVE` consecutive requests spanning at
/// least `SETTLE_MIN_SPAN_MS`. With `expected`, additionally waits for that
/// signature; if the timeout hits while the session is stable on a
/// *different* signature it returns `matchedExpected: false` (a real
/// divergence the caller must report) instead of throwing. Any other
/// timeout throws with the recent signatures.
async function waitForStable(proc, method, params, signatureOf, timeoutMsLocal, expected) {
  const start = Date.now();
  const history = []; // { t, signature }
  let lastError = null;
  const stableSignature = () => {
    // The trailing run of identical, non-null signatures must have at least
    // SETTLE_CONSECUTIVE requests and span at least SETTLE_MIN_SPAN_MS (a
    // fixed-size window would be unsatisfiable when requests are fast).
    const lastEntry = history[history.length - 1];
    if (lastEntry === undefined || lastEntry.signature === null) return null;
    let runStart = history.length - 1;
    while (runStart > 0 && history[runStart - 1].signature === lastEntry.signature) runStart -= 1;
    if (history.length - runStart < SETTLE_CONSECUTIVE) return null;
    if (lastEntry.t - history[runStart].t < SETTLE_MIN_SPAN_MS) return null;
    return lastEntry.signature;
  };
  while (Date.now() - start < timeoutMsLocal) {
    try {
      const { result } = await proc.request(method, params);
      history.push({ t: Date.now(), signature: signatureOf(result) });
    } catch (err) {
      lastError = err;
      history.push({ t: Date.now(), signature: null });
    }
    const stable = stableSignature();
    if (stable !== null && (expected === undefined || stable === expected)) {
      return { signature: stable, requests: history.length, elapsedMs: Date.now() - start, matchedExpected: true };
    }
    await sleep(SETTLE_POLL_MS);
  }
  const stable = stableSignature();
  if (stable !== null && expected !== undefined) {
    return { signature: stable, requests: history.length, elapsedMs: Date.now() - start, matchedExpected: false };
  }
  const recent = history.slice(-8).map((h) => h.signature);
  throw new Error(
    `waitForStable(${method}): no stable answer after ${timeoutMsLocal}ms ` +
      `(recent signatures ${JSON.stringify(recent)}${lastError ? `, last error ${JSON.stringify(lastError)}` : ""})`,
  );
}

const completionSignature = (result) => {
  const count = completionItems(result).length;
  return count > 0 ? String(count) : null;
};
const hoverSignature = (result) => (isUsefulHover(result) ? JSON.stringify(result.contents ?? result) : null);

// ---------------------------------------------------------------------
// Mode: coldinit
// ---------------------------------------------------------------------
//
// One fresh `outou-lsp` process per run. The clock starts when `initialize`
// is SENT (`outou-lsp` runs `load_workspace` and the rust-analyzer handshake
// before it replies, `crates/outou-lsp/src/server.rs`, so that work is
// inside the measurement). Hover, completion and definition are then each
// requested with retry until a useful answer comes back. Reported both
// cumulatively from `initialize` sent ("time until X is usable", the number
// a user feels) and per step. Sequential within one process (hover, then
// completion, then definition); N fresh processes, median/max per quantity.
async function modeColdinit(args) {
  const root = resolve(args.root);
  const n = Number(args.n ?? 5);
  const timeoutMs = Number(args.timeout ?? 240000);
  const mainRsxPath = resolve(root, "src/main.rsx");
  const originalText = readFileSync(mainRsxPath, "utf8");
  const mainUri = pathToFileURL(mainRsxPath).href;

  const hoverPos = positionOf(originalText, "let user = load_user();");
  const definitionPos = positionOf(originalText, "load_user(", 0);

  // Cumulative from `initialize` sent.
  const samples = { initializeResponse: [], hover: [], completion: [], definition: [], total: [] };
  // Per step (hover/completion/definition: the retry loop's own elapsed time).
  const stepSamples = { hover: [], completion: [], definition: [] };
  const runs = [];

  for (let i = 0; i < n; i++) {
    const env = { ...process.env, OUTOU_RUST_ANALYZER: resolve(args.ra) };
    const proc = new LspProcess(args["outou-lsp"], [], env);
    try {
      const initStart = Date.now();
      await proc.request("initialize", {
        processId: process.pid,
        capabilities: capabilities(),
        workspaceFolders: [{ uri: pathToFileURL(root).href, name: "measure-a1-coldinit" }],
        initializationOptions: {},
      });
      const initializeResponseMs = Date.now() - initStart;
      proc.notify("initialized", {});
      proc.notify("textDocument/didOpen", {
        textDocument: { uri: mainUri, languageId: "outou-rsx", version: 1, text: originalText },
      });

      const hover = await requestWithRetry(
        proc,
        "textDocument/hover",
        { textDocument: { uri: mainUri }, position: { line: hoverPos.line, character: hoverPos.character + 6 } },
        isUsefulHover,
        timeoutMs,
      );
      const hoverCumulativeMs = Date.now() - initStart;

      const completionText = originalText.replace(
        "let user = load_user();",
        "let user = load_user();\n    user.",
      );
      proc.notify("textDocument/didChange", {
        textDocument: { uri: mainUri, version: 2 },
        contentChanges: [{ text: completionText }],
      });
      const completionLine = completionText.split("\n").findIndex((l) => l.trim() === "user.");
      const completion = await requestWithRetry(
        proc,
        "textDocument/completion",
        { textDocument: { uri: mainUri }, position: { line: completionLine, character: 9 } },
        hasCompletionItems,
        timeoutMs,
      );
      const completionCumulativeMs = Date.now() - initStart;

      const definition = await requestWithRetry(
        proc,
        "textDocument/definition",
        { textDocument: { uri: mainUri }, position: { line: definitionPos.line, character: definitionPos.character + 2 } },
        isUsefulDefinition,
        timeoutMs,
      );
      const totalMs = Date.now() - initStart;

      samples.initializeResponse.push(initializeResponseMs);
      samples.hover.push(hoverCumulativeMs);
      samples.completion.push(completionCumulativeMs);
      samples.definition.push(totalMs);
      samples.total.push(totalMs);
      stepSamples.hover.push(hover.elapsedSinceStartMs);
      stepSamples.completion.push(completion.elapsedSinceStartMs);
      stepSamples.definition.push(definition.elapsedSinceStartMs);
      runs.push({ run: i, initializeResponseMs, hoverCumulativeMs, completionCumulativeMs, totalMs });
    } finally {
      await proc.shutdown();
    }
  }

  const stat = (values) => ({ medianMs: median(values), maxMs: max(values) });
  return {
    mode: "coldinit",
    clockStart: "initialize sent",
    n,
    root,
    runs,
    samples,
    stepSamples,
    summary: {
      initializeResponse: stat(samples.initializeResponse),
      hover: { cumulative: stat(samples.hover), step: stat(stepSamples.hover) },
      completion: { cumulative: stat(samples.completion), step: stat(stepSamples.completion) },
      definition: { cumulative: stat(samples.definition), step: stat(stepSamples.definition) },
      total: stat(samples.total),
    },
  };
}

// ---------------------------------------------------------------------
// Mode: flycheck
// ---------------------------------------------------------------------
//
// One warm `outou-lsp` session and one warm plain-rust-analyzer session
// (on the generated file it opens, standing in for "a plain `.rs` save"),
// each: warm up until a first save's diagnostic round-trips, then N
// repeated edit/save/revert cycles, timing `didSave` -> the matching
// diagnostic's publish.
async function modeFlycheck(args) {
  const root = resolve(args.root);
  const n = Number(args.n ?? 10);
  const timeoutMs = Number(args.timeout ?? 60000);

  const outou = await flycheckThroughOutouLsp(root, args, n, timeoutMs);
  const direct = await flycheckDirect(root, args, n, timeoutMs);

  return {
    mode: "flycheck",
    // `waitForDiagnostic` polls every 50 ms; differences below that are noise.
    pollResolutionMs: DIAGNOSTIC_POLL_MS,
    n,
    root,
    outouLsp: outou,
    direct,
    summary: {
      outouLsp: { medianMs: median(outou.samples), maxMs: max(outou.samples) },
      direct: { medianMs: median(direct.samples), maxMs: max(direct.samples) },
    },
  };
}

async function flycheckThroughOutouLsp(root, args, n, timeoutMs) {
  const mainRsxPath = resolve(root, "src/main.rsx");
  const mainUri = pathToFileURL(mainRsxPath).href;
  const originalText = readFileSync(mainRsxPath, "utf8");
  const broken = originalText.replace(
    "let user = load_user();",
    "let user: i32 = load_user();",
  );
  if (broken === originalText) throw new Error("flycheck: expected target line not found in main.rsx");

  const env = { ...process.env, OUTOU_RUST_ANALYZER: resolve(args.ra) };
  const proc = new LspProcess(args["outou-lsp"], [], env);
  const samples = [];
  try {
    await proc.request("initialize", {
      processId: process.pid,
      capabilities: capabilities(),
      workspaceFolders: [{ uri: pathToFileURL(root).href, name: "measure-a1-flycheck" }],
      initializationOptions: {},
    });
    proc.notify("initialized", {});
    proc.notify("textDocument/didOpen", {
      textDocument: { uri: mainUri, languageId: "outou-rsx", version: 1, text: originalText },
    });
    let version = 2;

    // rust-analyzer's `checkOnSave` flycheck does not run usefully until
    // its initial project/crate-graph load has finished; a fresh session
    // that jumps straight to `didSave` (no prior request) can otherwise
    // stall indefinitely. A hover warm-up, exactly like `coldinit`'s,
    // establishes that before any measured save/wait cycle.
    const hoverPos = positionOf(originalText, "let user = load_user();");
    await requestWithRetry(
      proc,
      "textDocument/hover",
      { textDocument: { uri: mainUri }, position: { line: hoverPos.line, character: hoverPos.character + 6 } },
      isUsefulHover,
      timeoutMs,
    );

    // `outou-lsp`'s own `textDocument/didSave` re-plans the crate *from
    // the now-on-disk sources* (`crates/outou-lsp/README.md`, "Running
    // it"), unlike a plain rust-analyzer session, which trusts its
    // `didChange` overlay: a save that only updates the in-memory buffer
    // (no write to disk) would be re-planned from the unchanged file on
    // disk, so the deliberately broken text would never actually reach
    // rust-analyzer at all. Every measured save here therefore writes to
    // disk first, matching `outou-lsp-client.mjs`'s own `saveToDisk` and
    // what a real editor's "Save" always does anyway.
    const saveAndWait = async (text, predicate) => {
      writeFileSync(mainRsxPath, text);
      proc.notify("textDocument/didChange", {
        textDocument: { uri: mainUri, version: version++ },
        contentChanges: [{ text }],
      });
      const start = Date.now();
      proc.notify("textDocument/didSave", { textDocument: { uri: mainUri } });
      const elapsed = await proc.waitForDiagnostic(mainUri, predicate, timeoutMs);
      if (elapsed === null) {
        throw new Error(`flycheck (outou-lsp): timed out waiting for a matching diagnostic after ${Date.now() - start}ms`);
      }
      return elapsed;
    };
    const isTypeError = (d) => (d.message ?? "").includes("mismatched types");

    // Reverts to the original (correct) text, saves, and waits until the
    // type-error diagnostic has actually cleared — not merely until the
    // save completes — so the *next* `saveAndWait(broken, ...)` starts
    // from a genuinely clean state instead of immediately matching a
    // stale diagnostic still sitting in `proc.diagnostics` from before
    // this revert's own publish arrived (which would otherwise report a
    // false "0ms" latency for the next measured introduction).
    const revertAndWaitClear = async () => {
      writeFileSync(mainRsxPath, originalText);
      proc.notify("textDocument/didChange", {
        textDocument: { uri: mainUri, version: version++ },
        contentChanges: [{ text: originalText }],
      });
      proc.notify("textDocument/didSave", { textDocument: { uri: mainUri } });
      if (!(await proc.waitForClear(mainUri, isTypeError, timeoutMs))) {
        throw new Error(`flycheck (outou-lsp): the type-error diagnostic did not clear within ${timeoutMs}ms of the revert`);
      }
    };

    // Warm-up: one full cycle, not measured.
    await saveAndWait(broken, isTypeError);
    await revertAndWaitClear();

    for (let i = 0; i < n; i++) {
      const introduced = await saveAndWait(broken, isTypeError);
      samples.push(introduced);
      await revertAndWaitClear();
    }
  } finally {
    await proc.shutdown();
  }
  return { samples };
}

async function flycheckDirect(root, args, n, timeoutMs) {
  const generatedPath = resolve(root, "src/.generated/crate-root.rs");
  const generatedUri = pathToFileURL(generatedPath).href;
  const originalText = readFileSync(generatedPath, "utf8");
  const needle = "let user = load_user();";
  if (!originalText.includes(needle)) {
    throw new Error("flycheck (direct): expected `let user = load_user();` not found in generated crate-root.rs");
  }
  const broken = originalText.replace(needle, "let user: i32 = load_user();");

  const proc = new LspProcess(args.ra, [], process.env);
  const samples = [];
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
    let version = 2;

    const hoverPos = positionOf(originalText, needle);
    await requestWithRetry(
      proc,
      "textDocument/hover",
      { textDocument: { uri: generatedUri }, position: { line: hoverPos.line, character: hoverPos.character + 6 } },
      isUsefulHover,
      timeoutMs,
    );

    const isTypeError = (d) => (d.message ?? "").includes("mismatched types");

    // `checkOnSave` shells out to a real `cargo check`, which reads the
    // file from disk regardless of what rust-analyzer's own `didChange`
    // overlay holds — every measured save writes to disk first, exactly
    // like `flycheckThroughOutouLsp`'s own `saveAndWait` (and what a real
    // editor's "Save" always does).
    const saveAndWait = async (text, predicate) => {
      writeFileSync(generatedPath, text);
      proc.notify("textDocument/didChange", {
        textDocument: { uri: generatedUri, version: version++ },
        contentChanges: [{ text }],
      });
      const start = Date.now();
      proc.notify("textDocument/didSave", { textDocument: { uri: generatedUri } });
      const elapsed = await proc.waitForDiagnostic(generatedUri, predicate, timeoutMs);
      if (elapsed === null) {
        throw new Error(`flycheck (direct): timed out waiting for a matching diagnostic after ${Date.now() - start}ms`);
      }
      return elapsed;
    };

    // See `flycheckThroughOutouLsp`'s own `revertAndWaitClear` for why
    // this waits for the diagnostic to actually clear rather than just
    // for the revert save to complete.
    const revertAndWaitClear = async () => {
      writeFileSync(generatedPath, originalText);
      proc.notify("textDocument/didChange", {
        textDocument: { uri: generatedUri, version: version++ },
        contentChanges: [{ text: originalText }],
      });
      proc.notify("textDocument/didSave", { textDocument: { uri: generatedUri } });
      if (!(await proc.waitForClear(generatedUri, isTypeError, timeoutMs))) {
        throw new Error(`flycheck (direct): the type-error diagnostic did not clear within ${timeoutMs}ms of the revert`);
      }
    };

    await saveAndWait(broken, isTypeError);
    await revertAndWaitClear();

    for (let i = 0; i < n; i++) {
      const introduced = await saveAndWait(broken, isTypeError);
      samples.push(introduced);
      await revertAndWaitClear();
    }
  } finally {
    await proc.shutdown();
  }
  return { samples };
}

// ---------------------------------------------------------------------
// Mode: completion
// ---------------------------------------------------------------------
//
// Same-session A/B method (`docs/gate3-results.md`, "Proxy overhead
// (S8)"), generalized to a list of measurement-only *sites* injected at
// runtime into the fresh temp copy of `examples/phase0-app` this
// harness's caller already made — never into a checked-in fixture other
// tests depend on. Each site is a small, self-contained code shape
// designed to make rust-analyzer's own returned candidate-list size land
// in a different rough order of magnitude (tens / hundreds / 1000+), so
// overhead can be measured per *observed* size rather than assumed. A
// site consists of:
//
//   - `structDef` (optional): extra plain-Rust top-level source, inserted
//     verbatim before `fn load_user(` in both the `.rsx` source and the
//     already-generated `crate-root.rs` — plain (non-JSX) Rust items pass
//     through Outou's codegen unchanged, so the identical text is valid
//     in both places.
//   - `insertAfterNeedle` / `insertText`: the statement inserted right
//     after `insertAfterNeedle` in both files (again identical text,
//     since a plain non-JSX statement round-trips unchanged).
//   - `cursorNeedle`: the substring whose end marks the cursor position
//     for the completion request.
//
// `candidateCount` is read from each session's own response independently
// (never assumed equal between `outou-lsp` and the direct session) — see
// `docs/alpha-results.md`, "A1: measurement", (d) for the observed counts
// and why the earlier prefix-based bucketing attempt did not actually
// vary candidate count.
/// A plain C-like enum definition with `count` variants (`V0`..`V{count-1}`),
/// for the "large-enum-path" completion site below.
function largeEnumDef(name, count) {
  const variants = Array.from({ length: count }, (_, i) => `    V${i},\n`).join("");
  return `enum ${name} {\n${variants}}\n\n`;
}

const COMPLETION_SITES = [
  {
    label: "tiny-local-type",
    structDef:
      "struct __MeasureTiny;\nimpl __MeasureTiny {\n    fn alpha(&self) {}\n    fn beta(&self) {}\n    fn gamma(&self) {}\n}\n\n",
    insertAfterNeedle: "let user = load_user();",
    insertText: "\n    let __measure_tiny = __MeasureTiny;\n    __measure_tiny.",
    cursorNeedle: "__measure_tiny.",
  },
  {
    label: "option-member-access",
    structDef: "",
    insertAfterNeedle: "let user = load_user();",
    insertText: "\n    user.",
    cursorNeedle: "user.",
  },
  {
    label: "large-enum-path",
    // Every other approach tried for a large candidate list (a longer
    // identifier prefix on a member-access site; a single-character
    // prefix at an expression position; a single-character prefix at a
    // type-ascription position) plateaued at 26-42 candidates regardless
    // of method — this rust-analyzer version's import/flyimport search
    // appears to cap its own suggestion count well under 1000 (see
    // `docs/alpha-results.md`, (d), for the list of approaches). A
    // large local enum sidesteps import search entirely: `EnumName::`
    // path completion lists every variant directly from the already-
    // resolved local type, uncapped by any import-suggestion limit, and
    // is a genuine (if synthetic) shape a real project can hit — a big
    // generated state machine or protocol/opcode enum.
    structDef: largeEnumDef("__MeasureMega", 1200),
    insertAfterNeedle: "let user = load_user();",
    insertText: "\n    let __measure_blank = __MeasureMega::;",
    cursorNeedle: "__measure_blank = __MeasureMega::",
  },
];

async function modeCompletion(args) {
  const root = resolve(args.root);
  const n = Number(args.n ?? 10);
  const timeoutMs = Number(args.timeout ?? 120000);

  // `--site <label>` restricts the run to one site (default: all three).
  const sites = args.site
    ? COMPLETION_SITES.filter((s) => s.label === args.site)
    : COMPLETION_SITES;
  if (sites.length === 0) {
    throw new Error(
      `unknown --site ${JSON.stringify(args.site)}; expected one of ${COMPLETION_SITES.map((s) => s.label).join(", ")}`,
    );
  }

  const buckets = [];
  for (const site of sites) {
    // Direct first: its settled candidate count is what the `outou-lsp`
    // session must converge to before it is sampled.
    const direct = await completionDirect(root, args, n, timeoutMs, site);
    const outouLsp = await completionThroughOutouLsp(root, args, n, timeoutMs, site, direct.settledCount);
    const sessionSummary = (r) => ({
      candidateCount: r.candidateCount,
      settledCount: r.settledCount,
      settleMs: r.settleMs,
      settleRequests: r.settleRequests,
      candidateCounts: r.counts,
      countsStable: r.counts.every((c) => c === r.counts[0]),
      attempts: r.attempts,
      retriedSamples: r.retriedSamples,
      samplesMs: r.samples,
      medianMs: median(r.samples),
      p95Ms: p95(r.samples),
    });
    buckets.push({
      label: site.label,
      // Both sessions must settle on, and be sampled at, the same count;
      // `measure_a1.rs` fails the bucket otherwise.
      candidateCountsAgree:
        outouLsp.settledCount === direct.settledCount &&
        outouLsp.candidateCount === direct.candidateCount,
      outouLsp: sessionSummary(outouLsp),
      direct: sessionSummary(direct),
      overheadMs: {
        median: median(outouLsp.samples) - median(direct.samples),
        p95: p95(outouLsp.samples) - p95(direct.samples),
      },
    });
  }
  return { mode: "completion", n, root, sampleIndexBase: 0, buckets };
}

function p95(samples) {
  if (samples.length === 0) return null;
  const sorted = [...samples].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.ceil(0.95 * sorted.length) - 1);
  return sorted[idx];
}

/// Applies `site`'s `structDef`/`insertAfterNeedle`/`insertText` to
/// `text`, returning the mutated text plus the cursor `{line, character}`
/// derived from `site.cursorNeedle`'s end position.
function applySite(text, site) {
  const structAnchor = "fn load_user(";
  if (site.structDef && !text.includes(structAnchor)) {
    throw new Error(`completion site ${site.label}: expected ${JSON.stringify(structAnchor)} not found`);
  }
  let mutated = site.structDef ? text.replace(structAnchor, `${site.structDef}${structAnchor}`) : text;
  if (!mutated.includes(site.insertAfterNeedle)) {
    throw new Error(`completion site ${site.label}: expected ${JSON.stringify(site.insertAfterNeedle)} not found`);
  }
  mutated = mutated.replace(site.insertAfterNeedle, `${site.insertAfterNeedle}${site.insertText}`);
  const needlePos = positionOf(mutated, site.cursorNeedle, 0);
  return { mutated, cursor: { line: needlePos.line, character: needlePos.character + site.cursorNeedle.length } };
}

async function completionThroughOutouLsp(root, args, n, timeoutMs, site, expectedSettledCount) {
  const mainRsxPath = resolve(root, "src/main.rsx");
  const mainUri = pathToFileURL(mainRsxPath).href;
  const originalText = readFileSync(mainRsxPath, "utf8");
  const { mutated, cursor } = applySite(originalText, site);
  const env = { ...process.env, OUTOU_RUST_ANALYZER: resolve(args.ra) };
  const proc = new LspProcess(args["outou-lsp"], [], env);
  try {
    await proc.request("initialize", {
      processId: process.pid,
      capabilities: capabilities(),
      workspaceFolders: [{ uri: pathToFileURL(root).href, name: "measure-a1-completion" }],
      // `outou-lsp`'s own `build_ra_init_options` (`crates/outou-lsp/src/server.rs`)
      // starts from exactly this object and only forces `checkOnSave`/
      // `cargo.buildScripts.enable` on top of it, so `completion.autoimport`
      // reaches the spawned rust-analyzer unchanged — matching the direct
      // session's own `RA_INIT_OPTIONS`, which now also sets it.
      initializationOptions: { completion: { autoimport: { enable: true } } },
    });
    proc.notify("initialized", {});
    proc.notify("textDocument/didOpen", {
      textDocument: { uri: mainUri, languageId: "outou-rsx", version: 1, text: originalText },
    });
    proc.notify("textDocument/didChange", {
      textDocument: { uri: mainUri, version: 2 },
      contentChanges: [{ text: mutated }],
    });
    const params = { textDocument: { uri: mainUri }, position: cursor };
    // `outou-lsp` does not forward `experimental/serverStatus`, so settle
    // on the answer itself: stable count over >= 2 s, equal to the direct
    // session's settled count when that is known.
    const settled = await waitForStable(
      proc,
      "textDocument/completion",
      params,
      completionSignature,
      timeoutMs,
      expectedSettledCount === undefined ? undefined : String(expectedSettledCount),
    );
    return await measureCompletionSamples(proc, params, n, settled);
  } finally {
    await proc.shutdown();
  }
}

const COMPLETION_MAX_ATTEMPTS = 4;

/// Sends `textDocument/completion` `n` times after the session has settled,
/// recording each response's own `elapsedMs` and candidate count. An empty
/// response for the identical request is retried (up to
/// `COMPLETION_MAX_ATTEMPTS` attempts, 200 ms apart) before being accepted
/// at face value. `attempts[i]` is the number of attempts sample `i` needed
/// and `retriedSamples` lists the (0-based) samples that needed more than
/// one, so retries are visible instead of silently absorbed; the recorded
/// time is the final attempt's own. `counts` has every sample's count;
/// `candidateCount` is the most frequent one.
async function measureCompletionSamples(proc, params, n, settled) {
  const samples = [];
  const counts = [];
  const attempts = [];
  for (let i = 0; i < n; i++) {
    let elapsedMs, count;
    let attempt = 0;
    while (attempt < COMPLETION_MAX_ATTEMPTS) {
      attempt++;
      const response = await proc.request("textDocument/completion", params);
      elapsedMs = response.elapsedMs;
      count = completionItems(response.result).length;
      if (count > 0) break;
      await sleep(200);
    }
    samples.push(elapsedMs);
    counts.push(count);
    attempts.push(attempt);
  }
  const frequency = new Map();
  for (const c of counts) frequency.set(c, (frequency.get(c) ?? 0) + 1);
  const candidateCount = [...frequency.entries()].sort((a, b) => b[1] - a[1])[0][0];
  const retriedSamples = attempts.flatMap((a, index) => (a > 1 ? [{ index, attempts: a }] : []));
  return {
    samples,
    candidateCount,
    counts,
    attempts,
    retriedSamples,
    settledCount: Number(settled.signature),
    settleMs: settled.elapsedMs,
    settleRequests: settled.requests,
  };
}

async function completionDirect(root, args, n, timeoutMs, site) {
  const generatedPath = resolve(root, "src/.generated/crate-root.rs");
  const generatedUri = pathToFileURL(generatedPath).href;
  const originalText = readFileSync(generatedPath, "utf8");
  const { mutated, cursor } = applySite(originalText, site);
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
    proc.notify("textDocument/didChange", {
      textDocument: { uri: generatedUri, version: 2 },
      contentChanges: [{ text: mutated }],
    });
    const params = { textDocument: { uri: generatedUri }, position: cursor };
    await waitQuiescent(proc, timeoutMs);
    const settled = await waitForStable(proc, "textDocument/completion", params, completionSignature, timeoutMs);
    return await measureCompletionSamples(proc, params, n, settled);
  } finally {
    await proc.shutdown();
  }
}

// ---------------------------------------------------------------------
// Mode: rss
// ---------------------------------------------------------------------
//
// `--config single`: one plain rust-analyzer session on `root`.
// `--config two-process`: `outou-lsp` (which spawns its own
// rust-analyzer child, found via `pgrep -P`) plus a second, independent
// plain rust-analyzer session on the same `root` — standing in for the
// editor's own session against the same crate (`crates/outou-lsp/README.md`,
// "Two processes, not one").
//
// Each measured entry is a whole process TREE under its root pid
// (rust-analyzer-proc-macro-srv, transient cargo/rustc for flycheck and
// build scripts), sampled every `--interval-ms` from one `ps -A` snapshot:
// summed RSS (`ps` RSS, which collapses when macOS compresses a process's
// pages) next to the summed macOS physical footprint (`footprint(1)`, which
// counts compressed pages and so does not collapse). CPU is the tree's
// summed cumulative CPU time at each sample (a transient child that has
// already exited stops contributing, so this can slightly undercount).
//
// `--phase cold`: sample from spawn until BOTH sessions have settled (same
// criterion for both, see `settleSession`). `--phase cold+edit`: the same
// cold window, then, in the SAME process (no restart), `--edits` scripted
// edits; the result reports the two windows separately (`windows.cold`,
// `windows.edit`, split at `settledAtMs`).
async function modeRss(args) {
  const root = resolve(args.root);
  const config = args.config ?? "single";
  const phase = args.phase ?? "cold";
  const intervalMs = Number(args["interval-ms"] ?? 500);
  const edits = Number(args.edits ?? 10);
  const editDelayMs = Number(args["edit-delay-ms"] ?? 500);
  const timeoutMs = Number(args.timeout ?? 180000);

  // Spawned from t0 so the cold window starts at process spawn.
  const t0 = Date.now();
  const entries = []; // { label, pid, tree }
  const direct = await startDirectSession(root, args);
  entries.push({ label: "plain-rust-analyzer", pid: direct.proc.pid, tree: true });

  let outouLspEntry = null;
  if (config === "two-process") {
    const outou = await startOutouLspSession(root, args);
    const raChildPid = await findChildPid(outou.proc.pid, timeoutMs);
    entries.push({ label: "outou-lsp", pid: outou.proc.pid, tree: false });
    entries.push({ label: "outou-lsp-ra-child", pid: raChildPid, tree: true });
    outouLspEntry = outou;
  }

  const samples = []; // { atMs, [label]: { rssKb, footprintKb, cpuSeconds, processCount } }
  const sample = () => samples.push({ atMs: Date.now() - t0, ...readEntries(entries) });

  const sampler = setInterval(sample, intervalMs);
  sample();

  let settledAtMs = null;
  const settle = {};
  try {
    settle.plainRustAnalyzer = await direct.settle(timeoutMs);
    if (outouLspEntry) settle.outouLsp = await outouLspEntry.settle(timeoutMs);
    sample();
    settledAtMs = Date.now() - t0;
    if (phase === "cold+edit") {
      for (let i = 0; i < edits; i++) {
        await direct.edit(i);
        if (outouLspEntry) await outouLspEntry.edit(i);
        await sleep(editDelayMs);
      }
      await sleep(editDelayMs * 2);
      sample();
    }
  } finally {
    clearInterval(sampler);
    await direct.proc.shutdown();
    if (outouLspEntry) await outouLspEntry.proc.shutdown();
  }

  const windows = { cold: summarizeWindow(samples.filter((s) => s.atMs <= settledAtMs), entries, config) };
  if (phase === "cold+edit") {
    windows.edit = summarizeWindow(samples.filter((s) => s.atMs > settledAtMs), entries, config, samples.filter((s) => s.atMs <= settledAtMs).at(-1));
  }

  return {
    mode: "rss",
    config,
    phase,
    root,
    intervalMs,
    edits,
    editDelayMs,
    settledAtMs,
    settle,
    note: "rssKb is summed ps RSS over the process tree (excludes pages macOS has compressed); footprintKb is the summed macOS physical footprint (includes them). Prefer footprint when the two disagree.",
    windows,
    samples,
  };
}

/// Per-label and (two-process) total stats over `window`'s samples.
/// `before`, when given, is the last sample before the window; it supplies
/// the CPU baseline so the edit window reports CPU consumed *during* it.
function summarizeWindow(window, entries, config, before) {
  const labels = entries.map((e) => e.label);
  const seriesOf = (label) => window.map((s) => s[label]).filter((v) => v && v.rssKb !== null);
  const baselineOf = (label) => before?.[label]?.cpuSeconds ?? null;
  const out = {};
  for (const label of labels) out[label] = statsOf(seriesOf(label), baselineOf(label));
  if (config === "two-process") {
    const parts = ["outou-lsp-ra-child", "plain-rust-analyzer"];
    const combined = window.map((s) => combine(parts.map((l) => s[l])));
    out.total = {
      ...statsOf(combined.filter((v) => v !== null), before ? sumNullable(parts.map(baselineOf)) : null),
      note: "outou-lsp-ra-child tree + plain-rust-analyzer tree (the editor's own session); excludes outou-lsp's own process",
    };
  }
  return out;
}

function combine(values) {
  if (values.some((v) => !v || v.rssKb === null)) return null;
  return {
    rssKb: sumNullable(values.map((v) => v.rssKb)),
    footprintKb: sumNullable(values.map((v) => v.footprintKb)),
    cpuSeconds: sumNullable(values.map((v) => v.cpuSeconds)),
  };
}

function sumNullable(values) {
  return values.some((v) => v === null || v === undefined) ? null : values.reduce((a, b) => a + b, 0);
}

function statsOf(series, cpuBaseline) {
  if (series.length === 0) return { sampleCount: 0 };
  const column = (key) => series.map((v) => v[key]).filter((v) => v !== null && v !== undefined);
  const peak = (values) => (values.length > 0 ? Math.max(...values) : null);
  const last = (values) => (values.length > 0 ? values[values.length - 1] : null);
  const cpu = column("cpuSeconds");
  const cpuStart = cpuBaseline ?? (cpu.length > 0 ? cpu[0] : null);
  return {
    sampleCount: series.length,
    peakRssKb: peak(column("rssKb")),
    lastRssKb: last(column("rssKb")),
    peakFootprintKb: peak(column("footprintKb")),
    lastFootprintKb: last(column("footprintKb")),
    cpuSecondsConsumed: cpuStart !== null && cpu.length > 0 ? Number((Math.max(...cpu) - cpuStart).toFixed(2)) : null,
    peakProcessCount: peak(column("processCount")),
  };
}

/// Hover-warm-up anchor present in `text`. `examples/phase0-app`:
/// `let user = load_user();`. `tests/fixtures/workspace/app`: `Card` in
/// `use ui_kit::{Button, Card};`, which resolves only once the `ui-kit`
/// path dependency has been loaded, so a useful hover there implies
/// cross-crate resolution (the point of measuring that workspace).
function warmupAnchor(text) {
  if (text.includes("let user = load_user();")) {
    return { needle: "let user = load_user();", offset: 6 };
  }
  if (text.includes("use ui_kit::{Button, Card};")) {
    return { needle: "Card", offset: 1 };
  }
  throw new Error("warmupAnchor: neither `let user = load_user();` nor `use ui_kit::{Button, Card};` found");
}

/// Settled = a useful hover at the anchor that is identical for
/// `SETTLE_CONSECUTIVE` requests over >= `SETTLE_MIN_SPAN_MS`. A direct
/// session additionally waits for `experimental/serverStatus` quiescent
/// (outou-lsp does not forward it). The same criterion ends the cold window
/// for every session in every config, so the window is not a race against
/// the first lucky hover.
async function settleSession(proc, hoverParams, timeoutMsLocal, { requireQuiescent }) {
  const start = Date.now();
  const quiescent = requireQuiescent ? await waitQuiescent(proc, timeoutMsLocal) : null;
  const stable = await waitForStable(proc, "textDocument/hover", hoverParams, hoverSignature, timeoutMsLocal);
  return { settleMs: Date.now() - start, quiescent, hoverRequests: stable.requests };
}

async function startDirectSession(root, args) {
  const generatedPath = resolve(root, "src/.generated/crate-root.rs");
  const generatedUri = pathToFileURL(generatedPath).href;
  const originalText = readFileSync(generatedPath, "utf8");
  const proc = new LspProcess(args.ra, [], process.env);
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
  const anchor = warmupAnchor(originalText);
  const hoverPos = positionOf(originalText, anchor.needle);
  const hoverParams = {
    textDocument: { uri: generatedUri },
    position: { line: hoverPos.line, character: hoverPos.character + anchor.offset },
  };
  let version = 2;
  return {
    proc,
    settle: (timeoutMs) => settleSession(proc, hoverParams, timeoutMs, { requireQuiescent: true }),
    edit: async (i) => {
      const text = i % 2 === 0 ? `${originalText}\n// measure-a1 edit ${i}\n` : originalText;
      proc.notify("textDocument/didChange", {
        textDocument: { uri: generatedUri, version: version++ },
        contentChanges: [{ text }],
      });
      if (i % 5 === 4) {
        // `checkOnSave` shells to a real `cargo check`, which reads disk,
        // not the `didChange` overlay (see `flycheckDirect`'s own note);
        // write the edit to disk first so a scripted save session
        // actually triggers real flycheck work, not a no-op check of
        // unchanged content.
        writeFileSync(generatedPath, text);
        proc.notify("textDocument/didSave", { textDocument: { uri: generatedUri } });
      }
    },
  };
}

async function startOutouLspSession(root, args) {
  const mainRsxPath = resolve(root, "src/main.rsx");
  const mainUri = pathToFileURL(mainRsxPath).href;
  const originalText = readFileSync(mainRsxPath, "utf8");
  const env = { ...process.env, OUTOU_RUST_ANALYZER: resolve(args.ra) };
  const proc = new LspProcess(args["outou-lsp"], [], env);
  await proc.request("initialize", {
    processId: process.pid,
    capabilities: capabilities(),
    workspaceFolders: [{ uri: pathToFileURL(root).href, name: "measure-a1-rss" }],
    initializationOptions: {},
  });
  proc.notify("initialized", {});
  proc.notify("textDocument/didOpen", {
    textDocument: { uri: mainUri, languageId: "outou-rsx", version: 1, text: originalText },
  });
  const anchor = warmupAnchor(originalText);
  const hoverPos = positionOf(originalText, anchor.needle);
  const hoverParams = {
    textDocument: { uri: mainUri },
    position: { line: hoverPos.line, character: hoverPos.character + anchor.offset },
  };
  let version = 2;
  return {
    proc,
    settle: (timeoutMs) => settleSession(proc, hoverParams, timeoutMs, { requireQuiescent: false }),
    edit: async (i) => {
      const text = i % 2 === 0 ? `${originalText}\n// measure-a1 edit ${i}\n` : originalText;
      proc.notify("textDocument/didChange", {
        textDocument: { uri: mainUri, version: version++ },
        contentChanges: [{ text }],
      });
      if (i % 5 === 4) {
        // `outou-lsp`'s own `didSave` re-plans from disk (see
        // `flycheckThroughOutouLsp`'s own note) — write first.
        writeFileSync(mainRsxPath, text);
        proc.notify("textDocument/didSave", { textDocument: { uri: mainUri } });
      }
    },
  };
}

/// Finds `pid`'s first child process (rust-analyzer, spawned directly by
/// `outou-lsp`) via `pgrep -P`, retrying until it appears or `timeoutMs`
/// elapses — `outou-lsp` spawns it only after planning/generating the
/// crate, which is not instantaneous.
async function findChildPid(pid, timeoutMs) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const out = execSync(`pgrep -P ${pid}`, { stdio: ["ignore", "pipe", "ignore"] })
        .toString()
        .trim();
      if (out) return Number(out.split("\n")[0]);
    } catch {
      // pgrep exits non-zero when no children match yet; keep polling.
    }
    await sleep(200);
  }
  throw new Error(`findChildPid: outou-lsp (pid ${pid}) had no rust-analyzer child after ${timeoutMs}ms`);
}

/// One `ps -A` snapshot parsed into `Map<pid, {ppid, rssKb, cpuSeconds}>`.
function readProcessTable() {
  const out = execFileSync("ps", ["-A", "-o", "pid=,ppid=,rss=,time="], { stdio: ["ignore", "pipe", "ignore"] }).toString();
  const table = new Map();
  for (const line of out.split("\n")) {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 4) continue;
    table.set(Number(parts[0]), {
      ppid: Number(parts[1]),
      rssKb: Number(parts[2]),
      cpuSeconds: parseCpuTime(parts[3]),
    });
  }
  return table;
}

/// `root` and every transitive descendant present in `table`.
function descendantPids(table, root) {
  const children = new Map();
  for (const [pid, info] of table) {
    if (!children.has(info.ppid)) children.set(info.ppid, []);
    children.get(info.ppid).push(pid);
  }
  const found = [];
  const stack = [root];
  while (stack.length > 0) {
    const pid = stack.pop();
    if (!table.has(pid)) continue;
    found.push(pid);
    stack.push(...(children.get(pid) ?? []));
  }
  return found;
}

const FOOTPRINT_UNIT_KB = { B: 1 / 1024, KB: 1, MB: 1024, GB: 1024 * 1024 };

/// macOS physical footprint (KiB) per pid via one `footprint -p ...` call.
/// A pid that exited between the `ps` snapshot and this call makes
/// `footprint` exit non-zero, but it still prints the surviving pids, so
/// stdout is parsed regardless of the exit status. Empty map if the tool
/// is unavailable.
function readFootprints(pids) {
  const result = new Map();
  if (pids.length === 0) return result;
  const run = spawnSync("footprint", pids.flatMap((p) => ["-p", String(p)]), {
    stdio: ["ignore", "pipe", "ignore"],
  });
  if (run.error || !run.stdout) return result;
  for (const match of run.stdout.toString().matchAll(/\[(\d+)\]:.*?Footprint:\s*([\d.]+)\s*(B|KB|MB|GB)\b/g)) {
    result.set(Number(match[1]), Number(match[2]) * FOOTPRINT_UNIT_KB[match[3]]);
  }
  return result;
}

/// One sample for every entry. Entries whose root has exited report nulls.
function readEntries(entries) {
  let table;
  try {
    table = readProcessTable();
  } catch {
    return Object.fromEntries(entries.map((e) => [e.label, nullSample()]));
  }
  const pidsByLabel = new Map(
    entries.map((e) => [e.label, e.tree ? descendantPids(table, e.pid) : table.has(e.pid) ? [e.pid] : []]),
  );
  const footprints = readFootprints([...new Set([...pidsByLabel.values()].flat())]);
  const out = {};
  for (const e of entries) {
    const pids = pidsByLabel.get(e.label);
    if (pids.length === 0) {
      out[e.label] = nullSample();
      continue;
    }
    const known = pids.filter((p) => footprints.has(p));
    out[e.label] = {
      rssKb: pids.reduce((a, p) => a + table.get(p).rssKb, 0),
      footprintKb: known.length > 0 ? Math.round(known.reduce((a, p) => a + footprints.get(p), 0)) : null,
      cpuSeconds: Number(pids.reduce((a, p) => a + (table.get(p).cpuSeconds ?? 0), 0).toFixed(2)),
      processCount: pids.length,
    };
  }
  return out;
}

function nullSample() {
  return { rssKb: null, footprintKb: null, cpuSeconds: null, processCount: 0 };
}

function parseCpuTime(text) {
  if (!text) return null;
  const [daysPart, rest] = text.includes("-") ? text.split("-") : [null, text];
  const parts = rest.split(":").map(Number);
  let seconds = 0;
  for (const part of parts) seconds = seconds * 60 + part;
  if (daysPart) seconds += Number(daysPart) * 86400;
  return seconds;
}

/// Load average and swap/compressor state, recorded around every run so a
/// result can be judged against how loaded the machine was.
function machineState() {
  const read = (cmd, args) => {
    try {
      return execFileSync(cmd, args, { stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
    } catch {
      return null;
    }
  };
  return {
    loadAverage: loadavg().map((v) => Number(v.toFixed(2))),
    swapUsage: read("sysctl", ["-n", "vm.swapusage"]),
    compressorMode: read("sysctl", ["-n", "vm.compressor_mode"]),
  };
}

// ---------------------------------------------------------------------

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
    if (!args.help && args.mode) validateArgs(args);
  } catch (err) {
    console.error(`measure-a1: ${err.message}`);
    process.exit(2);
  }
  if (args.help || !args.mode) {
    console.log(usage());
    process.exit(args.help ? 0 : 2);
  }
  const modes = { coldinit: modeColdinit, flycheck: modeFlycheck, completion: modeCompletion, rss: modeRss };
  const machineBefore = machineState();
  const result = await modes[args.mode](args);
  console.log(JSON.stringify({ ...result, machineState: { before: machineBefore, after: machineState() } }));
}

main().catch((err) => {
  console.error(err instanceof Error ? err.stack : err);
  process.exit(1);
});
