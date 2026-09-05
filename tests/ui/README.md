# ui tests

rustc-style UI tests for Outou diagnostics. Each case is a directory:

```text
tests/ui/<case>/
├── input.rsx
└── expected.stderr
```

`expected.stderr` is the exact output of `outou check` for `input.rsx`, after path normalization. A harness compares them once `outou-syntax` exists; there are no cases yet.
