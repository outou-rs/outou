"use strict";
// Development-only LSP client for outou-lsp (issue F17). Plain CommonJS, no
// build step. Client capabilities are deliberately the library defaults: the
// point is to see what a real editor negotiates with the server.

const fs = require("node:fs");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { resolveServerPath } = require("./server-path");

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

  const outputChannel = vscode.window.createOutputChannel("Outou Language Server");
  const serverOptions = {
    command: resolved.path,
    args: [],
    transport: TransportKind.stdio,
    options: { env: { ...process.env } },
  };
  const clientOptions = {
    documentSelector: [{ language: "outou-rsx" }],
    outputChannel,
    traceOutputChannel: outputChannel,
  };
  client = new LanguageClient("outou", "Outou Language Server", serverOptions, clientOptions);
  outputChannel.appendLine(`Starting ${resolved.path}`);
  try {
    await client.start();
  } catch (error) {
    outputChannel.appendLine(`Failed to start: ${error}`);
    void vscode.window.showErrorMessage(`Outou: failed to start outou-lsp: ${error}`);
  }
}

async function deactivate() {
  if (client !== undefined) await client.stop();
}

module.exports = { activate, deactivate };
