use turbogit_domain::model::{CommitId, RootId};

use crate::root_caches::{LogRead, LogScope};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRecovery {
    Refresh,
    ResetFilters,
    CommitLocalChanges,
}

impl LogRecovery {
    pub fn label(self) -> &'static str {
        match self {
            Self::Refresh => "Refresh",
            Self::ResetFilters => "Reset filters",
            Self::CommitLocalChanges => "Commit local changes",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogState {
    LoadingLog,
    LoadingCommits,
    LoadingDetails,
    LoadingFilters,
    CommitsFailed(String),
    DetailsFailed(String),
    ChangesFailed(String),
    FilteringFailed(String),
    FileHistoryFailed(String),
    /// The repository has no history at all.
    NothingCommitted,
    /// History exists; the active filters exclude all of it.
    FilteredOut,
    /// The scoped file has no commits touching it.
    FileHistory,
    Ready,
}

impl LogState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::LoadingLog => "Loading Git Log…",
            Self::LoadingCommits => "Loading commits…",
            Self::LoadingDetails => "Loading selected details",
            Self::LoadingFilters => "Applying Filters…",
            Self::CommitsFailed(_) => "Error loading commits",
            Self::DetailsFailed(_) => "Error loading commit details",
            Self::ChangesFailed(_) => "Error loading changes",
            Self::FilteringFailed(_) => "Error filtering commits",
            Self::FileHistoryFailed(_) => "Error calculating file history",
            Self::NothingCommitted => "No changes committed.",
            Self::FilteredOut => "No commits matching filters",
            Self::FileHistory => "File history",
            Self::Ready => "Changes log",
        }
    }

    /// The message the failing read answered with, beside the state's own
    /// label: the label says *what* died, this says *why*.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::CommitsFailed(m)
            | Self::DetailsFailed(m)
            | Self::ChangesFailed(m)
            | Self::FilteringFailed(m)
            | Self::FileHistoryFailed(m) => Some(m),
            _ => None,
        }
    }

    pub fn is_error(&self) -> bool {
        self.reason().is_some()
    }

    pub fn is_loading(&self) -> bool {
        matches!(
            self,
            Self::LoadingLog | Self::LoadingCommits | Self::LoadingDetails | Self::LoadingFilters
        )
    }

    /// A state that reports nothing loaded, as distinct from a failed read,
    /// and as distinct from `Ready`, which is not an empty because it is not a
    /// message at all.
    pub fn is_empty_state(&self) -> bool {
        matches!(
            self,
            Self::NothingCommitted | Self::FilteredOut | Self::FileHistory
        )
    }

    /// The way out this state offers, and the machine's second rule: `Some`
    /// for every error and every empty state, `None` for a read still running
    /// and for the default.
    pub fn recovery(&self) -> Option<LogRecovery> {
        match self {
            Self::CommitsFailed(_) | Self::DetailsFailed(_) | Self::ChangesFailed(_) => {
                Some(LogRecovery::Refresh)
            }
            Self::FilteringFailed(_)
            | Self::FileHistoryFailed(_)
            | Self::FilteredOut
            | Self::FileHistory => Some(LogRecovery::ResetFilters),
            Self::NothingCommitted => Some(LogRecovery::CommitLocalChanges),
            _ => None,
        }
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

pub fn named_states() -> Vec<LogState> {
    let why = || "…".to_owned();
    vec![
        LogState::LoadingLog,
        LogState::LoadingCommits,
        LogState::LoadingDetails,
        LogState::LoadingFilters,
        LogState::CommitsFailed(why()),
        LogState::DetailsFailed(why()),
        LogState::ChangesFailed(why()),
        LogState::FilteringFailed(why()),
        LogState::FileHistoryFailed(why()),
        LogState::NothingCommitted,
        LogState::FilteredOut,
        LogState::FileHistory,
        LogState::Ready,
    ]
}

pub fn commit_list(state: &AppState, window_empty: bool) -> LogState {
    if let Some(path) = &state.ui.log_path_scope {
        return path_scope(state, path);
    }
    let roots = listed_roots(state);
    if window_empty {
        for root in &roots {
            if let Some(message) = state.caches.log_failure(&failed_listing(state, root)) {
                return LogState::CommitsFailed(message.to_owned());
            }
        }
        if !roots.iter().any(|root| listing_settled(state, root)) {
            return if state.ui.log_ref_scope.is_some() {
                LogState::LoadingCommits
            } else {
                LogState::LoadingLog
            };
        }
        return if state.ui.log_filter.trim().is_empty() && state.ui.log_ref_scope.is_none() {
            LogState::NothingCommitted
        } else {
            LogState::FilteredOut
        };
    }
    LogState::Ready
}

pub fn commit_details(state: &AppState) -> LogState {
    let Some((root, cid)) = selection(state) else {
        return LogState::Ready;
    };
    if let Some(message) = state
        .caches
        .log_failure(&LogRead::Details(root.clone(), cid.clone()))
    {
        return LogState::DetailsFailed(message.to_owned());
    }
    if !state.caches.file_stats_loaded(&root, &cid) {
        return LogState::LoadingDetails;
    }
    LogState::Ready
}

pub fn changed_files(state: &AppState) -> LogState {
    let Some((root, cid)) = selection(state) else {
        return LogState::Ready;
    };
    if let Some(message) = state.caches.log_failure(&LogRead::Changes(root, cid)) {
        return LogState::ChangesFailed(message.to_owned());
    }
    LogState::Ready
}

/// Inside a path scope this slot is always `Ready`: the scoped listing may only
/// ever show commits touching the scoped path, so the pickaxe never runs there
/// and owns no state to report ([`crate::log_display::sources`]).
pub fn filtering(state: &AppState) -> LogState {
    let query = state.ui.log_filter.trim().to_owned();
    if query.is_empty() || state.ui.log_path_scope.is_some() {
        return LogState::Ready;
    }
    let roots = crate::log_display::visible_roots(state);
    for root in &roots {
        if let Some(message) = state.caches.log_failure(&LogRead::Scope(
            root.clone(),
            LogScope::Search(query.clone()),
        )) {
            return LogState::FilteringFailed(message.to_owned());
        }
    }
    if roots
        .iter()
        .all(|root| state.caches.search_log(root, &query).is_none())
    {
        return LogState::LoadingFilters;
    }
    LogState::Ready
}

fn path_scope(state: &AppState, path: &std::path::Path) -> LogState {
    let Some(root) = state.selected_root.clone() else {
        return LogState::Ready;
    };
    let scope = LogScope::Path(path.to_path_buf());
    if let Some(message) = state
        .caches
        .log_failure(&LogRead::Scope(root.clone(), scope))
    {
        return LogState::FileHistoryFailed(message.to_owned());
    }
    match state.caches.path_log(&root, path) {
        None => LogState::LoadingCommits,
        Some([]) => LogState::FileHistory,
        Some(_) => LogState::Ready,
    }
}

fn listed_roots(state: &AppState) -> Vec<RootId> {
    if let Some((root, _)) = &state.ui.log_ref_scope {
        vec![root.clone()]
    } else {
        crate::log_display::visible_roots(state)
    }
}

fn listing_settled(state: &AppState, root: &RootId) -> bool {
    match &state.ui.log_ref_scope {
        Some((scope_root, name)) if scope_root == root => {
            state.caches.ref_log(root, name).is_some()
        }
        _ => state.caches.log(root).is_some(),
    }
}

fn failed_listing(state: &AppState, root: &RootId) -> LogRead {
    match &state.ui.log_ref_scope {
        Some((scope_root, name)) if scope_root == root => {
            LogRead::Scope(root.clone(), LogScope::Ref(name.clone()))
        }
        _ => LogRead::Commits(root.clone()),
    }
}

/// The reads whose failures produced the state the graph pane is wearing, and
/// only those. A `Refresh` that re-asked a healthy root too would drop a
/// window that has never failed, which is the collapse this machine replaced
/// coming back through the recovery path.
pub fn failed_listing_reads(state: &AppState) -> Vec<LogRead> {
    listed_roots(state)
        .iter()
        .map(|root| failed_listing(state, root))
        .filter(|read| state.caches.has_log_failure(read))
        .collect()
}

pub fn failed_details_read(state: &AppState) -> Option<LogRead> {
    let (root, cid) = selection(state)?;
    let read = LogRead::Details(root, cid);
    state.caches.has_log_failure(&read).then_some(read)
}

pub fn failed_changes_read(state: &AppState) -> Option<LogRead> {
    let (root, cid) = selection(state)?;
    let read = LogRead::Changes(root, cid);
    state.caches.has_log_failure(&read).then_some(read)
}

fn selection(state: &AppState) -> Option<(RootId, CommitId)> {
    state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref())
        .map(|(root, cid)| (root.clone(), cid.clone()))
}
