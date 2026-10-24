//! The values one `rsx!` call evaluates before it runs: its keys
//! (`crate::key`) and its select and option values (`crate::select`). The
//! call's text refers to them by synthesized identifiers.

use outou_codegen::Writer;
use outou_syntax::ast::{JsxAttribute, JsxElement};
use outou_syntax::select::SelectScan;

use crate::key::KeyBindings;
use crate::select::SelectBindings;
use crate::Cx;

pub struct CallBindings<'a> {
    keys: KeyBindings<'a>,
    selects: SelectBindings<'a>,
}

impl<'a> CallBindings<'a> {
    /// Collects the bindings of the `rsx!` call that `element` starts.
    pub fn collect(element: &'a JsxElement, scan: &SelectScan) -> Self {
        Self {
            keys: KeyBindings::collect(element),
            selects: SelectBindings::collect(element, scan),
        }
    }

    /// Whether the call has nothing to bind (and so needs no block).
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.selects.is_empty()
    }

    /// The identifier bound to `attribute`'s value, if it is bound.
    pub fn identifier(&self, attribute: &JsxAttribute) -> Option<String> {
        self.keys
            .identifier(attribute)
            .or_else(|| self.selects.identifier(attribute))
    }

    /// The generated `selected: …` attribute of `option`, if it gets one.
    pub fn selected_attribute(&self, option: &JsxElement, scan: &SelectScan) -> Option<String> {
        self.selects.selected_attribute(option, scan)
    }

    /// Writes the `let`s: keys first, then select and option values.
    pub fn write_bindings(&self, writer: &mut Writer, source: &str, cx: Cx<'_>) {
        self.keys.write_bindings(writer, source, cx);
        self.selects.write_bindings(writer, source, cx);
    }
}
