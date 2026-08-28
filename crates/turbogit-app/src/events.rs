//! Events posted from worker threads back to the UI thread over a channel.
//!
//! Defined here (DDD split issue 08) so the engine port never references
//! them and their producers can live in any worker-thread module. The app
//! drains the channel each frame and calls `ctx.request_repaint()`.

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{Branch, Commit, RootId, RootStatus};

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
}
