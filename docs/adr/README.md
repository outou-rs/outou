# Architecture decision records

One file per decision. Each record has the sections Context, Decision, Consequences and Alternatives considered, and fits on one page. Records are numbered in the order they were written and are never deleted; a superseding record links back.

| # | Decision | Status |
|---|---|---|
| [0001](0001-standalone-rsx-source-format.md) | `.rsx` is a standalone source format, not a macro | Accepted |
| [0002](0002-drop-rsx-as-syntax-name.md) | The syntax is called "Outou JSX"; `.rsx` is only the extension | Accepted |
| [0003](0003-dioxus-as-temporary-backend.md) | Dioxus is the temporary execution backend | Accepted |
| [0004](0004-grammar-extension-limited-to-jsx.md) | The grammar extension is limited to JSX expressions | Accepted |
| [0005](0005-rust-analyzer-strategy-priority.md) | rust-analyzer strategy priority: overlay, shadow project, rust-project.json | Accepted |
| [0006](0006-ambiguous-module-is-an-error.md) | An ambiguous module is an error | Accepted |
| [0007](0007-source-map-many-to-many.md) | Source maps are many-to-many | Accepted |
| [0008](0008-pregenerated-publish-artifacts.md) | Published crates ship pre-generated Rust | Accepted |
| [0009](0009-generated-source-location.md) | Where generated Rust lives | Accepted |
| [0010](0010-facade-crate-as-backend-boundary.md) | The `outou` facade crate is the backend boundary | Accepted |
