//! The Log surface's **named state machine** — one state per thing the log
//! loads, each with the recovery that escapes it.
//!
//! Plain data owned by the application layer, like [`crate::log_display`]:
//! nothing here references egui, and the UI paints what these answer with
//! rather than deciding per call site which sentence fits.
//!
//! The machine exists because the log loads **four things independently** — the
//! commit listing, the selected commit's details, its changed files, and the
//! code-change search — and a surface-level `Reading / Empty / Populated`
//! triple cannot tell those four apart. Two rules hold it together:
//!
//! * **Every error state names the operation that failed.** Five of them —
//!   commits, commit details, changes, filtering, file history — and no two
//!   share a label. Collapsing them into one "something went wrong · Retry"
//!   is what this module exists to prevent, and
//!   [`named_states`]'s census is where that is pinned.
//! * **Every error state and every empty state carries its own recovery.**
//!   Loading states and the default carry none: they have nothing to escape
//!   from. The two empty states carry *different* actions, because they are
//!   different facts about the repository — "no history exists" sends the
//!   developer to the Commit surface, "your filters excluded the history"
//!   clears the filters. Painting the second where the first is true sends
//!   someone to reset filters they never set.
//!
//! A failure is a fact about **one read of one key**, recorded where the read
//! happens ([`crate::root_caches::RootCaches`]) and forgotten by the same read
//! answering, by [`crate::root_caches::RootCaches::forget_log_read`], or by
//! invalidation — never a surface-wide flag, which is how one pane's crash
//! comes to black out another's.
//!
//! The reference's model has one more property TurboGit's event pump cannot
//! copy directly: it claims the *whole* message slot for a failure. Here a
//! failure only claims the slot when that subject has nothing else to show;
//! a failed page of an already-listing history keeps its rows, and the
//! activity feed ([`crate::activity`]) carries the failure. Hiding two hundred
//! loaded rows behind an error banner would be a worse lie than the one this
//! machine replaces.

use turbogit_domain::model::{CommitId, RootId};

use crate::root_caches::{LogRead, LogScope};
use crate::state::AppState;

/// The action one state offers as its way out. Each is one control on the
/// pane that wears the state, and every one of them is non-destructive of a
/// context the developer did not ask to lose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRecovery {
    /// Ask the failed read again. The listing this state belongs to is the one
    /// re-asked — never "refresh the whole surface".
    Refresh,
    /// Clear the filter set: the live term, the path scope, the ref scope.
    /// The escape for a state the filters caused.
    ResetFilters,
    /// Go and commit what is in the working tree — the call to action that
    /// belongs to a repository with no history at all.
    CommitLocalChanges,
}

impl LogRecovery {
    /// The control's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Refresh => "Refresh",
            Self::ResetFilters => "Reset filters",
            Self::CommitLocalChanges => "Commit local changes",
        }
    }
}

/// One named state of the log machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogState {
    /// The tool window's very first paint: no listing of any root has settled.
    LoadingLog,
    /// A scoped listing — a ref's history — is on the pump.
    LoadingCommits,
    /// The selected commit's detail read is on the pump.
    LoadingDetails,
    /// The code-change search is on the pump.
    LoadingFilters,
    /// The commit listing failed, and there are no rows to show instead.
    CommitsFailed(String),
    /// The selected commit's details failed. The list beside it may be perfect.
    DetailsFailed(String),
    /// The selected commit's changed-file list failed.
    ChangesFailed(String),
    /// The filter read failed. Escaped by resetting the filters, since re-running
    /// the read that just failed is not the way out of a filter.
    FilteringFailed(String),
    /// A file's own history failed. Its own state because its own recovery is
    /// the filters that scoped to it.
    FileHistoryFailed(String),
    /// The repository has no history at all.
    NothingCommitted,
    /// History exists; the active filters exclude all of it.
    FilteredOut,
    /// The scoped file has no commits touching it.
    FileHistory,
    /// The default: the rows are the answer, so the message slot paints nothing.
    /// Named `Changes log` after the reference's default caption for the
    /// surface, which TurboGit never paints over a populated list.
    Ready,
}

impl LogState {
    /// The state's sentence. Every one of them is a whole string, so a test
    /// reading painted output cannot match a substring of another state and
    /// pass for the wrong reason.
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

    /// A read on the pump — the answer is coming, so the slot has nothing to
    /// escape and paints the spinner treatment.
    pub fn is_loading(&self) -> bool {
        matches!(
            self,
            Self::LoadingLog | Self::LoadingCommits | Self::LoadingDetails | Self::LoadingFilters
        )
    }

    /// A state that reports nothing loaded — as distinct from a failed read,
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

    /// Whether the pane paints anything for this state. `Ready` is the machine
    /// naming the populated case so the census is complete, not a message.
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// The machine's whole census: five loading-or-default states, five failures
/// and three empties. It exists so the two copy rules above can be checked
/// over **every** state rather than over whichever ones one surface happens to
/// paint this frame — the failure payloads carry a placeholder reason, since
/// the invariants are about labels and actions, not about a message.
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

// --- the four subjects, derived ------------------------------------------------

/// The state of the commit listing the graph pane shows, for the active scope.
///
/// `window_empty` is the derived display window's own emptiness
/// ([`crate::log_display::LogDisplay::is_empty`]) rather than something
/// recomputed here: the window is the thing the pane paints, and asking the
/// listing to re-narrow itself to decide a label would be a second derivation
/// of the same answer.
pub fn commit_list(state: &AppState, window_empty: bool) -> LogState {
    if let Some(path) = &state.ui.log_path_scope {
        return path_scope(state, path);
    }
    let roots = listed_roots(state);
    // 1. A read that failed *and left nothing to show* is the listing's own
    //    error. A failure over held rows keeps the rows — see the module note.
    if window_empty {
        for root in &roots {
            if let Some(message) = state.caches.log_failure(&failed_listing(state, root)) {
                return LogState::CommitsFailed(message.to_owned());
            }
        }
        // 2. Nothing this listing reads from has ever settled: a cold union is
        //    the first paint, a cold scope is commits loading inside it.
        if !roots.iter().any(|root| listing_settled(state, root)) {
            return if state.ui.log_ref_scope.is_some() {
                LogState::LoadingCommits
            } else {
                LogState::LoadingLog
            };
        }
        // 3. Settled, and empty. Which empty is decided by whether anything was
        //    narrowing the listing — a term, or a scope that IS a filter chip.
        return if state.ui.log_filter.trim().is_empty() && state.ui.log_ref_scope.is_none() {
            LogState::NothingCommitted
        } else {
            LogState::FilteredOut
        };
    }
    LogState::Ready
}

/// The state of the selected commit's details: the churn numbers are the one
/// part of this pane that is a read rather than a row of the listing.
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

/// The state of the selected commit's changed-file list.
pub fn changed_files(state: &AppState) -> LogState {
    let Some((root, cid)) = selection(state) else {
        return LogState::Ready;
    };
    if let Some(message) = state.caches.log_failure(&LogRead::Changes(root, cid)) {
        return LogState::ChangesFailed(message.to_owned());
    }
    LogState::Ready
}

/// The state of the code-change search — the pickaxe read, which is a listing
/// in its own right and the only one whose failure is a *filter* problem.
///
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

/// One file's own history: its failure, its loading, its empty — the three the
/// unscoped listing cannot say for it, since "this file was never touched" is
/// neither "nothing was committed" nor "the commits failed".
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

/// The roots the listing is read from: a ref scope names its own root, and an
/// unscoped view is the union of every visible one. Never one root when the
/// workspace has several — the union is the product.
fn listed_roots(state: &AppState) -> Vec<RootId> {
    if let Some((root, _)) = &state.ui.log_ref_scope {
        vec![root.clone()]
    } else {
        crate::log_display::visible_roots(state)
    }
}

/// Whether the listing `root` answers from has settled at all. The scope picks
/// which window that is, exactly as [`crate::log_display::sources`] picks it
/// for the rows: a ref scope reads its own window and nothing else, so a root
/// with a full unscoped window and an unread ref scope is still loading.
fn listing_settled(state: &AppState, root: &RootId) -> bool {
    match &state.ui.log_ref_scope {
        Some((scope_root, name)) if scope_root == root => {
            state.caches.ref_log(root, name).is_some()
        }
        _ => state.caches.log(root).is_some(),
    }
}

/// The read the listing for `root` is actually answering from: a ref scope's
/// own scoped read, or the root's unscoped window. The failure record is keyed
/// by the read that ran, so the lookup has to name the same one.
fn failed_listing(state: &AppState, root: &RootId) -> LogRead {
    match &state.ui.log_ref_scope {
        Some((scope_root, name)) if scope_root == root => {
            LogRead::Scope(root.clone(), LogScope::Ref(name.clone()))
        }
        _ => LogRead::Commits(root.clone()),
    }
}

/// The reads whose failures produced the state the graph pane is wearing — and
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

/// The details pane's failed read, when the pane is wearing `Refresh`.
pub fn failed_details_read(state: &AppState) -> Option<LogRead> {
    let (root, cid) = selection(state)?;
    let read = LogRead::Details(root, cid);
    state.caches.has_log_failure(&read).then_some(read)
}

/// The changed-files pane's failed read, when the pane is wearing `Refresh`.
pub fn failed_changes_read(state: &AppState) -> Option<LogRead> {
    let (root, cid) = selection(state)?;
    let read = LogRead::Changes(root, cid);
    state.caches.has_log_failure(&read).then_some(read)
}

/// The commit the details and changed-files panes are both answering about.
fn selection(state: &AppState) -> Option<(RootId, CommitId)> {
    state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref())
        .map(|(root, cid)| (root.clone(), cid.clone()))
}
