# outou-sourcemap

Many-to-many source maps between `.rsx` files and generated Rust, and a workspace-wide registry (generated URI → map → original URIs).
Reverse-mapping rule: if a `.rsx` file produced the location, map back to it; otherwise return the Rust location untouched.

Phase 0: Week 4.
