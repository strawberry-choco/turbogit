use chrono::{DateTime, Local};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActivityKind {
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct ActivityEntry {
    pub at: DateTime<Local>,
    pub repo: Option<String>,
    pub message: String,
    pub kind: ActivityKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TimeWindow {
    #[default]
    All,
    Last30Min,
}

pub struct ActivityLog {
    pub entries: Vec<ActivityEntry>,
    pub repo_filter: Option<String>,
    pub window: TimeWindow,
    pub expanded: bool,
}

impl Default for ActivityLog {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            repo_filter: None,
            window: TimeWindow::All,
            expanded: false,
        }
    }
}

impl ActivityLog {
    const MAX_ENTRIES: usize = 1000;

    pub fn push(&mut self, entry: ActivityEntry) {
        self.entries.push(entry);
        if self.entries.len() > Self::MAX_ENTRIES {
            let overflow = self.entries.len() - Self::MAX_ENTRIES;
            self.entries.drain(0..overflow);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

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
