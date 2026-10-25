"use strict";
// Development-only LSP client for outou-lsp (issue F17). Plain CommonJS, no
// build step. Client capabilities are deliberately the library defaults: the
// point is to see what a real editor negotiates with the server.

const fs = require("node:fs");
const readline = require("node:readline");
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");
const { resolveServerPath } = require("./server-path");

// The library's default shows every line of the server's stderr at [error], so
// a notice such as "running in degraded mode" looks like a failure. outou-lsp
// writes progress and degraded-mode notices there, not only errors, so show
// both streams at [info]. (rust-analyzer's own stderr never gets this far
// unless OUTOU_LSP_RA_LOG is set for the server; see crates/outou-lsp/README.md.)
const serverStdioOptions = {
  stdout: (input, channel) => pipeLines(input, (line) => channel.info(line)),
  stderr: (input, channel) => pipeLines(input, (line) => channel.info(line)),
};

function pipeLines(input, onLine) {
  readline.createInterface({ input, crlfDelay: Infinity, terminal: false, historySize: 0 }).on("line", onLine);
}

/** @type {LanguageClient | undefined} */
let client;

async function activate(context) {
  const config = vscode.workspace.getConfiguration("outou");
  const resolved = resolveServerPath({
    configured: config.get("server.path"),
    extensionPath: context.extensionPath,
    workspaceFolders: (vscode.workspace.workspaceFolders ?? []).map((f) => f.uri.fsPath),
    exists: (candidate) => fs.existsSync(candidate),
  });
  if (!resolved.ok) {
    void vscode.window.showErrorMessage(`Outou: ${resolved.message}`);
    return;
  }

  // vscode-languageclient 10 logs through `LogOutputChannel` methods
  // (`error`/`warn`/`info`/`debug`/`trace`) and reads `logLevel`; a plain
  // OutputChannel has none of them and start() fails with "this.outputChannel.error
  // is not a function". `{ log: true }` is what makes VS Code create one. The same
  // channel carries the trace output (the library's default for traceOutputChannel).
  const outputChannel = vscode.window.createOutputChannel("Outou Language Server", { log: true });
  context.subscriptions.push(outputChannel);
  // No `transport`: an Executable with `TransportKind.stdio` gets `--stdio`
  // appended to its args, which outou-lsp does not take; the default is stdio.
  const serverOptions = { command: resolved.path, args: [] };
  const clientOptions = {
    documentSelector: [{ language: "outou-rsx" }],
    outputChannel,
    stdioOptions: serverStdioOptions,
  };
  client = new LanguageClient("outou", "Outou Language Server", serverOptions, clientOptions);
  outputChannel.info(`Starting ${resolved.path}`);
  try {
    await client.start();
  } catch (error) {
    outputChannel.error(`Failed to start: ${error}`);
    void vscode.window.showErrorMessage(`Outou: failed to start outou-lsp: ${error}`);
  }
}

async function deactivate() {
  const stopping = client;
  client = undefined;
  // stop() throws when the client never reached Running (e.g. start() failed).
  if (stopping !== undefined && stopping.isRunning()) await stopping.stop();
}

module.exports = { activate, deactivate };
