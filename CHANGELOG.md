# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

- `outou fmt` keeps one blank line between JSX children, as Prettier does: 2 or more collapse into 1, leading and trailing blank lines are dropped, and an element with any text child loses its blank lines. It used to remove all of them. A text run that spans several lines is re-indented to follow its element (never reflowed: no words are joined, split or moved; only whitespace that `docs/grammar.md` §8 discards changes, so the generated Rust is unchanged; extra blank lines inside the text collapse to one, and the text next to the element's own opening or closing tag goes onto its own line when it has no significant edge whitespace there; text glued to a sibling child stays glued); it used to keep its old columns, leaving the interior lines and the closing tag misaligned. See `crates/outou-fmt/tests/golden/README.md`.
- Repository skeleton, public design documents, and the Week 1 rust-analyzer spike scaffold.
- `outou-lsp`: the integrated language server for `.rsx` files (Phase 0 Week 5, Gate 3 — PASS). See `docs/gate3-results.md`.
