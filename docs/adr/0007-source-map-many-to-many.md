# 0007. Source maps are many-to-many

Status: Accepted

## Context

A naive source map pairs one generated range with one source range. JSX does not lower that way:

```rsx
<Greeting>
    Hello
</Greeting>
```

becomes

```rust
Greeting {
    "Hello"
}
```

The generated identifier `Greeting` comes from both the opening and the closing tag. Closing tags and attribute names often have no generated counterpart at all, and one source expression may be emitted several times.

## Decision

The mapping model is many-to-many:

```text
1 source span → N generated spans
1 generated span → N source spans   (including zero)
```

Each generated file has one `SourceMap`. A workspace-wide `Registry` maps a generated file URI to its `SourceMap` and on to the original `.rsx` URIs.

Reverse-mapping rule: if a `.rsx` file produced the location, map back to it. Otherwise return the Rust location untouched, whether it is a plain `.rs` module or a dependency crate.

## Consequences

- Cross-file "go to definition" works through the registry.
- Rename, when it comes, can update both tags from one generated identifier without a new model.
- Generated spans with no source are ordinary; tools must handle an empty list.

## Alternatives considered

- **One-to-one with a "primary" span.** Breaks rename and misplaces diagnostics on closing tags.
- **Line-based maps** as in JavaScript. Too coarse for token-level hover and completion.
