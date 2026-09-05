//! Inline banner (issue #02): a severity-tinted strip with one or more
//! deep-link actions. Surfaces show banners for follow-up context
//! ("2 repos rejected the cascade push → Review", "Stash restored → View
//! diff"). The struct lives in `turbogit-app` because `UiState` owns it;
//! the UI layer renders it.

/// Semantic severity — drives the strip's accent color, matching the
/// same `STATE_*` tokens the toast uses (issue #22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerSeverity {
    Info,
    Warning,
    Error,
    Success,
}

/// One clickable deep-link action on a banner (issue #02).
///
/// The closure runs against the full [`crate::state::AppState`]:
/// "Review" navigates to a surface, "View diff" opens a tool window, etc.
/// Closures are `Box<dyn FnOnce>` so a banner built with `.action(...)`
/// stays a value type — perfect for a single test-set / single paint
/// pattern. The closure is consumed when the user clicks.
pub struct BannerAction {
    pub label: String,
    pub on_click: Box<dyn FnOnce(&mut crate::state::AppState)>,
}

impl BannerAction {
    pub fn new(
        label: impl Into<String>,
        on_click: impl FnOnce(&mut crate::state::AppState) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            on_click: Box::new(on_click),
        }
    }
}

/// A banner to display. Owned (not borrowed) so the surface can build it
/// once and assign to `state.ui.banner` without lifetime juggling.
pub struct Banner {
    pub severity: BannerSeverity,
    pub message: String,
    pub actions: Vec<BannerAction>,
}

impl Banner {
    pub fn new(severity: BannerSeverity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            actions: Vec::new(),
        }
    }

    /// Append a deep-link action. Builder-style so the call site reads
    /// like the issue example: `Banner::new(Info, "...").action(...)`.
    pub fn action(mut self, a: BannerAction) -> Self {
        self.actions.push(a);
        self
    }
}
