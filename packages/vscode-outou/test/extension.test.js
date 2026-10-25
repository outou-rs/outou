"use strict";
// Runs the real extension.js against the real vscode-languageclient with a
// mocked `vscode` module (test/vscode-mock.js), and starts the real outou-lsp
// over stdio. Needs `cargo build -p outou-lsp` (or OUTOU_LSP_BIN); that test is
// skipped with a message when the binary is missing.
//
// Not covered here: anything that needs the editor itself (activation events,
// Output panel rendering, the log-level picker, real document sync, UI toasts).

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { createVscodeMock, installVscodeMock } = require("./vscode-mock");

const extensionPath = path.resolve(__dirname, "..");
const serverBinary =
  process.env.OUTOU_LSP_BIN ??
  path.resolve(extensionPath, "..", "..", "target", "debug", process.platform === "win32" ? "outou-lsp.exe" : "outou-lsp");
const hasServer = fs.existsSync(serverBinary);

/** Loads extension.js + the real library fresh, with `vscode` mocked. */
function loadExtension(mock) {
  for (const key of Object.keys(require.cache)) {
    if (key.startsWith(extensionPath + path.sep) && !key.includes(`${path.sep}test${path.sep}`)) {
      delete require.cache[key];
    }
  }
  const uninstall = installVscodeMock(mock.vscode);
  try {
    return {
      extension: require("../extension.js"),
      languageclient: require("vscode-languageclient/node"),
      uninstall,
    };
  } catch (error) {
    uninstall();
    throw error;
  }
}

/** Records the LanguageClient that extension.js builds, without changing it. */
function captureClient(languageclient) {
  const captured = { client: undefined };
  const originalStart = languageclient.LanguageClient.prototype.start;
  languageclient.LanguageClient.prototype.start = function start(...args) {
    captured.client = this;
    return originalStart.apply(this, args);
  };
  captured.restore = () => {
    languageclient.LanguageClient.prototype.start = originalStart;
  };
  return captured;
}

test("manifest: engines.vscode covers LogOutputChannel (1.74+) and the library's requirement", () => {
  const manifest = require("../package.json");
  const lib = JSON.parse(
    fs.readFileSync(path.join(extensionPath, "node_modules", "vscode-languageclient", "package.json"), "utf8"),
  );
  assert.match(manifest.engines.vscode, /^\^1\.(\d+)\./);
  assert.ok(Number(/^\^1\.(\d+)\./.exec(manifest.engines.vscode)[1]) >= 91);
  assert.equal(manifest.dependencies["vscode-languageclient"], lib.version, "pinned exactly");
});

test("a plain OutputChannel reproduces the 0.0.1 failure (X is not a function)", () => {
  const run = spawnSync(process.execPath, [path.join(__dirname, "plain-channel-repro.js")], {
    cwd: extensionPath,
    encoding: "utf8",
    timeout: 20000,
  });
  assert.equal(run.status, 0, run.stderr);
  const result = JSON.parse(run.stdout.trim().split("\n").pop());
  assert.equal(result.hasError, "undefined", "a plain OutputChannel has no error()");
  assert.equal(result.rejected, true);
  assert.match(result.message, /this\.outputChannel\.\w+ is not a function/);
});

test("activate() asks VS Code for a LogOutputChannel and surfaces start failures through it", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "outou-ext-"));
  // Exists (so the path resolves) but is not executable: spawn fails with EACCES.
  const failing = path.join(dir, "outou-lsp");
  fs.writeFileSync(failing, "not a program\n", { mode: 0o644 });
  const mock = createVscodeMock({ settings: { "outou.server.path": failing }, workspaceFolders: [dir] });
  const { extension, languageclient, uninstall } = loadExtension(mock);
  const captured = captureClient(languageclient);
  try {
    const subscriptions = [];
    await extension.activate({ extensionPath, subscriptions });
    assert.deepEqual(mock.channels, [{ name: "Outou Language Server", log: true }]);
    assert.equal(subscriptions.length, 1, "the channel is disposed with the extension");
    assert.ok(mock.lines.some((l) => l.startsWith("[info] Starting ")), mock.lines.join("\n"));
    assert.ok(mock.lines.some((l) => l.startsWith("[error] Failed to start:")), mock.lines.join("\n"));
    assert.ok(mock.shown.some((m) => m.startsWith("error: Outou: failed to start outou-lsp")), mock.shown.join("\n"));
    assert.ok(!mock.lines.some((l) => /is not a function/.test(l)), mock.lines.join("\n"));
    await extension.deactivate(); // must not throw for a client that never ran
  } finally {
    captured.restore();
    uninstall();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("activate() reports a missing server binary without creating a client", async () => {
  const mock = createVscodeMock({ settings: { "outou.server.path": "/nonexistent/outou-lsp" } });
  const { extension, uninstall } = loadExtension(mock);
  try {
    await extension.activate({ extensionPath, subscriptions: [] });
    assert.deepEqual(mock.channels, []);
    assert.match(mock.shown.join("\n"), /outou\.server\.path is set to/);
    await extension.deactivate();
  } finally {
    uninstall();
  }
});

test(
  "activate() starts the real outou-lsp over stdio and gets an initialize response",
  { skip: hasServer ? false : `build it first: cargo build -p outou-lsp (looked for ${serverBinary})` },
  async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "outou-ext-"));
    const mock = createVscodeMock({
      settings: { "outou.server.path": serverBinary },
      workspaceFolders: [dir],
    });
    const { extension, languageclient, uninstall } = loadExtension(mock);
    const captured = captureClient(languageclient);
    try {
      await extension.activate({ extensionPath, subscriptions: [] });
      const { client } = captured;
      assert.ok(client, "extension.js constructed and started a LanguageClient");
      assert.ok(client.isRunning(), `not running; log:\n${mock.lines.join("\n")}\nshown: ${mock.shown.join("\n")}`);
      assert.ok(client.initializeResult, "server answered initialize");
      assert.ok(client.initializeResult.capabilities, "capabilities present");
      assert.ok(!mock.shown.some((m) => m.startsWith("error:")), mock.shown.join("\n"));
      // The server's own stderr lands in the channel as [error]; only the client's
      // own failures matter here.
      assert.ok(
        !mock.lines.some((l) => l.startsWith("[error]") && !l.startsWith("[error] outou-lsp:")),
        mock.lines.join("\n"),
      );
      await extension.deactivate();
      assert.ok(!client.isRunning());
    } finally {
      captured.restore();
      uninstall();
      fs.rmSync(dir, { recursive: true, force: true });
    }
  },
);
