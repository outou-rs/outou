"use strict";
// Child-process helper for extension.test.js: builds a LanguageClient with a
// PLAIN OutputChannel (what extension.js 0.0.1 passed) against the real
// library and prints the message start() rejects with. Runs in its own process
// because the library keeps logging from background callbacks after start()
// has failed, which would surface as stray unhandled rejections in the runner.

const { createVscodeMock, installVscodeMock } = require("./vscode-mock");

const mock = createVscodeMock();
installVscodeMock(mock.vscode);
const { LanguageClient } = require("vscode-languageclient/node");

const plain = mock.vscode.window.createOutputChannel("Outou Language Server");
const client = new LanguageClient(
  "outou",
  "Outou Language Server",
  { command: process.execPath, args: ["-e", "process.exit(0)"] },
  { documentSelector: [{ language: "outou-rsx" }], outputChannel: plain },
);
process.on("unhandledRejection", () => {});
client.start().then(
  () => {
    console.log(JSON.stringify({ rejected: false }));
    process.exit(0);
  },
  (error) => {
    console.log(JSON.stringify({ rejected: true, message: String(error), hasError: typeof plain.error }));
    process.exit(0);
  },
);
