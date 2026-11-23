//! The tooltip over nsh-base's Tooltip: the popover bubble, shown after the
//! pointer rests or the keyboard focus moves. The trigger is the view the
//! `.tooltip` hint sits on.

use nsh_base::{theme, tooltip as base_tooltip};
use nsh_core::prelude::*;

use crate::theme as data;

/// The props of [`tooltip`].
pub struct TooltipProps {
    /// The trigger the tooltip follows.
    pub trigger: View,
    /// The tooltip's text.
    pub label: SharedString,
}

impl TooltipProps {
    pub fn new(trigger: View, label: impl Into<SharedString>) -> Self {
        TooltipProps {
            trigger,
            label: label.into(),
        }
    }
}

/// A tooltip: the popover surface, the chip type size.
pub fn tooltip(props: TooltipProps) -> impl IntoElement {
    let t = theme();
    let label = props.label.clone();
    props.trigger.tooltip(
        base_tooltip()
            .bg(t.popover)
            .text_color(t.popover_foreground)
            .rounded(px(data::MARK_RADIUS))
            .px(px(data::DENSITY_DENSE_BUTTON_PAD_X))
            .py(px(data::DENSITY_DENSE_BUTTON_PAD_Y))
            .child(text(move || label.clone()).text_size(px(data::TYPE_CHIP))),
    )
}
