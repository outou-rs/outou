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
// The library can hit the missing method from a background callback (the
// child's exit event) before start() settles; that surfaces as an uncaught
// exception instead of a rejection. Either one is the failure being
// reproduced, so report whichever comes first and exit.
let reported = false;
function report(result) {
  if (reported) return;
  reported = true;
  console.log(JSON.stringify({ ...result, hasError: typeof plain.error }));
  process.exit(0);
}
process.on("unhandledRejection", (error) => report({ rejected: true, message: String(error) }));
process.on("uncaughtException", (error) => report({ rejected: true, message: String(error) }));
client.start().then(
  () => report({ rejected: false }),
  (error) => report({ rejected: true, message: String(error) }),
);
