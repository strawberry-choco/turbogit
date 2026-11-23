//! The scrollbar over nsh-base's scrollbar(): the app's bar a scroll view
//! follows. The thumb names the foreground token, half transparent.

use nsh_base::{ScrollbarMode, ScrollbarTarget, scrollbar as base_scrollbar, theme};
use nsh_core::geometry::Axis;
use nsh_core::prelude::*;

/// The props of [`scrollbar`].
pub struct ScrollbarProps {
    /// The scroll view or list it follows and moves: a Rust view's ref.
    pub target: ScrollbarTarget,
    /// Which axis the track runs along.
    pub axis: Axis,
    /// When it shows.
    pub mode: ScrollbarMode,
    /// What screen readers say of the bar.
    pub accessibility_label: Option<SharedString>,
}

impl ScrollbarProps {
    pub fn new(target: ScrollbarTarget) -> Self {
        ScrollbarProps {
            target,
            axis: Axis::Vertical,
            mode: ScrollbarMode::default(),
            accessibility_label: None,
        }
    }

    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    pub fn mode(mut self, mode: ScrollbarMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }
}

/// A scrollbar: the thumb at the foreground's 30 percent, the radius the
/// pill rounds.
pub fn scrollbar(props: ScrollbarProps) -> impl IntoElement {
    let t = theme();
    let mut bar = base_scrollbar()
        .scroll(props.target)
        .axis(props.axis)
        .mode(props.mode)
        .thumb_class("rounded-full")
        .thumb_style(move |s| s.bg(t.foreground.opacity(0.3)));
    if let Some(label) = props.accessibility_label.clone() {
        bar = bar.accessibility_label(label);
    }
    bar
}
