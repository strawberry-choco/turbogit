//! The select over nsh-base's Select: a trigger the app styles, a popup the
//! tokens paint.

use nsh_base::{SelectItem, select as base_select, theme};
use nsh_core::prelude::*;

use super::ControlSize;
use crate::theme as data;

/// The props of [`select`].
pub struct SelectProps {
    /// The items, in order.
    pub items: Vec<SelectItem>,
    /// The chosen value at first.
    pub default_value: Option<SharedString>,
    /// What the trigger says while nothing is chosen.
    pub placeholder: Option<SharedString>,
    /// What the trigger shows: the app binds it to its own state (the
    /// chosen title), or leaves it at the placeholder.
    pub label: Option<Box<dyn Fn() -> SharedString + 'static>>,
    pub disabled: bool,
    /// The choice handler, the chosen item's value.
    pub on_change: Option<super::OnChange<SharedString>>,
    /// What screen readers call it.
    pub accessibility_label: Option<SharedString>,
    pub size: ControlSize,
    /// The id UI tests open it by.
    pub test_id: Option<SharedString>,
}

impl SelectProps {
    pub fn new(items: Vec<SelectItem>) -> Self {
        SelectProps {
            items,
            default_value: None,
            placeholder: None,
            label: None,
            disabled: false,
            on_change: None,
            accessibility_label: None,
            size: ControlSize::default(),
            test_id: None,
        }
    }

    /// The trigger's label, read fresh on every render.
    pub fn label(mut self, label: impl Fn() -> SharedString + 'static) -> Self {
        self.label = Some(Box::new(label));
        self
    }

    pub fn default_value(mut self, value: impl Into<SharedString>) -> Self {
        self.default_value = Some(value.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, on_change: impl Fn(&SharedString) + 'static) -> Self {
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

/// A select: the trigger wears the input frame, the list wears the popover
/// surface, the rows answer the pointer with the accent band.
pub fn select(props: SelectProps) -> impl IntoElement {
    let t = theme();
    // The trigger: the input frame around the label, the chevron at its end.
    let trigger_label = props.label;
    let placeholder = props.placeholder.clone();
    let trigger_size = props.size;
    let trigger = view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .rounded(px(data::CONTROL_RADIUS))
        .px(px(data::BUTTON_PAD_X))
        .py(px(data::BUTTON_PAD_Y))
        .min_h(px(data::FILE_ROW_HEIGHT))
        .bg(t.background)
        .border(px(1.0))
        .border_color(t.input)
        .child(
            text(
                move || match (trigger_label.as_ref().map(|f| f()), placeholder.as_ref()) {
                    (Some(label), _) if !label.is_empty() => label,
                    (_, Some(placeholder)) => placeholder.clone(),
                    _ => SharedString::default(),
                },
            )
            .text_size(px(text_size_of(trigger_size)))
            .text_color(t.foreground)
            .flex_1(),
        )
        .child(
            text("▾")
                .text_size(px(text_size_of(trigger_size)))
                .text_color(t.muted_foreground),
        );
    let mut inner = base_select()
        .trigger(trigger)
        .items(props.items.clone())
        .disabled(props.disabled)
        .popup_class("rounded-md p-1 shadow-lg")
        .item_class("h-7 flex-row items-center rounded-sm px-2")
        .list_class("bg-popover")
        .group_class("px-2 py-1")
        .text_size(px(text_size_of(props.size)))
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .disabled_style(|s| s.opacity(0.45));
    if let Some(value) = props.default_value.clone() {
        inner = inner.default_value(value);
    }
    if let Some(placeholder) = props.placeholder.clone() {
        inner = inner.placeholder(placeholder);
    }
    if let Some(a11y) = props.accessibility_label.clone() {
        inner = inner.accessibility_label(a11y);
    }
    if let Some(on_change) = props.on_change {
        inner = inner.on_change(move |value: &Option<SharedString>| {
            if let Some(value) = value {
                on_change(value);
            }
        });
    }
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(inner),
        None => view().child(inner),
    }
}
