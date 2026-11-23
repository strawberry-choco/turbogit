//! The checkbox: the press target the app styles, a view with the checkbox
//! role; the indicator inside shows the value the app owns. A press reports
//! the next state. The test id lives on the press target itself.

use std::rc::Rc;

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::theme as data;

/// The checkbox's value, including the partial one a select-all holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckboxState {
    #[default]
    Unchecked,
    Checked,
    /// Neither checked nor unchecked: "select all" over a partial selection.
    Indeterminate,
}

/// The props of [`checkbox`]. The state reads through a closure, so the
/// app's signal drives the paint: a press reports the next state, and the
/// app setting it back repaints.
pub struct CheckboxProps {
    pub state: Rc<dyn Fn() -> CheckboxState>,
    pub disabled: bool,
    /// A press reports the next state.
    pub on_change: Option<Box<dyn Fn(CheckboxState) + 'static>>,
    /// What screen readers say: what the checkbox selects.
    pub accessibility_label: Option<SharedString>,
    /// The id UI tests press it by.
    pub test_id: Option<SharedString>,
}

impl CheckboxProps {
    pub fn new(state: impl Fn() -> CheckboxState + 'static) -> Self {
        CheckboxProps {
            state: Rc::new(state),
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

    pub fn on_change(mut self, on_change: impl Fn(CheckboxState) + 'static) -> Self {
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

/// The indicator's fill and mark per state, from the chrome tokens.
fn indicator(state: CheckboxState) -> impl IntoElement {
    let t = theme();
    let (fill, mark, ink, edge) = match state {
        CheckboxState::Unchecked => (AdaptiveColor::TRANSPARENT, "", t.input, t.input),
        CheckboxState::Checked => (t.primary, "✓", t.primary_foreground, t.primary),
        CheckboxState::Indeterminate => (t.primary, "–", t.primary_foreground, t.primary),
    };
    view()
        .flex_row()
        .items_center()
        .justify_center()
        .w(px(data::TYPE_DETAIL_TITLE))
        .h(px(data::TYPE_DETAIL_TITLE))
        .rounded(px(data::MARK_RADIUS))
        .bg(fill)
        .border(px(1.0))
        .border_color(edge)
        .child(text(mark).text_size(px(data::TYPE_CHIP)).text_color(ink))
}

/// A checkbox: the box answers the pointer with the muted band, the checked
/// box wears the brand, the focus ring follows the keyboard.
pub fn checkbox(props: CheckboxProps) -> impl IntoElement {
    let t = theme();
    let state = props.state.clone();
    let pressed_state = props.state.clone();
    let mut inner = view()
        .role(AccessibilityRole::Checkbox)
        .optional_label(props.accessibility_label.clone())
        .accessibility_state(move || AccessibilityState {
            checked: Some(match state() {
                CheckboxState::Unchecked => nsh_core::Checked::False,
                CheckboxState::Checked => nsh_core::Checked::True,
                CheckboxState::Indeterminate => nsh_core::Checked::Mixed,
            }),
            ..AccessibilityState::default()
        })
        .disabled(props.disabled)
        .flex_row()
        .items_center()
        .rounded(px(data::MARK_RADIUS))
        .px(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .hover(|s| s.bg(t.muted))
        .active(|s| s.bg(t.accent))
        .disabled_style(|s| s.opacity(0.45))
        .child(dynamic(move || indicator(pressed_state())));
    if let Some(on_change) = props.on_change {
        let state = props.state.clone();
        inner = inner.on_press(move || {
            on_change(match state() {
                CheckboxState::Unchecked => CheckboxState::Checked,
                CheckboxState::Checked | CheckboxState::Indeterminate => CheckboxState::Unchecked,
            })
        });
    }
    if let Some(id) = props.test_id {
        inner = inner.test_id(&id);
    }
    inner
}
