//! Diff viewer (spec §8.4, issue #13).
//!
//! Restyled onto the central [`crate::theme::Palette`] tokens and the shared
//! widget vocabulary — behavior preserved, visual migration only:
//!
//! - **Async + cached** diff access through the app layer (Epic E7/J1): the
//!   patch text and the pane bytes both come from the keyed read
//!   ([`turbogit_app::keyed_read`]), which computes off the frame path and
//!   caches, so no `git diff` runs synchronously per frame. The Shell holds no
//!   Git engine, and no caller names a cache key.
//! - **Virtualized rendering (ADR-0014)**: rows paint through
//!   `ScrollArea::show_rows` over the display-row model the read answers with,
//!   built once per diff content — parsing and side-by-side pairing never
//!   run per frame, and hunk navigation scrolls by row index so unrealized
//!   rows stay reachable.
//! - **Segmented control** toggles Side-by-Side | Unified rendering.
//! - **Two axes of scope** (spec story 22), each framed as ONE grouped control
//!   and each a visibly different kind of group: `Repo | Staged | Local` select
//!   the working-tree comparison pair (Repo = HEAD↔worktree, Staged = HEAD↔index,
//!   Local = index↔worktree) in a filled pad, `File | Hunk | Line` choose what one
//!   staging action addresses in an outlined track. The second is working-tree
//!   only — commit-to-commit diffs have no index to stage into — and hides
//!   without leaving an empty frame behind. Explicit commit-to-commit targets
//!   (Git Log) keep their fixed pair and show neither.
//! - **Hunk navigation** ‹ n/N › steps between parsed hunks.
//! - **The file row carries its own statistics** (spec story 23): a file
//!   section's `+N −M` change counts, and the **Ignore whitespace** checkbox
//!   that feeds `DiffOpts::ignore_whitespace` into the engine call and the cache
//!   key. The row is painted in every state of the comparison — settled, empty,
//!   still computing or failed — so the control that produces an empty
//!   comparison does not disappear with it.
//! - Add/del lines paint token-exact backgrounds (`DIFF_ADD_BG` /
//!   `DIFF_DEL_BG`) with muted `INK_3` gutter numbers; hunk headers sit on
//!   SURFACE (spec §2.3). None of the diff pair, the added/removed accent pair
//!   or the row fills is touched by any of the above.
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

pub(crate) use actions::{chip_button, preview_hunk_count, preview_line_counts, preview_status};
pub use view::render_diff;

// Re-exported here so the historical `ui::diff` paths keep resolving
// (DDD split issue 04).
pub use turbogit_app::diff_data::{PaneCache, PaneEntry, PaneSide};
