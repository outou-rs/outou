# outou-cli

The `outou` command: `build`, `check` and `package`. `outou package` generates Rust and includes it in the published crate so that consumers need only `cargo build`.

Phase 0: Weeks 4–6.

## `outou check`

Parses every `.rsx` file under the current crate's `src/` directory (recursively, in sorted order — a dot-directory such as `src/.generated/` is never descended into, and a symlink is never followed) with `outou_syntax::parse` — the same front end `build`'s generation step and the language server use — and prints Outou's own rendered diagnostics for any file that has one: `error: <message>` for an error-severity diagnostic, `warning: <message>` for a warning-severity one, each followed by ` --> <path>:<line>:<col>`. Exits `1` if any file has an error-severity diagnostic, `0` otherwise (warnings alone do not fail the check). `build` and `package` remain `todo!()`; they are issue #8's and Week 6's work respectively.
