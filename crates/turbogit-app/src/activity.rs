//! Activity log (issue #04): the session-durable record of every dispatched
//! git operation and its outcome. Toasts summarize; this feed remembers —
//! entries survive the whole session and are filterable by repo and time
//! window.

use chrono::{DateTime, Local};
use std::path::Path;

/// Severity of one activity entry; drives its color in the panel
/// (success/error coloring, issue #04).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActivityKind {
    /// Operation succeeded (`STATE_SUCCESS`).
    Success,
    /// Completed but needs attention, e.g. conflicts left behind
    /// (`STATE_WARNING`).
    Warning,
    /// Operation failed (`STATE_ERROR`).
    Error,
}

/// One recorded operation outcome.
#[derive(Clone, Debug)]
pub struct ActivityEntry {
    pub at: DateTime<Local>,
    /// Repo the operation touched (display label: the root's basename);
    /// `None` when the op spanned every root of the project.
    pub repo: Option<String>,
    /// Human-readable outcome ("Fetch from origin", "Push origin/main:
    /// rejected", …).
    pub message: String,
    pub kind: ActivityKind,
}

/// Visible time window of the feed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimeWindow {
    #[default]
    All,
    /// Only entries from the last 30 minutes.
    Last30Min,
}

/// The session-durable activity feed plus its filter state. Lives on
/// [`crate::state::UiState`] — session-scoped by design: nothing here is
/// persisted, and collapsing/expanding never touches `entries`.
pub struct ActivityLog {
    pub entries: Vec<ActivityEntry>,
    /// Repo filter: `None` shows every repo, `Some(label)` narrows.
    pub repo_filter: Option<String>,
    pub window: TimeWindow,
    /// Panel expanded? Toggling only changes this flag — entries are kept
    /// regardless (issue #04).
    pub expanded: bool,
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            repo_filter: None,
            window: TimeWindow::All,
            // Collapsed by default: the feed keeps recording while hidden
            // (appending is independent of visibility), and the expanded
            // 200px strip eats tool-window space on short displays.
            expanded: false,
        }
    }
}

impl ActivityLog {
    /// Session-long feeds must stay bounded: keep the newest
    /// [`Self::MAX_ENTRIES`] and drop the oldest overflow.
    const MAX_ENTRIES: usize = 1000;

    pub fn push(&mut self, entry: ActivityEntry) {
        self.entries.push(entry);
        if self.entries.len() > Self::MAX_ENTRIES {
            let overflow = self.entries.len() - Self::MAX_ENTRIES;
            self.entries.drain(0..overflow);
        }
    }

    /// Clear empties the feed (issue #04).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Entries matching the current repo + time-window filters,
    /// oldest-first. `now` comes from the caller so the filter is a pure
    /// function of state.
    pub fn visible(&self, now: DateTime<Local>) -> Vec<&ActivityEntry> {
        self.entries
            .iter()
            .filter(|e| {
                self.repo_filter
                    .as_ref()
                    .is_none_or(|f| Some(f) == e.repo.as_ref())
                    && match self.window {
                        TimeWindow::All => true,
                        TimeWindow::Last30Min => (now - e.at).num_minutes() < 30,
                    }
            })
            .collect()
    }
}

/// Display label for a root path: the basename, or the whole path when it
/// has no file name (e.g. `/`).
pub fn root_label(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}
