# 0014. Dioxus continues as the current backend, but is not the specification

Status: Accepted (2026-10-24). Supersedes [0003](0003-dioxus-as-temporary-backend.md); amends [0010](0010-facade-crate-as-backend-boundary.md).

## Context

ADR 0003 deferred the runtime decision to the end of the alpha (gate A5): keep Dioxus, or build a native runtime, decided from `docs/backend-leakage.md`. The ledger now has 38 rows: 25 pure Dioxus leakage, 2 leakage fixed, 2 leakage / temporary limitation, 8 temporary limitation, 1 Outou intrinsic / leakage (the `#[component]` marker), 0 accepted permanent. Seven rows only correct, resolve or close earlier rows (16, 17, 22, 23, 25, 28, 38), so the count of distinct constraints is about 31 to 32 depending on how row 28 is counted. The A4 dogfood application (`examples/dogfood-board`, `docs/alpha-results.md`) added rows 31 to 38.

The rows that make Outou's semantics unnatural fall into nine groups. **Five can be fixed by the facade and codegen without owning a runtime:**

1. Hooks and signals API (rows 8, 31, 33): guard-returning `read()`/`write()` leak into everyday code.
2. Numeric children (row 34, and 20 in part): React renders numbers, the backend decides by accident.
3. `key` (rows 35, 38): static keys and keys containing `{`/`}` fail with backend-authored messages.
4. `<select value>` (row 37): same JSX, different meaning than React.
5. Event types, in part (row 9): the handler vocabulary.

**Four cannot**, because they come from the component model itself: props must be `Clone + PartialEq + 'static` (rows 1 to 3); the shape of `Element` and `children` (rows 4, 5); borrowed props (row 10); and hyphenated attribute names (rows 14, 17, 18, 29), which would remain even with a native runtime because they are a limit of how an attribute name becomes a Rust field.

Two further problems do not depend on the runtime: `cargo build` diagnostics land on generated lines (friction F5), and the two-process rust-analyzer layout (A1). From A1, the only runtime-related cost is the absolute cost of the Dioxus dependency graph; the difference against a smaller runtime was not measured. Release bundle size was not measured (only the debug wasm of the dogfood app exists), and runtime performance and React interop were not evaluated. Outou also has no defined update semantics of its own yet (row 3).

## Decision

**Dioxus remains the backend for now, but it is not Outou's specification.** Outou's semantics are defined by Outou-owned APIs and by the lowering, implemented on top of Dioxus. Observable behavior that exists only because of Dioxus is a ledger row, not a rule. No Outou runtime, scheduler or update semantics is built now: the evidence that would justify one (performance, release bundle, React interop) is unmeasured, and the semantics such a runtime would implement are undefined.

The decision is carried out in the remaining A5 work, tracked as separate issues (referred to here by name, not number):

1. **`outou cargo <build|check|clippy|test>`.** Generates, then runs `cargo ... --message-format=json-diagnostic-rendered-ansi` and translates every `compiler-message` span through the existing source maps to `.rsx` positions. Messages that cannot be translated are kept or dropped by a stated rule; backend-path spans (such as `def_site_span`) are leakage and dropped. `outou build` stays "generate only". The translation logic of the UI test harness (`crates/outou-cli/tests/ui.rs`) becomes a library and is reused.
2. **Typed keys.** `key={expr}` is evaluated outside `rsx!` (`::outou::__private::key(&(EXPR))` bound to a local, then `key: "{__outou_key_N}"`). `IntoKey` (strings and integers) carries `#[diagnostic::on_unimplemented]`. A static string takes the same path.
3. **Typed children.** `{::outou::__private::child(EXPR)}` with `IntoChild<M>` (a marker generic): delegates to Dioxus's `IntoDynNode` and renders numbers and `char` as strings. **`bool` is a compile error**, because `cond && <x/>` is not a valid Rust type for a child; the diagnostic points to `if` and `Option`. `IntoChild` and `IntoKey` live in a public module (for example `outou::jsx`) and are sealed.
4. **`<select value={V}>`.** The `<option>` elements lexically inside a `<select value=..>` subtree are rewritten: `V` is first evaluated outside as a `String`, and each option gains `selected: option_selected(..)`. An explicit `selected` on an option wins; comparison is on strings. Options not reachable lexically (via variables, returned by components, or without a `value` attribute) and `multiple` are out of scope and documented.
5. **Outou-named state API.** `State<T>` (a newtype over Dioxus's `Signal`), `use_state`, `provide_context`, `use_context::<T>()`, `launch`. Read with `get() -> T` (`T: Clone`) and `with(|v| ..) -> R`; write with `set(v)` and `update(|v: &mut T| ..)`, both on `&self`. `read()` and `write()` are not public, so a guard never leaves the API and row 33's borrow trap disappears structurally. `ReadableExt` and `WritableExt` leave the prelude. Subscription semantics are Dioxus's. The old names (`use_signal`, `Signal`, `use_context_provider`, `ReadableExt`, `WritableExt`) are removed without deprecation, because the alpha is not public and its only users are `dogfood-board` and `phase0-app`, which are migrated. A test pins the list of names in the prelude.
6. **Close-out.** New ledger rows (appended) for the items above, the Friction log dispositions, and the A5 section of `docs/alpha-results.md`.

The gate A5 closes when this ADR is accepted, the implementation issues for items 1 to 5 are closed, and `docs/alpha-results.md` is complete.

**Facade rule (ADR 0010 amendment).** `outou::prelude` is Outou-owned API; `outou::__private` is the backend. The prelude does not `pub use` a backend item directly. The exceptions are the hidden names needed for macro hygiene (`dioxus_core`, `dioxus_elements`, `dioxus_signals`, `Props`; rows 11, 16) and these remaining ones: `Element` (row 4), event argument types (row 9) and `VirtualDom` (row 36).

**Switching to an Outou runtime.** Reopen the decision if any of these holds:

- Borrowed props or non-`Clone` props become a 1.0 requirement.
- The cost of one Dioxus upgrade exceeds a limit to be fixed from future measurement (the number is deliberately not set here).
- The React interop design needs control of the renderer.
- Release bundle size or indexing cost is measured and found unacceptable.
- Rows keep being added where the same JSX means something different and the facade cannot absorb it.

**Signs this recommendation is wrong.** (a) The facade wrappers break substantially at every Dioxus upgrade. (b) After wrapping the state API, guard or copy semantics still show through. (c) In the next application, friction from the four unfixable groups dominates.

## Consequences

- Outou code keeps the constraints of the four unfixable groups until the decision is reopened; they stay ledger rows classified as leakage, never as Outou rules.
- The facade becomes real API with a maintenance cost: every Dioxus upgrade must be absorbed in `__private` and in the wrappers.
- Application code changes with the state API (`use_state`, `State<T>`), and `outou cargo ...` becomes the expected way to build.
- `docs/alpha.md` and `AGENTS.md` change scope: a thin state API that never exposes guards is in scope for the A5 work; a runtime, a scheduler or new update semantics remain out of scope.
- Ledger row 4 describes `Element = Option<VNode>`. That is out of date: in `dioxus-core 0.7.10` (the version in `Cargo.lock`) `Element` is `Result<VNode, RenderError>`. The row is not rewritten (rows are append-only); this ADR is the correction, and the closing ledger rows restate it.

## Alternatives considered

- **Build an Outou runtime now.** Rejected: the cost it would save (performance, bundle, React interop) is unmeasured, and Outou's update semantics are undefined, so there is nothing yet to implement.
- **Keep Dioxus as the specification (re-export everything).** Rejected: it freezes the nine groups, including the five that are cheap to fix, into Outou's public API.
- **Another backend (Leptos, Yew and similar).** Not reevaluated in this round. Dioxus keeps the lowered shape closest to what Outou needs (ADR 0003), and the facade work above is what makes a later reevaluation cheap. This is a scope decision, not a finding against them.
- **Deprecate the old state names instead of removing them.** Rejected: nothing outside the repository uses them.
