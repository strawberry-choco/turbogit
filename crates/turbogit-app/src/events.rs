use std::path::PathBuf;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::{
    BlameLine, ChangeStats, Commit, CommitId, CommitRef, Patch, RootId, RootStatus, Stash,
    Submodule, Worktree,
};

use crate::root_caches::{Affected, LogScope};

/// A decoded image ready for GPU upload on the UI thread: dimensions plus
/// straight (unmultiplied-alpha) RGBA8 pixels, row-major. Produced on worker
/// threads; only the upload itself touches egui.
#[derive(Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug)]
pub struct FetchedBlob {
    pub byte_len: u64,
    pub decoded: Option<DecodedImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogBatchMode {
    Replace,
    /// The batch continues a window whose oldest row is `anchor`, so it must
    /// lead with that row. If it leads with anything else the listing moved
    /// under the request, a commit, a checkout, a fetch, and the batch is
    /// torn (plan P3).
    Append {
        anchor: CommitId,
    },
}

#[derive(Debug)]
pub enum AppEvent {
    StatusScanned {
        root: RootId,
        status: TgResult<RootStatus>,
    },
    LogLoaded {
        root: RootId,
        commits: TgResult<Vec<Commit>>,
        mode: LogBatchMode,
    },
    LogBatchLoaded {
        root: RootId,
        scope: LogScope,
        commits: TgResult<Vec<Commit>>,
        mode: LogBatchMode,
    },
    RefsLoaded {
        root: RootId,
        deco: TgResult<Vec<(CommitId, Vec<CommitRef>)>>,
    },
    FileStatsLoaded {
        root: RootId,
        commit: CommitId,
        stats: TgResult<ChangeStats>,
    },
    OpCompleted {
        kind: crate::operation::OpKind,
        label: String,
        affected: Affected,
        result: TgResult<()>,
    },
    Error(String),
    /// The policies travel with the answer: the display model is folded under the
    /// policy the request was made under, never under whatever the viewer has
    /// since been switched to.
    DiffReady {
        key: String,
        policies: turbogit_domain::model::DiffPolicies,
        result: TgResult<Patch>,
    },
    BlameReady {
        key: String,
        result: TgResult<Vec<BlameLine>>,
    },
    /// fetched off the frame path and keyed like [`AppEvent::DiffReady`]. A
    /// `None` side means missing (new/deleted file), unreadable, or over the
    /// size cap; the pane falls back to the binary-change rendering.
    FileBytesReady {
        key: String,
        old: Option<FetchedBlob>,
        new: Option<FetchedBlob>,
    },
    /// One file section's text for a context reveal. `Ok(None)` is an answer of
    /// "no text to reveal": a side that is missing, unreadable, or not UTF-8.
    /// `comparison` is the key the reveal counts are kept under, verbatim, so
    /// folding the answer in never has to take a key apart.
    ContextReady {
        key: String,
        comparison: String,
        file: usize,
        result: TgResult<Option<std::sync::Arc<crate::reveals::SideText>>>,
    },
    AheadBehind {
        root: RootId,
        ahead: usize,
        behind: usize,
    },
    /// `Ok(None)` means the root has no upstream and was skipped.
    IncomingPolled {
        root: RootId,
        result: TgResult<Option<(usize, usize)>>,
    },
    WorktreesLoaded {
        root: RootId,
        worktrees: TgResult<Vec<Worktree>>,
        epoch: u64,
    },
    WorktreesMutated {
        root: RootId,
    },
    WorktreeDirty {
        root: RootId,
        path: PathBuf,
        dirty: TgResult<bool>,
    },
    SubmodulesLoaded {
        root: RootId,
        submodules: TgResult<Vec<Submodule>>,
    },
    /// The stash listing of a root was loaded. An empty listing is a normal
    /// answer and is stored; a failure keeps the cache cold, so the pane asks
    /// again instead of painting "no stashes" over a repository git refused to
    /// read.
    StashesLoaded {
        root: RootId,
        stashes: TgResult<Vec<Stash>>,
    },
    BulkCompleted {
        label: String,
        results: Vec<(RootId, TgResult<()>)>,
    },
    RemoteDefined {
        root: RootId,
        name: String,
        result: TgResult<()>,
    },
    BulkRunProgress {
        event: turbogit_services::bulk_run::RunEvent,
    },
}
