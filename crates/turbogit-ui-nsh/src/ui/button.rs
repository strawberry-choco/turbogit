//! The button: the press target the app styles, a view with the button
//! role. The test id lives on the press target itself, so a selector both
//! finds and presses it.

use nsh_base::theme;
use nsh_core::prelude::*;

use super::{ControlSize, size_pad_x, size_pad_y, size_type};
use crate::theme as data;

/// The button's variant: what role its fill and ink play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// The one primary: the brand fill, white ink.
    #[default]
    Primary,
    /// The raised secondary: muted fill, ink.
    Secondary,
    /// No fill at rest; the muted band answers the pointer.
    Ghost,
    /// The destructive fill, white ink.
    Danger,
}

/// The props of [`button`].
pub struct ButtonProps {
    /// The label.
    pub label: SharedString,
    /// Read fresh on every render, when the label names its row (a recents
    /// remove control); `label` then holds the mount-time spelling.
    pub label_of: Option<std::rc::Rc<dyn Fn() -> String>>,
    /// A leading icon glyph, when the button carries one.
    pub icon: Option<SharedString>,
    pub variant: ButtonVariant,
    pub size: ControlSize,
    /// No presses, dimmed.
    pub disabled: bool,
    /// A toggled trigger, the current tab: screen readers say so.
    pub selected: bool,
    /// The press handler.
    pub on_press: Option<Box<dyn Fn() + 'static>>,
    /// What screen readers say, when the label is not enough (an icon).
    pub accessibility_label: Option<SharedString>,
    /// The id UI tests press it by.
    pub test_id: Option<SharedString>,
}

impl ButtonProps {
    pub fn new(label: impl Into<SharedString>) -> Self {
        ButtonProps {
            label: label.into(),
            label_of: None,
            icon: None,
            variant: ButtonVariant::default(),
            size: ControlSize::default(),
            disabled: false,
            selected: false,
            on_press: None,
            accessibility_label: None,
            test_id: None,
        }
    }

    pub fn label_of(mut self, label_of: impl Fn() -> String + 'static) -> Self {
        self.label_of = Some(std::rc::Rc::new(label_of));
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
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

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn test_id(mut self, test_id: &str) -> Self {
        self.test_id = Some(SharedString::from(test_id.to_string()));
        self
    }
}

/// The fills and inks of each variant, at rest.
fn variant_colors(variant: ButtonVariant) -> (AdaptiveColor, AdaptiveColor) {
    let t = theme();
    match variant {
        ButtonVariant::Primary => (t.primary, t.primary_foreground),
        ButtonVariant::Secondary => (t.secondary, t.secondary_foreground),
        ButtonVariant::Ghost => (AdaptiveColor::TRANSPARENT, t.foreground),
        ButtonVariant::Danger => (t.destructive, t.primary_foreground),
    }
}

/// The fill the pointer answers with, per variant.
fn variant_hover(variant: ButtonVariant) -> AdaptiveColor {
    let t = theme();
    match variant {
        ButtonVariant::Primary => t.primary.map(|c| c.darken(0.12)),
        ButtonVariant::Secondary => t.muted,
        ButtonVariant::Ghost => t.muted,
        ButtonVariant::Danger => t.destructive.map(|c| c.darken(0.12)),
    }
}

/// The fill while the press is down.
fn variant_pressed(variant: ButtonVariant) -> AdaptiveColor {
    let t = theme();
    match variant {
        ButtonVariant::Primary => t.primary.map(|c| c.darken(0.24)),
        ButtonVariant::Secondary => t.accent,
        ButtonVariant::Ghost => t.accent,
        ButtonVariant::Danger => t.destructive.map(|c| c.darken(0.24)),
    }
}

/// A styled button: the focus ring, the pressed step, and the disabled dim
/// are the control's own; the label and the optional leading icon are its
/// slots.
pub fn button(props: ButtonProps) -> impl IntoElement {
    let t = theme();
    let (fill, ink) = variant_colors(props.variant);
    let (pad_x, pad_y) = (size_pad_x(props.size), size_pad_y(props.size));
    let label = props.label.clone();
    let label_of = props.label_of.clone();
    let icon = props.icon.clone();
    let a11y = props
        .accessibility_label
        .clone()
        .unwrap_or_else(|| props.label.clone());
    let a11y_of = props.label_of.clone();

    let mut inner = view()
        .role(AccessibilityRole::Button)
        .optional_label(move || {
            a11y_of
                .as_ref()
                .map(|read| SharedString::from(read()))
                .or_else(|| Some(a11y.clone()))
        })
        .accessibility_state(move || AccessibilityState {
            selected: props.selected,
            ..AccessibilityState::default()
        })
        .disabled(props.disabled)
        .flex_row()
        .items_center()
        .rounded(px(data::CONTROL_RADIUS))
        .px(px(pad_x))
        .py(px(pad_y))
        .bg(fill)
        .text_size(px(size_type(props.size)))
        .text_color(ink)
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .hover(|s| s.bg(variant_hover(props.variant)))
        .active(|s| s.bg(variant_pressed(props.variant)))
        .disabled_style(|s| s.opacity(0.45));
    if let Some(press) = props.on_press {
        inner = inner.on_press(press);
    }
    if let Some(icon) = icon {
        inner = inner.child(text(icon).text_size(px(size_type(props.size))));
    }
    inner = inner.child(match label_of {
        Some(read) => text(move || SharedString::from(read())),
        None => text(move || label.clone()),
    });
    // The test id names the press target itself, so a selector finds and
    // presses the same view.
    if let Some(id) = props.test_id {
        inner = inner.test_id(&id);
    }
    inner
}
