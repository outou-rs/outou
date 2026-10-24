// Tests for `measure-a1.mjs` argument validation.
//
// Run with: node --test spikes/rust-analyzer/client/*.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { parseArgs, validateArgs } from "./measure-a1-args.mjs";

const SITES = ["large-enum-path", "small-path"];
const base = ["--mode", "stall", "--root", "/r", "--ra", "/ra", "--outou-lsp", "/lsp"];
const check = (extra = []) => validateArgs(parseArgs([...base, ...extra]), SITES);

test("a complete stall invocation validates", () => {
  assert.doesNotThrow(() => check(["--n", "2", "--window-ms", "3000", "--request-timeout-ms", "5000"]));
});

test("an unknown flag (a typo) is rejected by name", () => {
  assert.throws(() => check(["--window-m", "3000"]), /unknown flag --window-m/);
  assert.throws(() => check(["--request-timeout", "5"]), /unknown flag --request-timeout/);
});

test("--request-timeout-ms must be a positive integer", () => {
  assert.throws(() => check(["--request-timeout-ms", "0"]), /positive integer/);
});

test("--log-dir must be an existing directory", () => {
  assert.doesNotThrow(() => check(["--log-dir", tmpdir()]));
  assert.throws(() => check(["--log-dir", "/nonexistent/measure-a1-logs"]), /not an existing directory/);
});

test("a bad value for a known flag is still rejected", () => {
  assert.throws(() => check(["--via", "sideways"]), /unknown --via/);
  assert.throws(() => check(["--site", "nope"]), /unknown --site/);
});
