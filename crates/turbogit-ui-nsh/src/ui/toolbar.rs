//! The toolbar over nsh-base's Toolbar: one stop of keyboard focus, arrow
//! keys along its items, every item the app's own control.

use nsh_base::toolbar as base_toolbar;
use nsh_core::prelude::*;

use crate::theme as data;

/// The props of [`toolbar`].
pub struct ToolbarProps {
    /// The items (already-styled controls), groups, and separators.
    pub children: Vec<AnyElement>,
    /// A named toolbar for screen readers, when a page has several.
    pub accessibility_label: Option<SharedString>,
    /// Every item disabled with it.
    pub disabled: bool,
    pub test_id: Option<SharedString>,
}

impl ToolbarProps {
    pub fn new() -> Self {
        ToolbarProps {
            children: Vec::new(),
            accessibility_label: None,
            disabled: false,
            test_id: None,
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_element());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

impl Default for ToolbarProps {
    fn default() -> Self {
        Self::new()
    }
}

/// A toolbar: a row of items with the list gap between them.
pub fn toolbar(props: ToolbarProps) -> impl IntoElement {
    let mut bar = base_toolbar()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .disabled(props.disabled);
    if let Some(label) = props.accessibility_label.clone() {
        bar = bar.accessibility_label(label);
    }
    for child in props.children {
        bar = bar.child(child);
    }
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(bar),
        None => view().child(bar),
    }
}
