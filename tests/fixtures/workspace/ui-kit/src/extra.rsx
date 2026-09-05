use outou::prelude::*;

/// A component only compiled when the `extra` feature is enabled.
///
/// `#[cfg(...)]` is never evaluated by the Outou module resolver (ADR
/// 0006): this file is always resolved and generated regardless of
/// whether `extra` is on, and the `#[cfg(feature = "extra")]` attribute
/// is preserved verbatim on the generated `mod extra;` declaration so
/// `rustc` is the one that actually decides whether it compiles.
#[component]
pub fn ExtraWidget() -> Element {
    <div class="extra">Extra</div>
}
