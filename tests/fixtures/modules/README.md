# modules fixtures

`mixed/` is the minimum tree Phase 0 must handle:

```text
src/
├── main.rsx
├── components.rsx
└── components/
    ├── user.rsx
    └── button.rs
```

`ambiguous/` has both `widgets.rs` and `widgets.rsx`; resolving it must fail with `ambiguous Outou module `widgets``.
