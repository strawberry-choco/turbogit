//! Events posted from worker threads back to the UI thread over a channel.
//!
//! Defined here (DDD split issue 08) so the engine port never references
//! them and their producers can live in any worker-thread module. The app
//! drains the channel each frame and calls `ctx.request_repaint()`.

use std::path::PathBuf;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::{BlameLine, Branch, Commit, RootId, RootStatus, Submodule, Worktree};

use crate::root_caches::Affected;

/// A decoded image ready for GPU upload on the UI thread (spec R8):
/// dimensions plus straight (unmultiplied-alpha) RGBA8 pixels, row-major.
/// Produced on worker threads; only the upload itself touches egui.
#[derive(Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One fetched side of a non-text diff pane (spec R8): the raw byte length
/// (drives the binary-change caption) plus the decoded image when the side
/// was decodable within the size cap.
#[derive(Debug)]
pub struct FetchedBlob {
    pub byte_len: u64,
    pub decoded: Option<DecodedImage>,
}

/// Events posted from worker threads back to the UI thread over a channel.
///
/// The app drains these in `update()` and calls `ctx.request_repaint()`.
#[derive(Debug)]
pub enum AppEvent {
    /// A status scan for one root completed (or failed).
    StatusScanned {
        root: RootId,
        status: TgResult<RootStatus>,
    },
    /// Roots were (re)discovered.
    RootsDetected(Vec<RootId>),
    /// Branches for a root were loaded.
    BranchesLoaded {
        root: RootId,
        branches: TgResult<Vec<Branch>>,
    },
    /// Log for a root was loaded.
    LogLoaded {
        root: RootId,
        commits: TgResult<Vec<Commit>>,
    },
    /// Generic asynchronous completion (e.g. push/pull finished). `affected`
    /// declares which roots the op touched so the post-op refresh can be
    /// scoped (root-caches deepening, decision 6).
    OpCompleted {
        label: String,
        affected: Affected,
        result: TgResult<()>,
        /// Optional replay handle attached when the op was dispatched via
        /// [`crate::state::AppState::run_git_with_retry`]. On `Err` the
        /// error toast carries it as a `Retry` button (issue #02).
        retry: Option<crate::state::RetryAction>,
    },
    /// Fatal / unexpected error to surface in the UI.
    Error(String),
    /// App is ready (roots initialized, first scan dispatched).
    Ready,
    /// An asynchronously-computed diff is ready (keyed to avoid races).
    DiffReady {
        key: String,
        result: TgResult<String>,
    },
    /// Blame lines for the open blame target (issue 18) are ready — keyed
    /// like [`AppEvent::DiffReady`] so a result for a since-changed target
    /// is dropped instead of painted.
    BlameReady {
        key: String,
        result: TgResult<Vec<BlameLine>>,
    },
    /// Raw bytes for the open non-text diff pane (image/binary, spec R8)
    /// are ready — fetched off the frame path and keyed like
    /// [`AppEvent::DiffReady`]. A `None` side means missing (new/deleted
    /// file), unreadable, or over the size cap; the pane falls back to the
    /// binary-change rendering.
    FileBytesReady {
        key: String,
        old: Option<FetchedBlob>,
        new: Option<FetchedBlob>,
    },
    /// Ahead/behind counts for a root's current branch were computed.
    AheadBehind {
        root: RootId,
        ahead: usize,
        behind: usize,
    },
    /// One background incoming-check poll finished (issue #27): the fresh
    /// ahead/behind counts after the fetch, or the error that failed it.
    /// `Ok(None)` means the root has no upstream and was skipped.
    IncomingPolled {
        root: RootId,
        result: TgResult<Option<(usize, usize)>>,
    },
    /// The linked-worktree list of a root was loaded (issue 14). `epoch`
    /// guards against stale fills: a fetch started before a worktree
    /// mutation is dropped when its epoch no longer matches (ticket 02).
    WorktreesLoaded {
        root: RootId,
        worktrees: TgResult<Vec<Worktree>>,
        epoch: u64,
    },
    /// A worktree-mutating operation (add/remove, ticket 02) just completed
    /// for `root`: invalidate the cached list and bump its epoch so the next
    /// fill refetches the post-mutation list.
    WorktreesMutated { root: RootId },
    /// One linked worktree's dirty probe settled (ticket 04): the row at
    /// `path` updates independently of every other row.
    WorktreeDirty {
        root: RootId,
        path: PathBuf,
        dirty: TgResult<bool>,
    },
    /// The submodule list of a root was loaded (issue 14).
    SubmodulesLoaded {
        root: RootId,
        submodules: TgResult<Vec<Submodule>>,
    },
    /// A confirmed bulk operation (issue 09) finished: one result per
    /// planned root. Drained into an aggregate toast + activity entry and
    /// a full refresh (the op touched several roots).
    BulkCompleted {
        label: String,
        results: Vec<(RootId, TgResult<()>)>,
    },
    /// A cascade-run monitor transition (issue 10): one row started,
    /// finished (with duration), or was halted by a stop. Drained into the
    /// live monitor rows in `ui.bulk_run`.
    BulkRunProgress {
        event: turbogit_services::bulk_run::RunEvent,
    },
}
