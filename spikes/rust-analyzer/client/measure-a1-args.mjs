// Argument parsing and validation for `measure-a1.mjs`, split out so the
// validation is unit-testable (`measure-a1-args.test.mjs`) without running
// the harness.

export const MODE_NAMES = ["coldinit", "flycheck", "completion", "rss", "stall"];
const CONFIG_NAMES = ["single", "two-process"];
import { statSync } from "node:fs";

function isDirectory(path) {
  try {
    return statSync(path).isDirectory();
  } catch {
    return false;
  }
}

// `cold`: sample from spawn until both sessions have settled. `cold+edit`:
// the same cold window, then (in the SAME process, no restart) the scripted
// edits; the result reports the two windows separately.
const PHASE_NAMES = ["cold", "cold+edit"];

const KNOWN_FLAGS = [
  "mode", "config", "phase", "via", "autoimport", "n", "timeout", "interval-ms", "edits",
  "edit-delay-ms", "window-ms", "request-timeout-ms", "site", "root", "ra", "outou-lsp", "log-dir",
];

export function parseArgs(argv) {
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
export function validateArgs(args, siteLabels) {
  for (const name of Object.keys(args)) {
    if (name !== "help" && !KNOWN_FLAGS.includes(name)) {
      throw new Error(`unknown flag --${name}; expected one of ${KNOWN_FLAGS.map((f) => `--${f}`).join(", ")}`);
    }
  }
  requireOneOf(args, "mode", MODE_NAMES);
  requireOneOf(args, "config", CONFIG_NAMES);
  requireOneOf(args, "phase", PHASE_NAMES);
  requireOneOf(args, "via", ["direct", "outou-lsp"]);
  requireOneOf(args, "autoimport", ["true", "false"]);
  for (const name of ["n", "timeout", "interval-ms", "edits", "edit-delay-ms", "window-ms", "request-timeout-ms"]) {
    requirePositiveInteger(args, name);
  }
  if (args.site !== undefined) {
    if (args.site.trim() === "") throw new Error("--site must be non-empty");
    requireOneOf(args, "site", siteLabels);
  }
  if (args["log-dir"] !== undefined && !isDirectory(args["log-dir"])) {
    throw new Error(`--log-dir ${JSON.stringify(args["log-dir"])} is not an existing directory`);
  }
  for (const name of ["root", "ra"]) {
    if (!args[name]) throw new Error(`--${name} is required`);
  }
  const needsOutouLsp = args.mode !== "rss" || (args.config ?? "single") === "two-process";
  if (needsOutouLsp && !args["outou-lsp"]) throw new Error("--outou-lsp is required");
}

