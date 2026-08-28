//! Git engine layer.
//!
//! The `GitExecutor` trait is the **only** thing that talks to git. The plain
//! [`cli::CliExecutor`] shells out to the system `git` binary; the
//! [`git2_exec::Git2Executor`] alternative runs supported operations
//! in-process via libgit2 and falls back to the CLI for the rest. All
//! mutating ops always go through the CLI.
//!
//! See `product-spec.md` §10 and `execution-plan.md` §3.

pub mod cli;
#[cfg(test)]
pub mod fake;
pub mod git2_exec;

use crate::error::TgResult;
use crate::model::*;
use crate::root_caches::Affected;

// The engine port lives in its own crate (DDD split issue 05); re-exported
// here so every existing `engine::GitExecutor` / `engine::ApplyDirection`
// path keeps resolving.
pub use turbogit_engine_api::{ApplyDirection, GitExecutor};

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

/// Construct the engine for `settings` behind the seam (ADR-0001): selects
/// the libgit2-backed executor when `settings.backend` asks for it,
/// otherwise the plain CLI executor (library-migration plan Phase L2).
/// Callers rebuild this whenever settings change (e.g. the settings modal's
/// Apply), exactly like they rebuilt a bare [`cli::CliExecutor`] before.
pub fn build_executor(settings: &VcsSettings) -> std::sync::Arc<dyn GitExecutor> {
    let cli = cli::CliExecutor {
        settings: settings.clone(),
    };
    match settings.backend {
        GitBackend::Libgit2 => std::sync::Arc::new(git2_exec::Git2Executor::new(cli)),
        GitBackend::Cli => std::sync::Arc::new(cli),
    }
}
