# incomplete fixtures

Half-typed input. For each `*.rsx` the parser must return an AST with error nodes (never panic), and recovery-mode codegen must emit Rust in which every complete Rust expression is still analyzable by rust-analyzer. These three are required for Gate 1.
