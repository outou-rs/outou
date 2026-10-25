"use strict";
// A minimal stand-in for the `vscode` module, installed with
// `installVscodeMock()` so that extension.js AND the real vscode-languageclient
// in node_modules resolve `require("vscode")` to it. It exists to catch the
// "only fails when run inside VS Code" class of mistakes (wrong channel type,
// wrong options shape), not to emulate an editor.
//
// Behaviours that matter and mirror real VS Code:
//  - createOutputChannel(name) returns a plain OutputChannel (no error/warn/...,
//    no logLevel); only createOutputChannel(name, { log: true }) returns a
//    LogOutputChannel. This is what made outou 0.0.1 fail on start.

const Module = require("node:module");

const LOG_LEVEL = { Off: 0, Trace: 1, Debug: 2, Info: 3, Warning: 4, Error: 5 };

const DATA_CLASSES = [
  "CallHierarchyIncomingCall", "CallHierarchyItem", "CallHierarchyOutgoingCall", "CodeAction",
  "CodeLens", "Color", "ColorInformation", "ColorPresentation", "CompletionItem", "CompletionList",
  "Diagnostic", "DiagnosticRelatedInformation", "DocumentHighlight", "DocumentLink", "DocumentSymbol",
  "FoldingRange", "Hover", "InlayHint", "InlayHintLabelPart", "InlineCompletionItem",
  "InlineCompletionList", "InlineValueEvaluatableExpression", "InlineValueText",
  "InlineValueVariableLookup", "LinkedEditingRanges", "Location", "MarkdownString",
  "ParameterInformation", "RelativePattern", "SelectionRange", "SemanticTokens", "SemanticTokensEdit",
  "SemanticTokensEdits", "SignatureHelp", "SignatureInformation", "SnippetString", "SnippetTextEdit",
  "SymbolInformation", "TextEdit", "TypeHierarchyItem", "WorkspaceEdit",
];

const ENUMS = [
  "CodeActionTriggerKind", "CompletionItemKind", "CompletionItemTag", "CompletionTriggerKind",
  "DiagnosticSeverity", "DiagnosticTag", "DocumentHighlightKind", "FileType", "FoldingRangeKind",
  "InlineCompletionTriggerKind", "NotebookCellKind", "SignatureHelpTriggerKind", "SymbolKind",
  "SymbolTag", "TextDocumentSaveReason",
];

function event() {
  return () => ({ dispose() {} });
}

function createPlainChannel(name, lines) {
  return {
    name,
    append: (v) => lines.push(String(v)),
    appendLine: (v) => lines.push(String(v)),
    replace() {},
    clear() {},
    show() {},
    hide() {},
    dispose() {},
  };
}

function createLogChannel(name, lines) {
  const record = (level) => (message) => lines.push(`[${level}] ${message}`);
  return {
    ...createPlainChannel(name, lines),
    logLevel: LOG_LEVEL.Info,
    onDidChangeLogLevel: event(),
    trace: record("trace"),
    debug: record("debug"),
    info: record("info"),
    warn: record("warn"),
    error: record("error"),
  };
}

/** Anything the library registers (`languages.register*Provider`, ...) is a no-op Disposable. */
function noopRegistrations(base) {
  return new Proxy(base, {
    get(target, prop) {
      if (prop in target) return target[prop];
      if (typeof prop === "string" && /^(register|on)/.test(prop)) {
        return () => ({ dispose() {} });
      }
      return undefined;
    },
  });
}

class Disposable {
  constructor(fn) {
    this.fn = fn;
  }
  dispose() {
    this.fn?.();
  }
  static from(...items) {
    return new Disposable(() => items.forEach((i) => i.dispose?.()));
  }
}

class EventEmitter {
  constructor() {
    this.listeners = [];
    this.event = (listener) => {
      this.listeners.push(listener);
      return new Disposable(() => {
        this.listeners = this.listeners.filter((l) => l !== listener);
      });
    };
  }
  fire(value) {
    this.listeners.forEach((l) => l(value));
  }
  dispose() {
    this.listeners = [];
  }
}

const Uri = {
  file: (fsPath) => ({ scheme: "file", fsPath, path: fsPath, toString: () => `file://${fsPath}` }),
  parse: (value) => ({ scheme: "file", fsPath: value, path: value, toString: () => value }),
};

/**
 * @param {{ settings?: Record<string, unknown>, workspaceFolders?: string[] }} options
 * @returns {{ vscode: object, lines: string[], shown: string[], channels: {name: string, log: boolean}[] }}
 */
function createVscodeMock({ settings = {}, workspaceFolders = [] } = {}) {
  const lines = [];
  const shown = [];
  const channels = [];
  const show = (kind) => async (message) => {
    shown.push(`${kind}: ${message}`);
  };
  const vscode = {
    version: "1.91.0",
    LogLevel: LOG_LEVEL,
    ProgressLocation: { Window: 10, Notification: 15, SourceControl: 1 },
    Disposable,
    EventEmitter,
    Uri,
    Range: class Range {},
    Position: class Position {},
    CancellationError: class CancellationError extends Error {},
    CancellationTokenSource: class CancellationTokenSource {
      token = { isCancellationRequested: false, onCancellationRequested: event() };
      cancel() {}
      dispose() {}
    },
    TabInputText: class TabInputText {},
    TabInputTextDiff: class TabInputTextDiff {},
    TabInputCustom: class TabInputCustom {},
    TabInputNotebook: class TabInputNotebook {},
    env: { language: "en", appName: "mock", openExternal: async () => true },
    commands: noopRegistrations({}),
    languages: noopRegistrations({
      match: () => 0,
      createDiagnosticCollection: () => ({
        set() {},
        delete() {},
        clear() {},
        dispose() {},
        forEach() {},
        get: () => undefined,
        has: () => false,
      }),
    }),
    workspace: noopRegistrations({
      getConfiguration: (section) => ({
        get: (key, fallback) => {
          const full = section ? `${section}.${key}` : key;
          return full in settings ? settings[full] : fallback;
        },
        has: (key) => (section ? `${section}.${key}` : key) in settings,
        inspect: () => undefined,
        update: async () => {},
      }),
      workspaceFolders: workspaceFolders.map((fsPath, index) => ({
        uri: Uri.file(fsPath),
        name: fsPath,
        index,
      })),
      textDocuments: [],
      notebookDocuments: [],
      createFileSystemWatcher: () => ({
        onDidCreate: event(),
        onDidChange: event(),
        onDidDelete: event(),
        dispose() {},
      }),
    }),
    window: noopRegistrations({
      createOutputChannel: (name, options) => {
        const log = options !== undefined && typeof options === "object" && options.log === true;
        channels.push({ name, log });
        return log ? createLogChannel(name, lines) : createPlainChannel(name, lines);
      },
      showErrorMessage: show("error"),
      showWarningMessage: show("warning"),
      showInformationMessage: show("info"),
      withProgress: async (_options, task) => task({ report() {} }),
      tabGroups: { all: [], onDidChangeTabs: event() },
      visibleTextEditors: [],
      activeTextEditor: undefined,
    }),
  };
  // The library subclasses/instantiates many data classes (CompletionItem,
  // CodeLens, Diagnostic, ...) only to convert protocol values. It reads them
  // through `__importStar(require("vscode"))`, which copies own enumerable
  // properties, so they must be real properties (a Proxy would not do).
  for (const name of DATA_CLASSES) {
    vscode[name] ??= class {
      constructor(...args) {
        Object.assign(this, { args });
      }
    };
  }
  // Enums: any member resolves to a stable number (`Kind.Foo`), so the
  // library's lookup tables build; CodeActionKind members are objects with append().
  for (const name of ENUMS) {
    const members = new Map();
    vscode[name] = new Proxy({}, {
      get: (_t, member) => {
        if (typeof member !== "string") return undefined;
        if (!members.has(member)) members.set(member, members.size + 1);
        return members.get(member);
      },
    });
  }
  vscode.CodeActionKind = new Proxy({}, {
    get: (_t, member) => {
      const make = (value) => ({ value, append: (part) => make(`${value}.${part}`), contains: () => false });
      return typeof member === "string" ? make(member) : undefined;
    },
  });
  const withStubs = vscode;
  return { vscode: withStubs, lines, shown, channels };
}

/**
 * Makes `require("vscode")` return `vscode` until the returned function is
 * called. Node's own resolution cannot find a `vscode` package, so intercept
 * Module._load for that one specifier.
 */
function installVscodeMock(vscode) {
  const originalLoad = Module._load;
  Module._load = function load(request, ...rest) {
    if (request === "vscode") return vscode;
    return originalLoad.call(this, request, ...rest);
  };
  return () => {
    Module._load = originalLoad;
  };
}

module.exports = { createVscodeMock, installVscodeMock, LOG_LEVEL };
