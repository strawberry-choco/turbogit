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
//! - **No scope axis.** A `Repo | Staged | Local` chip group once selected the
//!   working-tree comparison pair (HEAD↔worktree, HEAD↔index, index↔worktree).
//!   It is deliberately removed: three places wrote that value and two of them
//!   were the app overriding the user, and one control the app overrode twice
//!   is not a control. The working-tree read is pinned to one pair,
//!   **HEAD ↔ worktree**, so every changed line of a file is on screen whether or
//!   not it is staged, and the view is stable under the only action it offers
//!   because staging a line cannot change `git diff HEAD`. Staged-ness is a
//!   per-line derived fact rather than a second view (ADR-0029). The pane header
//!   names the sides the read actually takes, so a commit-to-commit diff prints
//!   its own revisions. Staging granularity is not a second axis either: a
//!   changed row is always a line toggle and an addition always takes a
//!   char-range drag.
//! - **Hunk navigation** ‹ n/N › steps between parsed hunks.
//! - **Per-file statistics on the file row, the policies menu on the toolbar**
//!   (spec story 23): a file section's `+N −M` change counts paint on the
//!   section's file row, and the **Diff options** menu (`⌃⇧D`) holding the
//!   viewer's two five-value policies — which whitespace differences count as a
//!   change at all, and how finely a change is highlighted — paints on the
//!   toolbar. The policies are folded into the display model when the diff
//!   settles (ADR-0014) and are part of what the keyed read is keyed on; they
//!   change no byte of the patch partial staging composes from. The toolbar is
//!   painted in every state of the comparison — settled, empty, still computing
//!   or failed — so the control that decides what counts as a difference does
//!   not disappear with it.
//! - Add/del lines paint token-exact backgrounds (`DIFF_ADD_BG` /
//!   `DIFF_DEL_BG`) with muted `INK_3` gutter numbers; hunk headers sit on
//!   SURFACE (spec §2.3). None of the diff pair, the added/removed accent pair
//!   or the row fills is touched by any of the above.
//! - **Gutter staging (spec R2)**: every hunk-header band carries compact
//!   "+" / "−" controls that stage / unstage that whole hunk, and a changed row
//!   toggles one line. Neither composes from the bytes on screen: each operation
//!   is composed from the retained index-facing section its direction names and
//!   applied through the async op seam (ADR-0029). Conflicted files keep the
//!   controls visible but inert.
//! - **Non-text diffs (spec R8, ADR-0015)**: per-file section metadata is
//!   scanned beside the rows; rename metadata renders as a leading header
//!   row (a pure 100% rename shows "No content changes." instead of an
//!   empty scroller), an image pair renders as decoded pictures with
//!   dimension/size captions, and a lone binary change renders as a
//!   one-line description with byte sizes. Non-text bytes fetch off the
//!   frame path through the same async-event seam as diffs and cache
//!   decoded results beside them. A binary or pure-rename section holds no
//!   line rows, so it offers no toggle and the oracle holds no answer for it.

mod actions;
mod model;
mod panes;
mod view;

pub(crate) use actions::{preview_caret_line, preview_hunk_count, preview_line_counts};
pub use view::render_diff;
pub use view::show_diff;

// Re-exported here so the historical `ui::diff` paths keep resolving
// (DDD split issue 04).
pub use turbogit_app::diff_data::{PaneCache, PaneEntry, PaneSide};
