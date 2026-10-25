"use strict";
// Pure resolution of the `outou-lsp` binary path. No `vscode` import, so it
// can be unit-tested with `node --test` (server-path.test.js).

const path = require("node:path");

/** Directories (nearest first) from `start` up to the filesystem root. */
function ancestors(start) {
  const out = [];
  let dir = path.resolve(start);
  for (;;) {
    out.push(dir);
    const parent = path.dirname(dir);
    if (parent === dir) return out;
    dir = parent;
  }
}

function binaryName(platform) {
  return platform === "win32" ? "outou-lsp.exe" : "outou-lsp";
}

/**
 * Candidate paths, in priority order, when `outou.server.path` is empty:
 * the repository this extension lives in (`<ext>/../../target/{debug,release}`),
 * then a `target/{debug,release}` found in any ancestor of a workspace folder.
 */
function defaultCandidates({ extensionPath, workspaceFolders, platform }) {
  const name = binaryName(platform);
  const roots = [path.resolve(extensionPath, "..", "..")];
  for (const folder of workspaceFolders) roots.push(...ancestors(folder));
  const seen = new Set();
  const out = [];
  for (const root of roots) {
    for (const profile of ["debug", "release"]) {
      const candidate = path.join(root, "target", profile, name);
      if (!seen.has(candidate)) {
        seen.add(candidate);
        out.push(candidate);
      }
    }
  }
  return out;
}

/**
 * @returns {{ok: true, path: string} | {ok: false, message: string}}
 */
function resolveServerPath({
  configured,
  extensionPath,
  workspaceFolders = [],
  platform = process.platform,
  exists,
}) {
  const setting = typeof configured === "string" ? configured.trim() : "";
  if (setting !== "") {
    const candidates = path.isAbsolute(setting)
      ? [setting]
      : workspaceFolders.map((folder) => path.resolve(folder, setting));
    const found = candidates.find((candidate) => exists(candidate));
    if (found !== undefined) return { ok: true, path: found };
    const tried = candidates.length > 0 ? candidates.join(", ") : setting;
    return {
      ok: false,
      message: `outou.server.path is set to "${setting}" but no file exists there (tried: ${tried}).`,
    };
  }
  const candidates = defaultCandidates({ extensionPath, workspaceFolders, platform });
  const found = candidates.find((candidate) => exists(candidate));
  if (found !== undefined) return { ok: true, path: found };
  return {
    ok: false,
    message:
      "Could not find the outou-lsp binary. Run `cargo build -p outou-lsp` in the outou repository, " +
      "or set `outou.server.path` to the binary. Looked in: " +
      candidates.join(", "),
  };
}

module.exports = { resolveServerPath, defaultCandidates, binaryName };
