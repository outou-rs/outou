//! A small, deliberately non-exhaustive HTML attribute vocabulary for
//! attribute-name completion on intrinsic elements (`TODO(alpha) (#31)`:
//! a complete, spec-derived table, and validation of attribute names
//! against it, belong to `docs/backend-leakage.md` row 12).

/// Attributes every element accepts.
const GLOBAL: &[&str] = &[
    "class",
    "id",
    "key",
    "style",
    "title",
    "hidden",
    "tabindex",
    "role",
    "lang",
    "dir",
    "draggable",
    "contenteditable",
    "spellcheck",
    "aria-label",
    "aria-hidden",
    "aria-describedby",
    "aria-labelledby",
    "aria-live",
    "aria-expanded",
    "aria-checked",
    "aria-disabled",
];

/// The standard event-handler attributes.
const EVENTS: &[&str] = &[
    "onclick",
    "ondblclick",
    "onchange",
    "oninput",
    "onsubmit",
    "onfocus",
    "onblur",
    "onkeydown",
    "onkeyup",
    "onkeypress",
    "onmousedown",
    "onmouseup",
    "onmouseenter",
    "onmouseleave",
    "onmousemove",
    "onmouseover",
    "onmouseout",
    "onscroll",
    "onwheel",
    "ondrag",
    "ondragstart",
    "ondragend",
    "ondragover",
    "ondrop",
    "onload",
    "onerror",
];

/// Attributes specific to one element.
fn element_specific(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" => &["href", "target", "rel", "download"],
        "button" => &["type", "disabled", "name", "value", "form"],
        "form" => &["action", "method", "enctype", "novalidate", "autocomplete"],
        "img" => &["src", "alt", "width", "height", "loading"],
        "input" => &[
            "type",
            "value",
            "checked",
            "disabled",
            "readonly",
            "required",
            "placeholder",
            "name",
            "min",
            "max",
            "step",
            "maxlength",
            "minlength",
            "pattern",
            "autofocus",
            "autocomplete",
            "multiple",
            "accept",
        ],
        "label" => &["for"],
        "option" => &["value", "selected", "disabled", "label"],
        "select" => &["value", "disabled", "multiple", "name", "required"],
        "textarea" => &[
            "value",
            "placeholder",
            "rows",
            "cols",
            "disabled",
            "readonly",
            "required",
            "name",
            "maxlength",
        ],
        "iframe" => &["src", "width", "height", "name"],
        "link" => &["href", "rel", "type"],
        "meta" => &["name", "content", "charset"],
        "script" => &["src", "type", "async", "defer"],
        "td" | "th" => &["colspan", "rowspan"],
        "audio" | "video" => &["src", "controls", "autoplay", "loop", "muted"],
        "canvas" => &["width", "height"],
        _ => &[],
    }
}

/// Every attribute name offered for the HTML element `tag`, sorted and
/// deduplicated.
pub fn attribute_names(tag: &str) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = element_specific(tag)
        .iter()
        .chain(GLOBAL)
        .chain(EVENTS)
        .copied()
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}
