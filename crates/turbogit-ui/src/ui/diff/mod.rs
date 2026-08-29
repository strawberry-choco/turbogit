//! Diff viewer (spec §8.4, issue #13).
//!
//! Restyled onto the central [`crate::theme::Palette`] tokens and the shared
//! widget vocabulary — behavior preserved, visual migration only:
//!
//! - **Async + cached** engine access through the [`GitExecutor`] seam
//!   (Epic E7/J1): diffs are computed on a worker thread and cached, so no
//!   `git diff` runs synchronously per frame.
//! - **Virtualized rendering (ADR-0014)**: rows paint through
//!   `ScrollArea::show_rows` over a memoized display-row model built once
//!   per diff beside `diff_cache` — parsing and side-by-side pairing never
//!   run per frame, and hunk navigation scrolls by row index so unrealized
//!   rows stay reachable.
//! - **Segmented control** toggles Side-by-Side | Unified rendering.
//! - **Revision chips** select the working-tree comparison pair:
//!   Repo = HEAD↔worktree, Staged = HEAD↔index, Local = index↔worktree.
//!   Explicit commit-to-commit targets (Git Log) keep their fixed pair and
//!   hide the chips.
//! - **Hunk navigation** ‹ n/N › steps between parsed hunks.
//! - **Ignore whitespace** feeds `DiffOpts::ignore_whitespace` into the
//!   engine call and the cache key.
//! - Add/del lines paint token-exact backgrounds (`DIFF_ADD_BG` /
//!   `DIFF_DEL_BG`) with muted `INK_3` gutter numbers; hunk headers sit on
//!   SURFACE (spec §2.3).
//! - **Gutter staging (spec R2)**: every hunk-header band carries compact
//!   "+" / "−" controls that stage / unstage that whole hunk by composing a
//!   patch from the cached raw diff (ADR-0013) and applying it through the
//!   async op seam. Conflicted files keep the controls visible but inert.
//! - **Non-text diffs (spec R8, ADR-0015)**: per-file section metadata is
//!   scanned beside the rows; rename metadata renders as a leading header
//!   row (a pure 100% rename shows "No content changes." instead of an
//!   empty scroller), an image pair renders as decoded pictures with
//!   dimension/size captions, and a lone binary change renders as a
//!   one-line description with byte sizes. Non-text bytes fetch off the
//!   frame path through the same async-event seam as diffs and cache
//!   decoded results beside them.

mod actions;
mod model;
mod panes;
mod view;

pub(crate) use actions::preview_hunk_count;
pub use model::{RowSummary, parsed_rows};
pub use view::render_diff;

// Re-exported here so the historical `ui::diff` paths keep resolving
// (DDD split issue 04).
pub use turbogit_app::diff_data::{PaneCache, PaneEntry, PaneSide};
