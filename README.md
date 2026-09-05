# Outou — Rust with JSX.

**Phase 0 feasibility spike. Not usable yet.**

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

JSX is an expression of the language in a standalone `.rsx` file, not the body of a macro. The goal of Phase 0 is to find out whether that can be done while keeping everything Rust gives you: `cargo build`, rust-analyzer, rustfmt, real modules, real diagnostics.

## Documentation

- [docs/design.md](docs/design.md) — what Outou is betting on and what it does not do
- [docs/grammar.md](docs/grammar.md) — how JSX fits into Rust syntax
- [docs/phase0.md](docs/phase0.md) — the feasibility plan, its gates and what gets cut first
- [docs/backend-leakage.md](docs/backend-leakage.md) — where the temporary backend shows through
- [docs/adr/](docs/adr/README.md) — architecture decisions
- [spikes/rust-analyzer/](spikes/rust-analyzer/README.md) — the first experiment: rust-analyzer on hand-written generated Rust

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
