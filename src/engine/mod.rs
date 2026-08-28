//! Git engine layer.
//!
//! The port lives in `turbogit-engine-api` and the adapters plus the
//! backend-selection factory live in `turbogit-engine` (DDD split issue 06).
//! This module keeps re-export shims so every existing `engine::X` path
//! resolves untouched; the event types below move to the app crate in a
//! later ticket.

use crate::error::TgResult;
use crate::model::*;
use crate::root_caches::Affected;

pub use turbogit_engine::{build_executor, cli, git2_exec};
pub use turbogit_engine_api::{ApplyDirection, GitExecutor};

// The fake executor ships behind the engine crate's `test-util` feature,
// which the root crate activates only through its dev-dependencies.
#[cfg(test)]
pub use turbogit_engine::fake;

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
