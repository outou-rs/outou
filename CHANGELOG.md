# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

- `outou fmt` keeps one blank line between JSX children, as Prettier does: 2 or more collapse into 1, leading and trailing blank lines are dropped, and an element with any text child loses its blank lines. It used to remove all of them. See `crates/outou-fmt/tests/golden/README.md`.
- Repository skeleton, public design documents, and the Week 1 rust-analyzer spike scaffold.
- `outou-lsp`: the integrated language server for `.rsx` files (Phase 0 Week 5, Gate 3 — PASS). See `docs/gate3-results.md`.
