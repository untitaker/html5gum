use crate::trees::construction_dispatcher::{ElementNamespace, ExpandedName};

pub(crate) trait ElementScope {
    fn matches(&self, name: ExpandedName<'_>) -> bool;
}

impl<F> ElementScope for F where F: Fn(ExpandedName<'_>) -> bool {
    fn matches(&self, name: ExpandedName<'_>) -> bool {
        self(name)
    }
}

pub(crate) fn html_default_scope(name: ExpandedName<'_>) -> bool {
    name.namespace.is_none() && matches!(name.local_name, b"applet"  |     b"caption" |     b"html" |     b"table" |     b"td" |     b"th" |     b"marquee" |     b"object" |     b"template")
}

pub(crate) fn default_scope(name: ExpandedName) -> bool {
    html_default_scope(name) || mathml_text_integration_point(name) || svg_html_integration_point(name)
}

pub(crate) fn mathml_text_integration_point(name: ExpandedName) -> bool {
    matches!(name.namespace, Some(ElementNamespace::MathML))
        && matches!(name.local_name, b"mi" | b"mo" | b"mn" | b"ms" | b"mtext")
}

pub(crate) fn svg_html_integration_point(name: ExpandedName) -> bool {
    matches!(name.namespace, Some(ElementNamespace::SVG))
        && matches!(name.local_name, b"foreignObject" | b"desc" | b"title")
}

pub(crate) fn list_item_scope(name: ExpandedName) -> bool {
    default_scope(name) || (name.namespace.is_none() && matches!(name.local_name, b"ol" | b"ul"))
}

pub(crate) fn button_scope(name: ExpandedName) -> bool {
    default_scope(name) || (name.namespace.is_none() && matches!(name.local_name, b"button"))
}

pub(crate) fn table_scope(name: ExpandedName) -> bool {
    name.namespace.is_none() && matches!(name.local_name, b"html" | b"table" | b"template")
}

pub(crate) fn select_scope(name: ExpandedName) -> bool {
    !(name.namespace.is_none() && matches!(name.local_name, b"optgroup" | b"option"))
}
