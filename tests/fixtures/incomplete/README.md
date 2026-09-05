# incomplete fixtures

Half-typed input. For each `*.rsx` the parser must return an AST with error nodes (never panic), and recovery-mode codegen must emit Rust in which every complete Rust expression is still analyzable by rust-analyzer. These three are required for Gate 1.

Each `*.rsx` also has a sibling `*.expected` listing the Outou diagnostics it must produce, verbatim and in the order the parser emits them (innermost construct first), in the same format as `tests/fixtures/diagnostics/`. Rust-level errors from blocks the parser closes implicitly at end of input are not listed. These files are whole mid-edit buffers: a trailing `}` that closes the enclosing `fn` is part of what recovery must handle (`docs/grammar.md` §2.2).
