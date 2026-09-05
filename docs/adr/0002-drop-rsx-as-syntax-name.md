# 0002. The syntax is called "Outou JSX"; `.rsx` is only the extension

Status: Accepted

## Context

Earlier the syntax itself was called "RSX". Dioxus already uses `rsx!` for its macro and "RSX" for its DSL, and the two are not the same language: Dioxus RSX is a Rust-like builder syntax, Outou's is JSX.

## Decision

The syntax is called **Outou JSX**, or just **JSX** when the context is clear. `.rsx` is the file extension and nothing more. The name "RSX" is not used for the syntax in code, documentation or diagnostics.

## Consequences

- Documentation says "JSX expression", "JSX element", "JSX text".
- The editor language id is `outou-rsx`, which refers to the file type.
- Diagnostics never say "RSX".

## Alternatives considered

- **Keep "RSX".** Would be read as Dioxus's syntax by anyone who knows Dioxus, and Outou compiles *to* that syntax, which would be confusing in every error message.
- **Change the extension.** `.rsx` is short, unclaimed among Rust tools, and reads as "Rust + JSX"; the extension is not the problem.
