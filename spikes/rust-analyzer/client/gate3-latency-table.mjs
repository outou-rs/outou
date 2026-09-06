#!/usr/bin/env node
// Mechanically generates Gate 3's latency table from the raw artifacts
// (issue #9 Gate 3 review, M4): both `docs/gate3-results.md` and
// `docs/phase0/issues/09-integrated-lsp.md` used to carry a
// hand-transcribed latency table each, and neither matched the
// `spikes/rust-analyzer/results/gate3-*.json.gz` artifacts they both
// claimed to be drawn from (17 of 19 rows disagreed, two by a factor of
// 5). This script removes the transcription step entirely: it reads the
// artifacts of one `cargo test -p outou-lsp --test gate3 -- --ignored
// --nocapture` run and prints the exact markdown table both documents
// paste in verbatim, so the numbers can never drift from the evidence
// again — re-run this script after any gate3 run and re-paste its output
// into both docs (this file's own module doc comment, and a "generated
// by" note in both, say so).
//
// Usage:
//
//   node spikes/rust-analyzer/client/gate3-latency-table.mjs
//
// Reads every `spikes/rust-analyzer/results/gate3-*.json.gz` relative to
// the repository root (this file's own location, two directories up) and
// prints a GitHub-flavored markdown table to stdout. Exits non-zero (with
// a message on stderr) if an artifact this table needs is missing —
// never silently prints a partial or stale table.

import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");
const resultsDir = join(repoRoot, "spikes/rust-analyzer/results");

/// One row of the table: `file` is the `gate3-<file>.json.gz` artifact,
/// `key` is the field under its `latencyMs` object, `label` is the
/// human-readable row label. Order here is the order printed.
const ROWS = [
  {
    file: "progress-before-hover",
    key: "hover",
    label: "hover, after a `$/progress` `end` was forwarded (S6/L12)",
  },
  { file: "hover-user", key: "hover", label: "hover (`user`)" },
  { file: "hover-nonascii", key: "hover", label: "hover (non-ASCII prefix)" },
  {
    file: "hover-element-tag",
    key: "hover",
    label: "hover (element tag, sanitized to `null`)",
  },
  {
    file: "hover-closing-tag",
    key: "hover",
    label: "hover (closing tag, sanitized to `null`)",
  },
  {
    file: "hover-props-named-type",
    key: "hover",
    label: "hover (`…Props`-named user type)",
  },
  {
    file: "definition-load-user",
    key: "definition",
    label: "definition (`load_user`, same file)",
  },
  {
    file: "definition-user-card",
    key: "definition",
    label: "definition (`UserCard`, cross-file)",
  },
  {
    file: "completion-member",
    key: "completion",
    label: "completion (member, `user.`)",
  },
  {
    file: "completion-tag-component",
    key: "completion",
    label: "completion (tag, component)",
  },
  {
    file: "completion-tag-element",
    key: "completion",
    label: "completion (tag, element)",
  },
  {
    file: "completion-closing-tag",
    key: "completion",
    label: "completion (closing tag)",
  },
  {
    file: "completion-prop-name",
    key: "completion",
    label: "completion (attribute name)",
  },
  {
    file: "completion-attr-value",
    key: "completion",
    label: "completion (attribute value, sanitized to empty)",
  },
  {
    file: "completion-prop-value",
    key: "completion",
    label: "completion (prop value, `user={us}`)",
  },
  {
    file: "diagnostic-type-error",
    key: "didSaveToDiagnostics",
    label: "`didSave` -> mismatched-types diagnostic",
  },
  {
    file: "diagnostic-syntax-error",
    key: "didChangeToDiagnostics",
    label: "`didChange` -> Outou syntax diagnostic",
  },
  {
    file: "diagnostic-syntax-error",
    key: "definitionDespiteSyntaxError",
    label: "definition after an unrelated syntax error",
  },
  {
    file: "diagnostic-missing-prop",
    key: "missingPropToDiagnostics",
    label: "`didSave` -> missing-required-prop diagnostic (M5)",
  },
  {
    file: "stale-diagnostics-cleared",
    key: "typeErrorIntroduced",
    label: "type error introduced -> published, before the M6 revert",
  },
  {
    file: "save-with-syntax-error",
    key: "saveWithSyntaxErrorDiagnostics",
    label: "`didSave` -> Outou diagnostic, syntax error (M1 save probe)",
  },
  {
    file: "startup-broken-source",
    key: "startupDiagnostics",
    label: "startup -> Outou diagnostic, broken source at launch (M1)",
  },
];

function readArtifact(name) {
  const path = join(resultsDir, `gate3-${name}.json.gz`);
  let raw;
  try {
    raw = readFileSync(path);
  } catch (e) {
    throw new Error(`missing gate3 artifact ${path} (run the gate3 test first): ${e.message}`);
  }
  return JSON.parse(gunzipSync(raw).toString("utf8"));
}

const artifacts = new Map();
function artifact(name) {
  if (!artifacts.has(name)) artifacts.set(name, readArtifact(name));
  return artifacts.get(name);
}

// The `initialize` handshake cost is the same mechanism in every probe
// (this server's own setup, not full rust-analyzer indexing); reported as
// one range across every artifact rather than once per row.
const uniqueFiles = [...new Set(ROWS.map((row) => row.file))];
const initValues = uniqueFiles
  .map((file) => artifact(file).latencyMs?.initialize)
  .filter((v) => typeof v === "number");
const initMin = Math.min(...initValues);
const initMax = Math.max(...initValues);

const lines = [];
lines.push("| Request | Latency (ms) |");
lines.push("|---|---|");
lines.push(`| \`initialize\` (client <-> outou-lsp) | ${initMin}-${initMax} (per-probe) |`);
for (const row of ROWS) {
  const data = artifact(row.file);
  const value = data.latencyMs?.[row.key];
  if (typeof value !== "number") {
    throw new Error(`gate3-${row.file}.json.gz has no latencyMs.${row.key}`);
  }
  lines.push(`| ${row.label} | ${value} |`);
}

console.log(lines.join("\n"));
