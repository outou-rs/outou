# Outou — Rust with JSX.

**Phase 0 (feasibility) complete — see [`docs/phase0-results.md`](docs/phase0-results.md). Internal alpha in progress; not ready for outside use.**

```rsx
use outou::prelude::*;

#[component]
fn App() -> Element {
    let user = load_user();

    <main>
        <UserCard user={user} />
    </main>
}
```

Write JSX. Keep Rust.

JSX is an expression of the language in a standalone `.rsx` file, not the body of a macro. Phase 0 tested whether that can be done while keeping everything Rust gives you: `cargo build`, rust-analyzer, rustfmt, real modules, real diagnostics. The results, and their qualifiers, are in [docs/phase0-results.md](docs/phase0-results.md).

## Documentation

- [docs/design.md](docs/design.md) — what Outou is betting on and what it does not do
- [docs/grammar.md](docs/grammar.md) — how JSX fits into Rust syntax
- [docs/phase0.md](docs/phase0.md) — the feasibility plan, its gates and what gets cut first
- [docs/phase0-results.md](docs/phase0-results.md) — the Phase 0 decision report
- [docs/alpha.md](docs/alpha.md) — the internal front-end alpha: its gates, what must not be cut and what is cut first
- [docs/backend-leakage.md](docs/backend-leakage.md) — where the temporary backend shows through
- [docs/adr/](docs/adr/README.md) — architecture decisions
- [spikes/rust-analyzer/](spikes/rust-analyzer/README.md) — the first experiment: rust-analyzer on hand-written generated Rust

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
