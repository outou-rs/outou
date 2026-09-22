// Structural checks for the Outou JSX TextMate grammar: valid JSON, every
// `include` resolves to a repository key or a known external scope, and
// every `match`/`begin`/`end` regex compiles under Oniguruma's JS-regex
// approximation (RegExp) — good enough to catch a typo without a real
// Oniguruma dependency or network access.
//
// Run with: node --test packages/vscode-outou/syntaxes/*.test.mjs
// (mirrors spikes/rust-analyzer/client/*.test.mjs's own convention).

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const grammarPath = fileURLToPath(
  new URL("./outou-rsx.tmLanguage.json", import.meta.url),
);

// Scopes this grammar embeds but does not define itself: the host editor
// resolves these against whatever grammar registered that scope name (the
// Rust extension, for `source.rust`).
const KNOWN_EXTERNAL_SCOPES = new Set(["source.rust"]);

function loadGrammar() {
  const text = readFileSync(grammarPath, "utf8");
  return JSON.parse(text);
}

test("the grammar file is valid JSON with the expected top-level shape", () => {
  const grammar = loadGrammar();
  assert.equal(grammar.scopeName, "source.outou-rsx");
  assert.ok(Array.isArray(grammar.patterns), "patterns must be an array");
  assert.ok(
    grammar.repository && typeof grammar.repository === "object",
    "repository must be an object",
  );
});

/** Every regex-bearing string the rule (or its begin/end captures) contains. */
function* regexStringsOf(rule) {
  for (const key of ["match", "begin", "end", "while"]) {
    if (typeof rule[key] === "string") {
      yield rule[key];
    }
  }
}

/** Every `#name`/external-scope string an `include` in `rule.patterns` names. */
function* includesOf(rule) {
  if (!Array.isArray(rule.patterns)) {
    return;
  }
  for (const child of rule.patterns) {
    if (typeof child.include === "string") {
      yield child.include;
    }
  }
}

function* allRules(grammar) {
  yield grammar;
  for (const rule of Object.values(grammar.repository ?? {})) {
    yield rule;
  }
}

test("every regex in the grammar (match/begin/end) compiles", () => {
  const grammar = loadGrammar();
  for (const rule of allRules(grammar)) {
    for (const pattern of regexStringsOf(rule)) {
      assert.doesNotThrow(
        () => new RegExp(pattern),
        `pattern failed to compile: ${pattern}`,
      );
    }
    // Capture group patterns (beginCaptures/endCaptures/captures) can carry
    // their own nested `patterns`, which is where jsx-element name tokens
    // are matched; check those too via allRules's own iteration since they
    // are visited recursively below.
  }
});

/** Collects every rule reachable from `grammar`, including capture-nested ones. */
function collectAllRules(grammar) {
  const rules = new Map();
  rules.set("$root", grammar);
  for (const [name, rule] of Object.entries(grammar.repository ?? {})) {
    rules.set(`#${name}`, rule);
  }
  return rules;
}

function* captureSubRules(rule) {
  for (const capturesKey of ["captures", "beginCaptures", "endCaptures"]) {
    const captures = rule[capturesKey];
    if (!captures || typeof captures !== "object") {
      continue;
    }
    for (const capture of Object.values(captures)) {
      if (capture && typeof capture === "object") {
        yield capture;
      }
    }
  }
}

test("every include resolves to a repository key or a known external scope", () => {
  const grammar = loadGrammar();
  const known = collectAllRules(grammar);
  const unresolved = [];

  function visit(rule) {
    for (const include of includesOf(rule)) {
      if (include === "$self" || include === "$base") {
        continue;
      }
      if (include.startsWith("#")) {
        if (!known.has(include)) {
          unresolved.push(include);
        }
        continue;
      }
      if (!KNOWN_EXTERNAL_SCOPES.has(include)) {
        unresolved.push(include);
      }
    }
    for (const sub of captureSubRules(rule)) {
      visit(sub);
    }
    if (Array.isArray(rule.patterns)) {
      for (const child of rule.patterns) {
        // Inline (non-include) pattern objects are also rules in their own
        // right (they may have their own nested `patterns`/captures).
        if (!child.include) {
          visit(child);
        }
      }
    }
  }

  visit(grammar);
  for (const [, rule] of known) {
    visit(rule);
  }

  assert.deepEqual(unresolved, [], `unresolved includes: ${unresolved.join(", ")}`);
});

test("every repository rule with begin/end also declares the other half", () => {
  const grammar = loadGrammar();
  for (const [name, rule] of Object.entries(grammar.repository ?? {})) {
    const hasBegin = typeof rule.begin === "string";
    const hasEnd = typeof rule.end === "string";
    assert.equal(
      hasBegin,
      hasEnd,
      `repository rule "${name}" must declare both begin and end, or neither`,
    );
  }
});

test("the closing-tag end pattern backreferences the opening tag's captured name", () => {
  const grammar = loadGrammar();
  const jsxElement = grammar.repository["jsx-element"];
  assert.ok(jsxElement, "jsx-element rule must exist");
  assert.match(jsxElement.end, /\\2/, "end pattern must backreference group 2 (the tag name)");
});

// --- Behavioral checks on jsx-element's own `begin`/`end` regexes
// (issue #14 review, SHOULD-LAND-9): these run the literal Oniguruma
// pattern strings from the JSON through Node's own `RegExp` (Oniguruma
// and JS RegExp agree on everything this grammar uses: lookbehind,
// lookahead, backreferences), so they exercise the exact same pattern
// text the real grammar engine would use, without needing a real
// Oniguruma/vscode-textmate dependency.

function jsxElementBeginRegExp() {
  const grammar = loadGrammar();
  return new RegExp(grammar.repository["jsx-element"].begin);
}

test("jsx-element's begin pattern does not match ordinary Rust generics/comparisons", () => {
  const begin = jsxElementBeginRegExp();
  const mustNotMatch = [
    "let x: Vec<T> = v;",
    "impl<T> Trait for X {}",
    "if a <b {}",
    "x.iter().collect::<Vec<_>>()",
    "fn f<T: Clone>(t: T) {}",
  ];
  for (const line of mustNotMatch) {
    assert.equal(
      begin.test(line),
      false,
      `begin must NOT match: ${line}`,
    );
  }
});

test("jsx-element's begin pattern matches real JSX tag openings", () => {
  const mustMatch = ["<div>", "<Greeting />", "<my-element>", "return <div>", "(<div>"];
  for (const line of mustMatch) {
    // A fresh RegExp per line: a global/sticky-free `test()` call is
    // stateless here (no `g`/`y` flag), but a fresh instance keeps this
    // loop's failures unambiguous regardless.
    const begin = jsxElementBeginRegExp();
    assert.equal(begin.test(line), true, `begin MUST match: ${line}`);
  }
});

test("jsx-element's end pattern closes a hyphenated tag name", () => {
  const grammar = loadGrammar();
  const beginRe = jsxElementBeginRegExp();
  const match = beginRe.exec("<my-element>");
  assert.ok(match, "begin must match the hyphenated opening tag first");
  assert.equal(match[2], "my-element");

  // The end pattern's `\2` backreference is resolved by the grammar
  // engine against whatever `begin` actually captured; simulate that
  // substitution directly (Node's `RegExp` has no built-in numbered-
  // backreference-from-another-match substitution) and confirm the
  // resulting concrete pattern matches the real closing tag.
  const endSource = grammar.repository["jsx-element"].end.replace(
    /\\2/g,
    match[2].replace(/[.*+?^${}()|[\]\\]/g, "\\$&"),
  );
  const endRe = new RegExp(endSource);
  assert.equal(endRe.test("</my-element>"), true);
  assert.equal(endRe.test("</my>"), false);
});
