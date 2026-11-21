//! The icon button: a square press target for a glyph, with the label a
//! screen reader says. The test id lives on the press target itself.

use nsh_base::theme;
use nsh_core::prelude::*;

use super::{ControlSize, size_pad_x, size_pad_y};
use crate::theme as data;

/// The props of [`icon_button`].
pub struct IconButtonProps {
    /// The icon glyph.
    pub icon: SharedString,
    /// What screen readers say: the icon alone is not a name.
    pub accessibility_label: SharedString,
    pub size: ControlSize,
    pub disabled: bool,
    /// A toggled trigger.
    pub selected: bool,
    pub on_press: Option<Box<dyn Fn() + 'static>>,
    pub test_id: Option<SharedString>,
}

impl IconButtonProps {
    pub fn new(
        icon: impl Into<SharedString>,
        accessibility_label: impl Into<SharedString>,
    ) -> Self {
        IconButtonProps {
            icon: icon.into(),
            accessibility_label: accessibility_label.into(),
            size: ControlSize::default(),
            disabled: false,
            selected: false,
            on_press: None,
            test_id: None,
        }
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_press(mut self, on_press: impl Fn() + 'static) -> Self {
        self.on_press = Some(Box::new(on_press));
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// A ghost icon button: no fill at rest, the muted band on hover, the ring
/// under the keyboard. The square edge is the icon's own shape, the control
/// radius rounds it.
pub fn icon_button(props: IconButtonProps) -> impl IntoElement {
    let t = theme();
    let pad = (size_pad_x(props.size) + size_pad_y(props.size)) / 2.0;
    let mut inner = view()
        .role(AccessibilityRole::Button)
        .optional_label(Some(props.accessibility_label.clone()))
        .accessibility_state(move || AccessibilityState {
            selected: props.selected,
            ..AccessibilityState::default()
        })
        .disabled(props.disabled)
        .flex_row()
        .items_center()
        .justify_center()
        .rounded(px(data::CONTROL_RADIUS))
        .px(px(pad))
        .py(px(pad))
        .bg(AdaptiveColor::TRANSPARENT)
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.muted_foreground)
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .hover(|s| s.bg(t.muted).text_color(t.foreground))
        .active(|s| s.bg(t.accent))
        .disabled_style(|s| s.opacity(0.45));
    if let Some(press) = props.on_press {
        inner = inner.on_press(press);
    }
    inner = inner.child(text(move || props.icon.clone()));
    if let Some(id) = props.test_id {
        inner = inner.test_id(&id);
    }
    inner
}
