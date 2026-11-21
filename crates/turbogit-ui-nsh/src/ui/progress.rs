//! The progress bar over nsh-base's progress(): the brand ink on the muted
//! track, for an operation's share or its unknown duration.

use nsh_base::progress as base_progress;
use nsh_core::prelude::*;

/// The props of [`progress`].
pub struct ProgressProps {
    /// The share of the work, 0 to 100.
    pub value: f32,
    /// The extent is unknown: connecting, preparing.
    pub indeterminate: bool,
    /// What screen readers say: what is in progress.
    pub accessibility_label: Option<SharedString>,
    pub test_id: Option<SharedString>,
}

impl ProgressProps {
    pub fn new(value: f32) -> Self {
        ProgressProps {
            value,
            indeterminate: false,
            accessibility_label: None,
            test_id: None,
        }
    }

    pub fn indeterminate(mut self) -> Self {
        self.indeterminate = true;
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

/// A progress bar: the muted track, the primary ink at the share, the
/// track's radius the control radius rounds.
pub fn progress(props: ProgressProps) -> impl IntoElement {
    let mut bar = base_progress()
        .track_class("h-1.5 rounded-sm bg-muted")
        .indicator_class("h-1.5 rounded-sm bg-primary")
        .indeterminate(props.indeterminate)
        .hover(|s| s.opacity(1.0));
    if !props.indeterminate {
        bar = bar.value(props.value);
    }
    if let Some(label) = props.accessibility_label.clone() {
        bar = bar.accessibility_label(label);
    }
    let host = view().flex_col().child(bar);
    match props.test_id.clone() {
        Some(id) => view().test_id(&id).child(host),
        None => view().child(host),
    }
}
