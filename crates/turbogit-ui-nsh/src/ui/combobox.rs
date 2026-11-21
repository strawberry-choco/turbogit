//! The combobox over nsh-base's Combobox: a typed filter over a choice
//! list. The new-branch name field and its suggester land on this in N4.

use nsh_base::{SelectItem, combobox as base_combobox, theme};
use nsh_core::prelude::*;

use super::ControlSize;
use crate::theme as data;

/// The props of [`combobox`].
pub struct ComboboxProps {
    pub items: Vec<SelectItem>,
    /// The chosen values at first.
    pub default_value: Vec<SharedString>,
    /// What the input shows while empty.
    pub placeholder: Option<SharedString>,
    /// Several values: a choice toggles the item in the value.
    pub multiple: bool,
    pub disabled: bool,
    /// The choice handler, the values after the choice.
    pub on_change: Option<super::OnChange<Vec<SharedString>>>,
    pub accessibility_label: Option<SharedString>,
    pub size: ControlSize,
    pub test_id: Option<SharedString>,
}

impl ComboboxProps {
    pub fn new(items: Vec<SelectItem>) -> Self {
        ComboboxProps {
            items,
            default_value: Vec::new(),
            placeholder: None,
            multiple: false,
            disabled: false,
            on_change: None,
            accessibility_label: None,
            size: ControlSize::default(),
            test_id: None,
        }
    }

    pub fn default_value(mut self, value: impl Into<SharedString>) -> Self {
        self.default_value.push(value.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, on_change: impl Fn(&Vec<SharedString>) + 'static) -> Self {
        self.on_change = Some(Box::new(on_change));
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

fn text_size_of(size: ControlSize) -> f32 {
    match size {
        ControlSize::Compact => data::TYPE_CONTROL,
        ControlSize::Regular => data::TYPE_BODY,
        ControlSize::Large => data::TYPE_DETAIL_TITLE,
    }
}

/// A combobox: the input frame at rest, the popover list over it.
pub fn combobox(props: ComboboxProps) -> impl IntoElement {
    let t = theme();
    let mut inner = base_combobox()
        .items(props.items.clone())
        .multiple(props.multiple)
        .disabled(props.disabled)
        .popup_class("rounded-md p-1 shadow-lg")
        .item_class("h-7 flex-row items-center rounded-sm px-2")
        .text_size(px(text_size_of(props.size)))
        .bg(t.background)
        .text_color(t.foreground)
        .rounded(px(data::CONTROL_RADIUS))
        .border(px(1.0))
        .border_color(t.input)
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .disabled_style(|s| s.opacity(0.45));
    if !props.default_value.is_empty() {
        inner = inner.default_value(props.default_value.clone());
    }
    if let Some(placeholder) = props.placeholder.clone() {
        inner = inner.placeholder(placeholder);
    }
    if let Some(a11y) = props.accessibility_label.clone() {
        inner = inner.accessibility_label(a11y);
    }
    if let Some(on_change) = props.on_change {
        inner = inner.on_change(move |value: &Vec<SharedString>| on_change(value));
    }
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(inner),
        None => view().child(inner),
    }
}
