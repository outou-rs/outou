//! A small, deliberately non-exhaustive HTML attribute vocabulary for
//! attribute-name completion on intrinsic elements (`TODO(alpha) (#31)`:
//! a complete, spec-derived table, and validation of attribute names
//! against it, belong to `docs/backend-leakage.md` row 12).
//!
//! Every name below is one the backend accepts today, read from
//! `dioxus-html 0.7.10` (the version in `Cargo.lock`):
//!
//! - `OWN_ATTRIBUTES`: the fields of each element block in
//!   `src/elements.rs` (`builder_constructors!`), written the way a `.rsx`
//!   author writes them (`r#type` is `type`, `http_equiv` is `http-equiv`).
//!   Left out: names that are not standard HTML (`initial_checked`,
//!   `initial_value`, `initial_selected`, `margin_width`, `margin_height`,
//!   `frame_border`, `directory`, `script`, `text`, `elementtiming`,
//!   `attributionsrc`, `allowpaymentrequest`, `typemustmatch`, `longdesc`,
//!   `scrolling`, `align`, `language`).
//! - `GLOBAL`: `global_attributes` in `src/attribute_groups.rs`, plus
//!   `key` (Outou's own, grammar §5.1). Left out: `prevent_default`
//!   (deprecated), `dangerous_inner_html`, `is`, `exportparts`.
//! - `ARIA`: the `aria_*` entries of the same file, hyphenated.
//! - `EVENTS`: the standard DOM handlers declared under `src/events/`.
//!   Left out: `onmounted`, `onvisible`, `onresize`, `ondoubleclick`,
//!   `ondragexit` (not standard DOM events or duplicates of one).
//!
//! `crates/outou-backend-dioxus/tests/attr_names_compile.rs` includes this
//! file and compiles every name for every element in `OWN_ATTRIBUTES`
//! (`cargo test -p outou-backend-dioxus --test attr_names_compile -- --ignored`).

/// Attributes every element accepts.
const GLOBAL: &[&str] = &[
    "accesskey",
    "autocapitalize",
    "autofocus",
    "class",
    "contenteditable",
    "dir",
    "draggable",
    "enterkeyhint",
    "hidden",
    "id",
    "inert",
    "inputmode",
    "itemid",
    "itemprop",
    "itemref",
    "itemscope",
    "itemtype",
    "key",
    "lang",
    "nonce",
    "part",
    "popover",
    "role",
    "slot",
    "spellcheck",
    "style",
    "tabindex",
    "title",
    "translate",
];

/// ARIA attributes (`aria_*` in `attribute_groups.rs`), hyphenated.
const ARIA: &[&str] = &[
    "aria-activedescendant",
    "aria-atomic",
    "aria-autocomplete",
    "aria-busy",
    "aria-checked",
    "aria-colcount",
    "aria-colindex",
    "aria-colspan",
    "aria-controls",
    "aria-current",
    "aria-describedby",
    "aria-details",
    "aria-disabled",
    "aria-dropeffect",
    "aria-errormessage",
    "aria-expanded",
    "aria-flowto",
    "aria-grabbed",
    "aria-haspopup",
    "aria-hidden",
    "aria-invalid",
    "aria-keyshortcuts",
    "aria-label",
    "aria-labelledby",
    "aria-level",
    "aria-live",
    "aria-modal",
    "aria-multiline",
    "aria-multiselectable",
    "aria-orientation",
    "aria-owns",
    "aria-placeholder",
    "aria-posinset",
    "aria-pressed",
    "aria-readonly",
    "aria-relevant",
    "aria-required",
    "aria-roledescription",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowspan",
    "aria-selected",
    "aria-setsize",
    "aria-sort",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
    "aria-valuetext",
];

/// The standard event-handler attributes.
const EVENTS: &[&str] = &[
    "onabort",
    "onanimationend",
    "onanimationiteration",
    "onanimationstart",
    "onauxclick",
    "onbeforetoggle",
    "onblur",
    "oncancel",
    "oncanplay",
    "oncanplaythrough",
    "onchange",
    "onclick",
    "oncompositionend",
    "oncompositionstart",
    "oncompositionupdate",
    "oncontextmenu",
    "oncopy",
    "oncut",
    "ondblclick",
    "ondrag",
    "ondragend",
    "ondragenter",
    "ondragleave",
    "ondragover",
    "ondragstart",
    "ondrop",
    "ondurationchange",
    "onemptied",
    "onencrypted",
    "onended",
    "onerror",
    "onfocus",
    "onfocusin",
    "onfocusout",
    "ongotpointercapture",
    "oninput",
    "oninvalid",
    "onkeydown",
    "onkeypress",
    "onkeyup",
    "onload",
    "onloadeddata",
    "onloadedmetadata",
    "onloadstart",
    "onlostpointercapture",
    "onmousedown",
    "onmouseenter",
    "onmouseleave",
    "onmousemove",
    "onmouseout",
    "onmouseover",
    "onmouseup",
    "onpaste",
    "onpause",
    "onplay",
    "onplaying",
    "onpointercancel",
    "onpointerdown",
    "onpointerenter",
    "onpointerleave",
    "onpointermove",
    "onpointerout",
    "onpointerover",
    "onpointerup",
    "onprogress",
    "onratechange",
    "onreset",
    "onscroll",
    "onscrollend",
    "onseeked",
    "onseeking",
    "onselect",
    "onselectionchange",
    "onselectstart",
    "onstalled",
    "onsubmit",
    "onsuspend",
    "ontimeupdate",
    "ontoggle",
    "ontouchcancel",
    "ontouchend",
    "ontouchmove",
    "ontouchstart",
    "ontransitionend",
    "onvolumechange",
    "onwaiting",
    "onwheel",
];

/// Attributes specific to one element, keyed by lowercase tag name. An
/// element that is not listed has only the global ones.
const OWN_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("base", &["href", "target"]),
    (
        "link",
        &[
            "as",
            "blocking",
            "crossorigin",
            "disabled",
            "fetchpriority",
            "href",
            "hreflang",
            "integrity",
            "media",
            "referrerpolicy",
            "rel",
            "sizes",
            "title",
            "type",
        ],
    ),
    (
        "meta",
        &["charset", "content", "http-equiv", "name", "property"],
    ),
    ("style", &["media", "nonce", "title", "type"]),
    ("blockquote", &["cite"]),
    ("li", &["value"]),
    ("ol", &["reversed", "start", "type"]),
    (
        "a",
        &[
            "download", "href", "hreflang", "ping", "rel", "target", "type",
        ],
    ),
    ("data", &["value"]),
    ("q", &["cite"]),
    ("time", &["datetime"]),
    (
        "area",
        &[
            "alt", "coords", "download", "href", "hreflang", "shape", "target",
        ],
    ),
    (
        "audio",
        &[
            "autoplay",
            "controls",
            "crossorigin",
            "loop",
            "muted",
            "preload",
            "src",
        ],
    ),
    (
        "img",
        &[
            "alt",
            "crossorigin",
            "decoding",
            "fetchpriority",
            "height",
            "ismap",
            "loading",
            "referrerpolicy",
            "sizes",
            "src",
            "srcset",
            "usemap",
            "width",
        ],
    ),
    ("map", &["name"]),
    ("track", &["default", "kind", "label", "src", "srclang"]),
    (
        "video",
        &[
            "autoplay",
            "controls",
            "crossorigin",
            "height",
            "loop",
            "muted",
            "playsinline",
            "poster",
            "preload",
            "src",
            "width",
        ],
    ),
    ("embed", &["height", "src", "type", "width"]),
    (
        "iframe",
        &[
            "allow",
            "allowfullscreen",
            "height",
            "name",
            "referrerpolicy",
            "src",
            "srcdoc",
            "width",
        ],
    ),
    (
        "object",
        &["data", "form", "height", "name", "type", "usemap", "width"],
    ),
    ("param", &["name", "value"]),
    ("source", &["src", "type"]),
    ("canvas", &["height", "width"]),
    (
        "script",
        &[
            "async",
            "crossorigin",
            "defer",
            "fetchpriority",
            "integrity",
            "nomodule",
            "nonce",
            "referrerpolicy",
            "src",
            "type",
        ],
    ),
    ("del", &["cite", "datetime"]),
    ("ins", &["cite", "datetime"]),
    ("col", &["span"]),
    ("colgroup", &["span"]),
    ("td", &["colspan", "rowspan"]),
    ("th", &["abbr", "colspan", "rowspan", "scope"]),
    (
        "button",
        &[
            "autofocus",
            "disabled",
            "form",
            "formaction",
            "formenctype",
            "formmethod",
            "formnovalidate",
            "formtarget",
            "name",
            "popovertarget",
            "popovertargetaction",
            "type",
            "value",
        ],
    ),
    ("fieldset", &["disabled", "form", "name"]),
    (
        "form",
        &[
            "action",
            "autocomplete",
            "enctype",
            "method",
            "name",
            "novalidate",
            "target",
        ],
    ),
    (
        "input",
        &[
            "accept",
            "alt",
            "autocomplete",
            "autocorrect",
            "autofocus",
            "capture",
            "checked",
            "disabled",
            "form",
            "formaction",
            "formenctype",
            "formmethod",
            "formnovalidate",
            "formtarget",
            "height",
            "list",
            "max",
            "maxlength",
            "min",
            "minlength",
            "multiple",
            "name",
            "pattern",
            "placeholder",
            "popovertarget",
            "popovertargetaction",
            "readonly",
            "required",
            "size",
            "spellcheck",
            "src",
            "step",
            "tabindex",
            "type",
            "value",
            "width",
        ],
    ),
    ("label", &["for", "form"]),
    (
        "meter",
        &["form", "high", "low", "max", "min", "optimum", "value"],
    ),
    ("optgroup", &["disabled", "label"]),
    ("option", &["disabled", "label", "selected", "value"]),
    ("output", &["form", "name"]),
    ("progress", &["max", "value"]),
    (
        "select",
        &[
            "autocomplete",
            "autofocus",
            "disabled",
            "form",
            "multiple",
            "name",
            "required",
            "size",
            "value",
        ],
    ),
    (
        "textarea",
        &[
            "autocomplete",
            "autocorrect",
            "autofocus",
            "cols",
            "disabled",
            "form",
            "maxlength",
            "minlength",
            "name",
            "placeholder",
            "readonly",
            "required",
            "rows",
            "spellcheck",
            "value",
            "wrap",
        ],
    ),
    ("details", &["open"]),
    ("dialog", &["open"]),
    ("slot", &["name"]),
];

/// The lowercase tag names that have attributes of their own in
/// `OWN_ATTRIBUTES`. A lookup table only: whether a tag is an HTML element
/// at all is decided by its first letter (grammar §5), never by this list.
/// Only the tests use it (the table's own tests and the backend's compile
/// test, which includes this file), hence `cfg(test)`.
#[cfg(test)]
pub fn elements_with_own_attributes() -> impl Iterator<Item = &'static str> {
    OWN_ATTRIBUTES.iter().map(|(tag, _)| *tag)
}

/// Every attribute name offered for the HTML element `tag`, sorted and
/// deduplicated: the element's own attributes (if it has a table row), the
/// global attributes, ARIA and the event handlers.
pub fn attribute_names(tag: &str) -> Vec<&'static str> {
    let own: &[&str] = OWN_ATTRIBUTES
        .iter()
        .find(|(name, _)| *name == tag)
        .map_or(&[], |(_, names)| names);
    let mut names: Vec<&'static str> = own
        .iter()
        .chain(GLOBAL)
        .chain(ARIA)
        .chain(EVENTS)
        .copied()
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_without_a_table_row_still_gets_the_global_attributes() {
        let names = attribute_names("details-not-in-the-table");
        for expected in ["class", "id", "key", "aria-label", "onclick", "tabindex"] {
            assert!(names.contains(&expected), "{expected}");
        }
        assert!(!names.contains(&"checked"));
    }

    #[test]
    fn element_specific_names_are_offered_only_on_their_element() {
        assert!(attribute_names("input").contains(&"checked"));
        assert!(!attribute_names("div").contains(&"checked"));
        assert!(attribute_names("a").contains(&"href"));
    }

    #[test]
    fn names_are_sorted_and_unique() {
        let names = attribute_names("input");
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
        // `tabindex` and `spellcheck` are both global and listed on `input`.
        assert_eq!(names.iter().filter(|name| **name == "tabindex").count(), 1);
    }

    #[test]
    fn the_table_keys_are_unique_lowercase_tags() {
        let tags: Vec<&str> = elements_with_own_attributes().collect();
        let mut unique = tags.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(tags.len(), unique.len());
        assert!(tags
            .iter()
            .all(|tag| tag.chars().all(|c| c.is_ascii_lowercase())));
    }
}
