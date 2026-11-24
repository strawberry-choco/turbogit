//! The switch: an on/off setting that takes effect at once. The app owns the
//! value; a press reports the next one. The track and thumb paint from the
//! chrome tokens, and the focus ring follows the keyboard.

use std::rc::Rc;

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// The props of [`switch`]. The state reads through a closure, so the app's
/// signal drives the paint.
pub struct SwitchProps {
    pub checked: Rc<dyn Fn() -> bool>,
    pub disabled: bool,
    /// A press reports the next value.
    pub on_change: Option<Box<dyn Fn(bool) + 'static>>,
    /// What screen readers say: what the switch turns on.
    pub accessibility_label: Option<SharedString>,
    /// The id UI tests press it by.
    pub test_id: Option<SharedString>,
}

impl SwitchProps {
    pub fn new(checked: impl Fn() -> bool + 'static) -> Self {
        SwitchProps {
            checked: Rc::new(checked),
            disabled: false,
            on_change: None,
            accessibility_label: None,
            test_id: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, on_change: impl Fn(bool) + 'static) -> Self {
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

/// The track: the muted band at rest, the brand while on, a dim fill when
/// disabled. The thumb steps across with no timed transition.
fn track(checked: Rc<dyn Fn() -> bool>) -> impl IntoElement {
    let t = theme();
    let fill = checked.clone();
    let fill_color = t.clone();
    let thumb_checked = checked;
    let thumb_color = t.primary_foreground;
    view()
        .relative()
        .w(px(28.0))
        .h(px(16.0))
        .rounded(px(data::PILL_RADIUS))
        .bg(move || {
            if fill() {
                fill_color.primary
            } else {
                fill_color.muted
            }
        })
        .child(
            view()
                .absolute()
                .top(px(2.0))
                .left(move || px(if thumb_checked() { 14.0 } else { 2.0 }))
                .w(px(12.0))
                .h(px(12.0))
                .rounded(px(data::PILL_RADIUS))
                .bg(thumb_color),
        )
}

/// A switch with its label. A press reports the next value; the app setting
/// it back repaints the track and thumb.
pub fn switch(props: SwitchProps) -> impl IntoElement {
    let t = theme();
    let checked = props.checked.clone();
    let paint = props.checked.clone();
    let pressed = props.checked.clone();
    let mut inner = view()
        .role(AccessibilityRole::Switch)
        .optional_label(props.accessibility_label.clone())
        .accessibility_state(move || AccessibilityState {
            checked: Some(if paint() {
                nsh_core::Checked::True
            } else {
                nsh_core::Checked::False
            }),
            ..AccessibilityState::default()
        })
        .disabled(props.disabled)
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .rounded(px(data::CONTROL_RADIUS))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .hover(|s| s.bg(t.muted))
        .disabled_style(|s| s.opacity(0.45))
        .child(track(checked));
    if let Some(on_change) = props.on_change {
        inner = inner.on_press(move || on_change(!pressed()));
    }
    if let Some(id) = props.test_id {
        inner = inner.test_id(&id);
    }
    inner
}
