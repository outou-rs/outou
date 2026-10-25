"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const { resolveServerPath, binaryName } = require("./server-path");

const ext = path.resolve("/repo/packages/vscode-outou");
const existsOnly = (...files) => (p) => files.map((f) => path.resolve(f)).includes(p);

test("prefers the repository next to the extension when the setting is empty", () => {
  const debug = path.resolve("/repo/target/debug/outou-lsp");
  const r = resolveServerPath({
    configured: "",
    extensionPath: ext,
    workspaceFolders: ["/elsewhere/app"],
    platform: "linux",
    exists: existsOnly(debug),
  });
  assert.deepEqual(r, { ok: true, path: debug });
});

test("finds target/debug in an ancestor of the workspace folder", () => {
  const debug = path.resolve("/other/target/debug/outou-lsp");
  const r = resolveServerPath({
    configured: undefined,
    extensionPath: path.resolve("/nowhere/ext"),
    workspaceFolders: ["/other/examples/dogfood-board"],
    platform: "linux",
    exists: existsOnly(debug),
  });
  assert.deepEqual(r, { ok: true, path: debug });
});

test("falls back to the release build when no debug build exists", () => {
  const release = path.resolve("/repo/target/release/outou-lsp");
  const r = resolveServerPath({
    configured: "",
    extensionPath: ext,
    platform: "linux",
    exists: existsOnly(release),
  });
  assert.equal(r.ok && r.path, release);
});

test("an absolute setting wins over the defaults", () => {
  const custom = path.resolve("/opt/outou-lsp");
  const r = resolveServerPath({
    configured: custom,
    extensionPath: ext,
    platform: "linux",
    exists: existsOnly(custom, "/repo/target/debug/outou-lsp"),
  });
  assert.deepEqual(r, { ok: true, path: custom });
});

test("a relative setting resolves against workspace folders", () => {
  const bin = path.resolve("/ws/bin/outou-lsp");
  const r = resolveServerPath({
    configured: "bin/outou-lsp",
    extensionPath: ext,
    workspaceFolders: ["/ws"],
    platform: "linux",
    exists: existsOnly(bin),
  });
  assert.deepEqual(r, { ok: true, path: bin });
});

test("a missing configured path is an error naming the setting, not a silent fallback", () => {
  const r = resolveServerPath({
    configured: "/nope/outou-lsp",
    extensionPath: ext,
    platform: "linux",
    exists: existsOnly("/repo/target/debug/outou-lsp"),
  });
  assert.equal(r.ok, false);
  assert.match(r.message, /outou\.server\.path/);
  assert.match(r.message, /\/nope\/outou-lsp/);
});

test("nothing found yields an actionable message", () => {
  const r = resolveServerPath({ configured: "", extensionPath: ext, platform: "linux", exists: () => false });
  assert.equal(r.ok, false);
  assert.match(r.message, /cargo build -p outou-lsp/);
  assert.match(r.message, /outou\.server\.path/);
});

test("uses the .exe name on Windows", () => {
  assert.equal(binaryName("win32"), "outou-lsp.exe");
  assert.equal(binaryName("darwin"), "outou-lsp");
});
