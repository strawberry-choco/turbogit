//! Git Log four-pane workspace (issue #12, spec §8.3): branches pane,
//! graph pane, changed-files pane and commit-details pane. Since issue #19
//! the legacy History tab is gone: file history lives here as a path-scoped
//! view ("Show history for file…" on a changed-file entry).
//!
//! Layout (spec §8.3, re-chromed on the shared pane vocabulary — R2/R7):
//! 1. **Branches** (left, 210px): a [`widgets::card`] wearing the one shared
//!    pane header, then live search; LOCAL / REMOTE / TAGS groups fed by ref
//!    decorations; a bottom `ROOTS` filter for multi-root projects.
//! 2. **Graph** (center): the same shared header, a live search toolbar, a
//!    root-swatch legend, and the commit table under the shared column-header
//!    row (`ROOTS | HASH | AUTHOR | MESSAGE | DATE`) with one collapsed ref
//!    marker per decorated commit — the ref names (branch=brand, remote=success,
//!    tag=warning) are revealed in its hover tooltip — and the shared row
//!    vocabulary for the rows themselves: the translucent `selection_bg()` focus
//!    band (which composites over this pane to `#233455`, the same opaque value
//!    as the selected-list-row fill) **plus the one 2px `BRAND` rail** at the
//!    leading edge of a chosen row, and ink that does not change with selection.
//!    The rail and the per-root swatch share that leading edge — see [`GUTTER`],
//!    which is the whole width of the **ROOTS column**.
//!
//!    **The graph pane is a card, and the argument that used to say it was not
//!    is inverted rather than deleted** (R2, R7). The old comment read: *it is
//!    the remaining body, and a card over the whole of it would be a box around
//!    a box.* That was true, and it stopped being true the moment the columns
//!    stopped touching. The graph pane is no longer "the rest of the panel"; it
//!    is a region of its own with [`PANE_COLUMN_GAP`] of app background on either
//!    side of it, bounded on the left by the branches card and on the right by
//!    the right column. A region separated from its neighbours by air is one
//!    surface — R2's *cards are surfaces* — and the "rest" it used to be is
//!    precisely what made it read as a box: a card with no edges of its own can
//!    only be a box drawn around something else. The three columns are now peers,
//!    separated by air, which is the arrangement the approved frame shows.
//! 3. **Changed files** (right-top, 320px): a card with the shared header and
//!    the file count as a count chip beside the title; the selected commit's
//!    files with status badges; clicking loads the diff.
//! 4. **Commit details** (right-bottom, 440px): a card with the shared header
//!    — the subject, the hash chip, the **key/value fact list** (author, date,
//!    committer, parents) above the message, the churn summary, and the full
//!    message below. The pane says things about a commit and does nothing to one
//!    (ADR-0024): every action lives in the row's context menu, and the parent
//!    hashes stay the one link it owns. The list is built by
//!    [`commit_detail_rows`] over the [`Commit`] the log already holds; it is
//!    **not** the commit menu's action gates, which are a different question.
//!
//! The four headers are [`widgets::pane_header`] and the column row is
//! [`widgets::column_header`]; there is no second header in this file, and
//! `tests/git_log.rs` proves it by comparing all four headers' painted geometry
//! to each other.
//!
//! Every row this window paints — the commit table's, the changed-files pane's
//! and the ROOTS filter's — goes through the shared row shell in its **railed**
//! variant, [`components::row_shell`], over a rect the row has already
//! allocated. That is the whole of ticket 14's adoption: the shell takes a
//! caller-allocated rect, so the log keeps its measured pitch, its virtualisation
//! and its eliding, and shares the fill, the rail and the ink ramp instead of
//! re-deciding them. `tests/branch_component_kit.rs` pins the "already-allocated
//! rect" half of that contract from the shell's own source.
//!
//! **The blame view is a replacement, not a neighbour** (conformance issue 15).
//! It occupies this table's slot in the same region, so it reads its row height
//! and its cell offsets from [`CommitTable`] — the same numbers, published rather
//! than re-spelled — and paints its rows through the same [`paint_log_row`]. A
//! second table with its own height and its own offsets is what made switching to
//! blame a visual reset, and the ticket is the one that ends it.

use crate::theme::Palette;
use crate::ui::branch_tree_view::{self, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branches::fetch_scope;
use crate::ui::branches_tree::build_branch_view;
use crate::ui::commit_menu::{CommitFacts, CommitMenuAction, commit_menu};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets::{self, BadgeKind, MenuItemKind, MenuItemProps, RefKind, menu_item};
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Grid, Layout, Panel, Popup,
    PopupKind, Pos2, Rect, Response, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetType,
};
use std::ops::Range;
use std::path::PathBuf;
use turbogit_app::root_caches::{LogScope, file_stat};
use turbogit_app::state::{
    AppState, BlameTarget, Dialog, DiffTarget, NewBranchBase, PendingConfirm, Toast,
};
use turbogit_domain::model::{
    BranchKind, ChangeStatus, Commit, CommitId, DateFormat, GitRefKind, RefState, Root, RootId,
    SignatureState,
};
use turbogit_services::sync_service;

// --- Pane metrics (spec §8.3) -------------------------------------------------

/// Branches pane width.
const BRANCHES_WIDTH: f32 = 210.0;
/// Right column (changed files + details) width. Widened from the §8.3 320px
/// for the redesigned two-line file rows (decision D3): name, directory and
/// the `+N −M` column need the room.
const FILES_WIDTH: f32 = 344.0;
/// Commit details pane height. Grew from the §8.3 200px in issue 15 for the
/// Actions section, to 340px in issue 17 for the committer row and the
/// Copy-hash header, and to 440px for the redesigned blocks (decision D4):
/// subject, hash chip, author card, the committer / date / parents grid, the
/// churn summary, and the message body.
///
/// The actions, the header's copy button and the guardrail alert that D4 was
/// sized around are gone (ADR-0024), so the pane now has slack at its foot. The
/// height is left where it is rather than re-fitted: it is a design value with
/// pixel snapshots standing behind it, not a consequence of the removal, and
/// re-fitting it is a decision of its own. The short-window yield below still
/// applies.
const DETAILS_HEIGHT: f32 = 440.0;
/// Changed-file row height (redesign issue 04): a name line with its directory
/// underneath, so two lines where [`crate::theme::FILE_ROW_HEIGHT`] is one.
/// Deliberately local rather than a third ramp step — this is the log table's
/// own two-line row, and two-line rows must not retarget the graph's geometry.
const LOG_FILE_ROW_HEIGHT: f32 = 40.0;
/// Gap between the two churn numbers at the right edge of a file row.
const STAT_GAP: f32 = 6.0;
/// Root swatch width on multi-root rows — the ROOTS column's own content.
const STRIPE_WIDTH: f32 = 3.0;
/// Width of a commit row's leading gutter in a multi-root listing: the one
/// shared rail's width plus [`STRIPE_WIDTH`].
///
/// **This is the rail-versus-stripe decision, made once (conformance issue 14),
/// and it is also the ROOTS column's width (conformance issue 15). The two share
/// the gutter, rail first.** The rail leads flush at the row's leading edge —
/// where [`components::paint_rail`] puts it, and where every other rail in the
/// app is — and the per-root swatch sits immediately inside it, so both are on
/// screen at the same time and neither is ever displaced by the other. The 5 px
/// the ticket expected this to cost does not arise, because the gutter was
/// *already* 5 px wide: 3 px of swatch plus 2 px of air. The rail takes the air,
/// the cells do not move by a point, and the selected row — the one row the user
/// is reading — keeps its root colour.
///
/// The rail's width is the token layer's and only its one painter may name it,
/// so the gutter is written as the single number the log's geometry needs and
/// the swatch is placed at `GUTTER - STRIPE_WIDTH`, flush against the rail's
/// trailing edge. `tests/git_log.rs` proves the arrangement from painted
/// geometry: rail then swatch, both present on the selected row, and the same
/// content offset the column header measures from.
///
/// **A 5px column and a 9px label do not fit, and the honest answer is that this
/// column's label overhangs its own width.** [`COL_ROOTS`] therefore measures
/// *backwards* from the header row's origin by exactly the gutter, which puts the
/// label at the row's leading edge and the column's cells inside the gutter the
/// rail and the swatch already occupy. The overhang is a header-only artefact:
/// the header band is empty to the right of the label, and the cells the
/// overhang runs toward are the unlabelled graph lane. What it may never do is
/// reach the next label, so `tests/git_log.rs` asserts `ROOTS` ends clear of
/// `HASH` rather than asserting the label fits its column — a fit is not
/// available at this width and pretending otherwise would be a lie in the test.
const GUTTER: f32 = 5.0;
/// The air that divides the two cards stacked in the log's right column (R2).
///
/// A region divides itself from its neighbour with the space around it, and this
/// is that space: 8 px, the margin those panels' frames already carried, so the
/// gap reads as the same separation the surface swap replaced rather than as a
/// number invented for this ticket. There is no rule between the two cards and
/// no raised band wrapping both — one of those is the nested-boxes problem this
/// migration exists to remove, and the other is what it removed.
const PANE_GAP: f32 = 8.0;
/// The air that divides the log's three **columns** (R2) — 10pt of app
/// background between the branches card and the commits card, and between the
/// commits card and the changed-files card. The approved Log frame is what the
/// number is measured from: a scan across the tool-pane band finds `#1E1F22`
/// (`Palette::BG`, the app background) for exactly ten points on each side of
/// the commits card and nowhere else.
///
/// **This is deliberately a second name, not [`PANE_GAP`] under another spelling.**
/// `PANE_GAP` is the rhythm *inside* the right column, where two cards stack
/// against each other; this is the rhythm *between* columns, where three cards
/// sit abreast. The two are different numbers drawn from the same frame (8
/// stacked, 10 abreast) and they answer different questions, so one name for
/// "the gap" is exactly how a 2 becomes a 10 by accident in one direction or a
/// 10 becomes an 8 in the other. `PANE_GAP`'s existing use is unchanged.
///
/// Nothing draws this: it is *air*, and the app background is already painted
/// behind the log body by the central panel. The layout's whole job is to stop
/// covering it, which is why this ratchets from the painted output — a
/// "separation" test that could be satisfied by three cards that merely touch
/// would be satisfied by the layout this replaced.
const PANE_COLUMN_GAP: f32 = 10.0;
/// The one card padding the log's three panes use, in one place so the region's
/// arithmetic in [`show_log`] and the card's own frame agree. It is 8 rather
/// than the card default's [`crate::theme::PANEL_PADDING`] because 8 is the
/// margin these panels' frames already carried: swapping a surface must not
/// reflow what is inside it, and a 4-point move of every cell in three panes is
/// a reflow nobody asked for. The card default's padding answers a different
/// question — the air a card needs once its *stroke* is gone, where the stroke's
/// two pixels were the only thing separating content from edge — and nothing
/// was removed here.
const PANE_CARD_PAD: i8 = 8;
/// Uppercase micro text (§3.3) — shared control role (T2).
const MICRO_TEXT: f32 = crate::theme::TYPE_CONTROL;
/// Mono cell font size — shared body role (T2).
const MONO_TEXT: f32 = crate::theme::TYPE_BODY;

/// The log file-row status pill: the shared chip's height and text inset, at
/// the compact control radius rather than the full pill radius. This site has
/// always rounded at CONTROL_RADIUS; changing that is a design change, not a
/// refactor, so the consolidation must preserve it. Naming the geometry here
/// (rather than switching to `CHIP_GEOMETRY`) is what lets the row drop its
/// hand-rolled `rect_filled` + two-axis centring for
/// [`widgets::ChipGeometry::paint`] without moving a pixel.
const STATUS_PILL: widgets::ChipGeometry = widgets::ChipGeometry {
    height: widgets::CHIP_HEIGHT,
    pad_x: widgets::CHIP_PAD_X,
    radius: crate::theme::CONTROL_RADIUS as f32,
};

/// Commit-table column x-offsets, measured from `content_left` (the row left
/// edge, plus the root stripe in multi-root views). The micro column headers
/// and the row cells share these offsets so they stay vertically aligned.
/// `COL_GRAPH` is the graph node's center x (no header is drawn for it); the
/// remaining columns left-align with their cell text, except the date, which
/// trails the row right-aligned to `rect.right() - DATE_RIGHT_PAD`.
const COL_GRAPH: f32 = 10.0;
const COL_HASH: f32 = 26.0;
const COL_AUTHOR: f32 = 84.0;
/// Message column left edge — the wide column, sitting where the date used to.
const COL_MESSAGE: f32 = 164.0;
/// The **ROOTS column's** offset, and the only one that is negative.
///
/// Every other `COL_*` measures forward from the row's *content* edge, and the
/// header row is handed a rect that already starts clear of the gutter (see
/// [`table_content_left`]). The ROOTS column is the one column that **is** the
/// gutter, so it measures backwards by exactly that gutter: its header label
/// paints at the row's leading edge, over the rail and the per-root swatch it
/// names, and its cells fill the space between the rail's trailing edge and the
/// first cell column.
///
/// A 9px word does not fit in 5px, so the label overhangs its own column. That is
/// stated rather than hidden, and it is bounded: the header band has nothing to
/// the right of it until the unlabelled graph lane, and `tests/git_log.rs` holds
/// the label clear of the next label (`HASH`) so a wider one fails loudly.
const COL_ROOTS: f32 = -GUTTER;
/// Gap between the right-aligned date and the row's trailing edge.
const DATE_RIGHT_PAD: f32 = 8.0;
/// Space held back from the message for a label pill (icon + padding + gap)
/// so it never runs under the right-aligned date on decorated rows.
const PILL_RESERVE: f32 = 30.0;

/// The commit table's columns: one table, read by **both** the shared
/// column-header row and every data row.
///
/// The offsets are the `COL_*` values the rows have always measured from, so
/// this is not a new arrangement — it is the row's own arithmetic promoted to a
/// table so the header has nothing to invent. The graph column stays out of it
/// (it carries no header: it is the node, not a field), and the date trails the
/// row, so it is measured from the trailing edge like the cell it labels.
///
/// **The ROOTS column leads**, and it is the first entry rather than an
/// afterthought appended to the list: a free-floating label above the gutter is
/// not a header — it would carry none of the shared chrome's ink and sit outside
/// the header row's rule. In this table it is a column like any other, and the
/// header row paints it in the same pass, in the same ink, from the same
/// [`widgets::column_header`] call as `HASH`.
const COMMIT_COLUMNS: [widgets::PaneColumn; 5] = [
    widgets::PaneColumn::start("ROOTS", COL_ROOTS),
    widgets::PaneColumn::start("HASH", COL_HASH),
    widgets::PaneColumn::start("AUTHOR", COL_AUTHOR),
    widgets::PaneColumn::start("MESSAGE", COL_MESSAGE),
    widgets::PaneColumn::end("DATE", DATE_RIGHT_PAD),
];

/// Distinct lane colors for the commit graph (Epic D1). Also reused as the
/// deterministic per-root swatch palette.
const GRAPH_COLORS: &[Color32] = &[
    Color32::from_rgb(80, 140, 230),
    Color32::from_rgb(220, 120, 140),
    Color32::from_rgb(120, 200, 130),
    Color32::from_rgb(200, 170, 90),
    Color32::from_rgb(170, 130, 220),
    Color32::from_rgb(90, 190, 200),
    Color32::from_rgb(230, 150, 90),
    Color32::from_rgb(150, 200, 220),
];

/// The commit table's geometry, published for the view that replaces it.
///
/// The blame view occupies this table's slot in the same region, so it reads its
/// row height and its cell offsets from here instead of from its own literals —
/// a second private table is exactly what made switching to blame a visual
/// reset. **Constants and nothing else**: no row state, no data, nothing that
/// would let the two views drift into disagreeing about what a row *is*.
pub(crate) struct CommitTable;

impl CommitTable {
    /// A commit row's height. The blame view's rows are this tall too, so
    /// switching between the two views moves no line.
    pub(crate) const ROW_HEIGHT: f32 = crate::theme::FILE_ROW_HEIGHT;
    /// The hash cell's left edge, measured from the row's content edge.
    pub(crate) const HASH: f32 = COL_HASH;
    /// The author cell's left edge, likewise.
    pub(crate) const AUTHOR: f32 = COL_AUTHOR;
    /// The wide column's left edge — the subject here, the blamed line there.
    pub(crate) const MESSAGE: f32 = COL_MESSAGE;
    /// The inset the trailing column ends at, so a right-aligned cell tracks a
    /// row that resizes in both tables rather than in one of them.
    pub(crate) const DATE_RIGHT_PAD: f32 = DATE_RIGHT_PAD;
}

fn fmt_time(t: i64) -> String {
    match Utc.timestamp_opt(t, 0) {
        chrono::LocalResult::Single(dt) => {
            let local: DateTime<Local> = DateTime::from(dt);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}

/// Render a timestamp according to the configured format (Epic D2).
fn fmt_date(t: i64, mode: DateFormat) -> String {
    match mode {
        DateFormat::Iso => fmt_time(t),
        DateFormat::Absolute => fmt_time(t),
        DateFormat::Relative => {
            let now = Local::now().timestamp();
            let d = now - t;
            if d < 60 {
                format!("{d}s ago")
            } else if d < 3600 {
                format!("{}m ago", d / 60)
            } else if d < 86400 {
                format!("{}h ago", d / 3600)
            } else if d < 2592000 {
                format!("{}d ago", d / 86400)
            } else {
                fmt_time(t)
            }
        }
    }
}

/// Truncate a string to at most `n` chars without panicking on multibyte input.
fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The one deterministic colour for the root at `idx`.
///
/// **This is the single source of per-root colour in the app.** It has four
/// callers and no fifth palette is permitted: the ROOTS-filter row's dot, the
/// legend swatch, the ROOTS column's per-row swatch, and the commit-graph lane
/// node. All four ask this function, so "the same root is the same colour
/// everywhere" is arithmetic rather than a convention —
/// `tests/git_log.rs` compares the painted rects of the column, the lane and the
/// legend for one root index and requires them equal.
///
/// A root's colour is chosen by its position in `state.multi.roots`, wrapped
/// around the table, so it is stable for a given project and does not depend on
/// the view.
fn root_color(idx: usize) -> Color32 {
    GRAPH_COLORS[idx % GRAPH_COLORS.len()]
}

/// Where a repository sits in the project's root order — the index [`root_color`]
/// takes, so every per-root marker asks the same question and gets the same
/// answer.
///
/// A root that is not registered (a commit arriving from a root the ROOTS filter
/// hides, a row painted before the multi-root scan settled) reads as the first
/// root rather than panicking: a marker is a hint about which repository a row
/// came from, and a missing hint is better than a dropped row in a list that
/// virtualises.
fn root_index_of(state: &AppState, root: &RootId) -> usize {
    state
        .multi
        .roots
        .iter()
        .position(|r| &r.id == root)
        .unwrap_or(0)
}

// --- Data plumbing ------------------------------------------------------------

/// The roots whose history is displayed (roots-filter aware). One definition,
/// owned by the derived display window's own rule (log-view-scaling 04): the
/// ref-fill, the pager's guard and the union the held window is derived from
/// all read it from there, so "which roots are in view" cannot drift.
fn visible_root_ids(state: &AppState) -> Vec<RootId> {
    turbogit_app::log_display::visible_roots(state)
}

/// Lazily load (through the engine seam, cached) everything the four panes
/// need beyond the log itself: ref decorations per visible root, the
/// changed-file list of the selected commit, and — when a path scope is
/// active (issue #19) — the path-scoped commit listing. All fills happen
/// behind the [`turbogit_app::root_caches::RootCaches`] interface.
fn ensure_log_data(state: &mut AppState) {
    // Ref decorations load off the render thread (log-open perf, D1): kick
    // the worker per root while its decorations aren't in yet; the filled
    // cache arrives with the `RefsLoaded` event and the view renders the
    // empty-first log meanwhile. The one-per-root in-flight guard inside
    // `fetch_refs` keeps this from stacking workers frame after frame.
    for id in visible_root_ids(state) {
        if !state.caches.refs_loaded(&id) {
            state.fetch_refs(id);
        }
    }
    if let (Some(root), Some(cid)) = (
        state.selected_root.clone(),
        state.ui.selected_commit.clone(),
    ) {
        state.ensure_files(&root, &cid);
        // Per-file line counts (redesign issue 02) load off the render thread:
        // the in-flight guard makes a repeat ask a no-op, so rows render
        // without numbers for the frames a request is open rather than the
        // pane waiting on git.
        state.fetch_file_stats(root, cid);
    }
    // Path-scoped history (issue #19): the scoped listing fills through the
    // same cache read as the other two scopes.
    if let (Some(root), Some(path)) = (state.selected_root.clone(), state.ui.log_path_scope.clone())
    {
        state.ensure_log(&root, LogScope::Path(path));
    }
    // Ref-scoped history (branch-tree extraction, plan D9).
    if let Some((root, ref_name)) = state.ui.log_ref_scope.clone() {
        state.ensure_log(&root, LogScope::Ref(ref_name));
    }
    // Code-change search (issue 17): a non-empty search box also fills the
    // pickaxe cache per visible root — `git log -S` covers commits whose
    // content changed the query string's count, which client-side filtering
    // of message/hash/author cannot see.
    let query = state.ui.log_filter.trim().to_string();
    if !query.is_empty() {
        for id in visible_root_ids(state) {
            state.ensure_log(&id, LogScope::Search(query.clone()));
        }
    }
}

// --- Load more (log-view-scaling 03) --------------------------------------------

/// One loaded window the log list is drawn from, and the only thing the Load-more
/// affordance and its dispatch ever name.
///
/// A scope used to be fetched whole, so it had nothing more to offer and the
/// pane suppressed the affordance inside one. Scopes page now, so the pane
/// names windows instead of roots: the affordance is offered while ANY window
/// behind the displayed listing says its history continues past what it holds,
/// and pressing it pages exactly those windows.
enum ListedWindow {
    /// The unscoped union of every visible root's window. One entry rather than
    /// one per root, because the app's own unscoped pager
    /// ([`AppState::load_more_log`]) pages every registered root that says it has
    /// more — including a root the roots filter hides — and that is the
    /// behaviour this gate has always had.
    Unscoped,
    /// One scope's own window, in one root. A ref scope and a path scope are
    /// each the whole listing on their own — the ref scope names its own root,
    /// a path scope is read for the selected one — so a scope has exactly one
    /// window to page and no second listing to keep in step.
    Scoped(RootId, LogScope),
}

impl ListedWindow {
    /// Whether this window stops short of the end of its own history — the
    /// per-window answer, and the only one the affordance is allowed to read.
    /// Each window carries its own flag (log-view-scaling 01), so a scope
    /// exhausted while the unscoped union has pages left is not re-offered.
    fn has_more(&self, state: &AppState) -> bool {
        match self {
            Self::Unscoped => visible_root_ids(state)
                .iter()
                .any(|id| state.caches.log_has_more(id)),
            Self::Scoped(root, scope) => state.caches.scoped_log_has_more(root, scope),
        }
    }

    /// Ask this window for one more batch. A scope is paged by name through the
    /// app's off-thread scoped read — one read per `(root, scope)` in flight, so
    /// two scopes of one root never wait on each other, and the batch lands on a
    /// later frame's drain like every other worker read.
    fn load_more(&self, state: &mut AppState) {
        match self {
            Self::Unscoped => state.load_more_log(),
            Self::Scoped(root, scope) => state.load_more_scoped_log(root, scope),
        }
    }
}

/// Every loaded window behind the listing the log pane is about to paint, in
/// [`turbogit_app::log_display::sources`]'s own order and under its own scope
/// rules — a ref scope REPLACES every other listing, a path scope replaces it
/// with the selected root's window (and is nothing at all without a selected
/// root), and an unscoped view unions the visible roots. The pickaxe union is
/// then added per visible root, skipped inside a path scope, exactly as the
/// display window's own derivation skips it.
///
/// One list for the gate, the pager and the status line, so "what the button
/// loads", "whether it is offered" and "what N shown counts" are all read off
/// the same rule instead of three that can drift.
///
/// A search is a window like any other now that it batches: the app asks for it
/// from the front of its match stream and cuts the batch out of the tail, so a
/// term that matches more commits than one batch holds is a listing that grows
/// like the others rather than one frozen at the first batch. It used to be left
/// out of this list because its batch was positioned with `--skip`, which counts
/// the traversal rather than the matches, so every batch read as torn — see
/// `a_search_scope_batches_and_pages_like_a_path_or_ref_scope` for the behaviour
/// this replaced.
fn listed_windows(state: &AppState) -> Vec<ListedWindow> {
    let mut windows = match (&state.ui.log_ref_scope, &state.ui.log_path_scope) {
        (Some((root, ref_name)), _) => {
            vec![ListedWindow::Scoped(
                root.clone(),
                LogScope::Ref(ref_name.clone()),
            )]
        }
        (None, Some(path)) => state
            .selected_root
            .clone()
            .map(|root| ListedWindow::Scoped(root, LogScope::Path(path.clone())))
            .into_iter()
            .collect(),
        (None, None) => vec![ListedWindow::Unscoped],
    };
    let query = state.ui.log_filter.trim();
    if state.ui.log_path_scope.is_none() && !query.is_empty() {
        windows.extend(
            visible_root_ids(state)
                .into_iter()
                .map(|root| ListedWindow::Scoped(root, LogScope::Search(query.to_owned()))),
        );
    }
    windows
}

/// The cached commit `cid` of `root`, honoring the active path scope and the
/// ref scope like the displayed union does — borrowed over the cache slice
/// instead of cloning the whole listing for parent lookups (plan §1.3). The
/// scope rules themselves live in [`turbogit_app::log_display::sources`],
/// where the held window is derived.
fn find_commit<'a>(state: &'a AppState, root: &RootId, cid: &str) -> Option<&'a Commit> {
    if let Some((scope_root, ref_name)) = &state.ui.log_ref_scope
        && let Some(commits) = state.caches.ref_log(scope_root, ref_name)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    if let Some(path) = &state.ui.log_path_scope
        && let Some(commits) = state.caches.path_log(root, path)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    state
        .caches
        .log(root)
        .and_then(|commits| commits.iter().find(|c| c.id == cid))
}

// --- scroll-to-selected (issue 08) ---------------------------------------------

/// Where the row at `index` sits inside the scroll area's content, in points,
/// measured from the FIRST row the area built this frame (`visible`).
///
/// The offset is relative to what the area built, not to the top of the list:
/// that is what makes the same row ask for a different scroll from a different
/// place in the list, and therefore land ON the row rather than past it. The
/// arithmetic is signed on purpose — an index under `visible.start` is a
/// negative offset, and an unsigned subtraction would wrap into a scroll to the
/// far end of the history.
fn row_offset_from_built(index: usize, visible: Range<usize>, pitch: f32) -> f32 {
    (index as f32 - visible.start as f32) * pitch
}

/// Whether a row of `row_height` whose top sits `offset` points below the first
/// built row is already on screen, given a viewport that starts `view_top`
/// points below that row and is `view_height` tall.
///
/// The viewport, not the built range, is the test, and the row has to be
/// FULLY inside it. `show_rows` builds one row past the bottom edge on purpose,
/// so a row can be in the frame and still be below the fold — treating "built"
/// as "on screen" would leave a selection sitting just out of sight, which is
/// the one outcome this whole path exists to prevent.
fn row_is_in_view(offset: f32, row_height: f32, view_top: f32, view_height: f32) -> bool {
    offset >= view_top && offset + row_height <= view_top + view_height
}

/// The scroll delta that puts a row's centre on the viewport's centre.
///
/// `Ui::scroll_with_delta` is inverted against the area's offset (egui applies
/// `-delta` to it), so the delta is the viewport's centre MINUS the row's: a
/// row below the middle yields a negative delta, which moves the offset forwards
/// and reveals later history. The sign is the easy thing to get backwards, so it
/// is a named function rather than an expression at the call site.
fn centre_scroll_delta(row_centre: f32, viewport_centre: f32) -> f32 {
    viewport_centre - row_centre
}

/// The display index of `id` in the held window, or `None` when the window does
/// not hold it — a commit the live filter hid, or one outside this view's scope.
/// The lookup is over the window's own order, so the index it returns is the
/// index the list pages over. It lives here rather than on
/// [`turbogit_app::log_display::LogDisplay`] because that module is egui-free
/// state and this is a rendering question about a scroll area's rows.
fn commit_index(rows: &[turbogit_app::log_display::LogRow], id: &str) -> Option<usize> {
    rows.iter().position(|row| row.id == id)
}

fn ref_kind(kind: GitRefKind) -> RefKind {
    match kind {
        GitRefKind::Branch => RefKind::Branch,
        GitRefKind::Remote => RefKind::Remote,
        GitRefKind::Tag => RefKind::Tag,
    }
}

// --- Composition ----------------------------------------------------------------

pub fn show_log(ui: &mut Ui, state: &mut AppState) {
    // The ref scope clears when the selected repository changes (plan D9),
    // so the graph never becomes silently empty.
    if let Some((root, _)) = &state.ui.log_ref_scope
        && state.selected_root.as_ref() != Some(root)
    {
        state.ui.log_ref_scope = None;
    }
    ensure_log_data(state);

    // Fixed spec widths shrink proportionally on narrow windows so the graph
    // pane always keeps positive width and nothing clips irrecoverably
    // (issue #23: minimum sizes hold at small window sizes).
    let avail_w = ui.available_width();
    let branches_w = BRANCHES_WIDTH.min((avail_w * 0.25).max(140.0));
    let files_w = FILES_WIDTH.min((avail_w * 0.32).max(180.0));

    // Pane 1 — branches (left, 210px at full size), with the inter-column
    // gutter reserved as a wider panel whose trailing slice is left unpainted.
    Panel::left("log_branches_pane")
        .exact_size(branches_w + PANE_COLUMN_GAP)
        .resizable(false)
        // The panel paints nothing at all: the card inside it is the surface, so
        // a frame fill here would read as a second box around the card — the
        // exact nesting R2 retires. The separator line is switched off for the
        // same reason one notch further out: a 1px divider standing where the
        // column ends is an edge on a content region, which is the one thing R2
        // says a stroke must never be. The air is the separator now, and it is
        // ten points of it.
        .show_separator_line(false)
        .frame(Frame::NONE)
        .show(ui, |ui| {
            // The card takes the column's own width and stops short of the
            // panel's trailing [`PANE_COLUMN_GAP`], which is left showing the app
            // background. Nothing paints that slice, so the gutter is air rather
            // than a fourth surface.
            let region = ui.available_rect_before_wrap();
            ui.scope_builder(
                UiBuilder::new().max_rect(Rect::from_min_max(
                    Pos2::new(region.left(), region.top()),
                    Pos2::new(region.right() - PANE_COLUMN_GAP, region.bottom()),
                )),
                |ui| {
                    pane_card(ui, |ui| branches_pane(ui, state));
                },
            );
        });

    // Panes 3+4 — right column: changed files on top, details pinned below.
    // The panel is one gutter wider than the column so the same air sits on its
    // leading side, and the card is inset into it by the same constant.
    Panel::right("log_right_column")
        .exact_size(files_w + PANE_COLUMN_GAP)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE)
        .show(ui, |ui| {
            let region = ui.available_rect_before_wrap();
            ui.scope_builder(
                UiBuilder::new().max_rect(Rect::from_min_max(
                    Pos2::new(region.left() + PANE_COLUMN_GAP, region.top()),
                    Pos2::new(region.right(), region.bottom()),
                )),
                |ui| {
                    // The 300px details pane yields to short windows so the changed-
                    // files pane above it never collapses to zero height.
                    let details_h = DETAILS_HEIGHT.min((ui.available_height() - 80.0).max(96.0));
                    // The details pane's own panel, and inside it the card that is its
                    // surface. The `SURFACE` fill this frame used to carry is the
                    // "raised background parent" the card replaced: a raised band
                    // wrapping a bottom panel, on a background panel behind that.
                    Panel::bottom("log_details_pane")
                        .exact_size(details_h)
                        .resizable(false)
                        .frame(Frame::NONE)
                        .show(ui, |ui| pane_card(ui, |ui| details_pane(ui, state)));
                    // The files pane takes the rest of the column and wears its own
                    // card, so the column is two surfaces with air between them rather
                    // than one band with a panel cut out of it.
                    //
                    // The height asked for is the leftover **less one card padding**,
                    // and that subtraction is not a fudge: `Frame` paints the rect its
                    // content occupied *plus* the frame's margin, and a child ui's
                    // content starts at the region's own top edge, so a card asked to
                    // fill a region lands one padding outside it on every side. The
                    // branches and details panes are inside their own panels and cannot
                    // show that; the files pane's neighbour is 8 points away and can.
                    ui.add_space(PANE_GAP);
                    let files_h = (ui.available_height() - PANE_CARD_PAD as f32).max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), files_h),
                        Layout::top_down(Align::Min),
                        |ui| pane_card(ui, |ui| files_pane(ui, state)),
                    );
                },
            );
        });

    // Pane 2 — graph fills the remainder, in a card of its own; the blame view
    // (issue 18) takes its place while open, keeping the branches / files /
    // details panes. Both are wrapped in the SAME `pane_card` call so the slot
    // has one surface whatever is in it — the view swapped, the region did not.
    //
    // The leftovers' geometry is what makes the two gutters: the branches panel
    // already claimed its [`PANE_COLUMN_GAP`], the right column already claimed
    // its own, and the body that is left between them is the commits card's
    // region exactly. The air is not subtracted from anything, so the two outer
    // cards keep the widths they have always had.
    if state.ui.blame.is_some() {
        pane_card(ui, |ui| super::blame_view::show_blame(ui, state));
    } else {
        pane_card(ui, |ui| graph_pane(ui, state));
    }
}

/// Run one pane's body inside the log's one card.
///
/// [`widgets::card`] is the shared card, and everything it decides here is its
/// default decision: the content surface, the card radius, and **no stroke** —
/// [`widgets::CardFrame::bordered`] is reserved for a surface that floats above
/// its surroundings, and a log pane is not one. There are four cards in this
/// file's composition and they are all this function, so "a card paints no
/// stroke" is one assertion rather than four.
///
/// The padding is [`PANE_CARD_PAD`], and the other thing this adds is the
/// height. A card in a flow grows to its content, which is right for a card and
/// wrong for a *pane*: the details pane is a pinned 440 px bottom panel with a
/// height of its own, and a surface that stopped short of it would leave the
/// panel's own background showing underneath. So the card is stretched to the
/// region it was given — measured as the space actually left over, because a
/// min-height claim would be measured from wherever the content left the cursor
/// rather than from the pane's top edge, and a tall commit would then push the
/// surface past its own panel.
fn pane_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    widgets::card(
        ui,
        widgets::CardFrame::default().padded(PANE_CARD_PAD),
        |ui| {
            let height = ui.available_height();
            let out = add(ui);
            ui.add_space((height - ui.min_rect().height()).max(0.0));
            out
        },
    )
    .inner
}

// --- Pane 1: branches -------------------------------------------------------------

/// The log pane's Branches column: the ref tree, its search, and the pane
/// header above them.
///
/// Public so the shared-widget suite can render *this pane's own chrome* on its
/// own, without the log workspace's panels, sidebars and file rows around it —
/// which is what makes a cross-pane header-geometry comparison a comparison of
/// headers. It is the same function the workspace composes, not a second one.
pub fn branches_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::pane_header(ui, "BRANCHES", None, |_ui| {});
    ui.add_space(2.0);
    widgets::search_input(ui, "Search branches", &mut state.ui.log_branch_filter);
    ui.add_space(4.0);

    // Warm the shared tag cache (plan D10): whichever tool window opens
    // first warms the tags for both.
    branch_tree_view::warm_tags(state);

    // This instance has remotes visible, fixed — no toggle lives in the pane.
    state.ui.log_tree.show_remotes = true;
    // ... and they start collapsed: the remote-header rows show, their
    // branches hide until a group is expanded.

    // Build the tree over the Roots-filter-visible roots. The decorations
    // upgrade the warmed tag states (plan D8) and mark remote-tracking refs
    // whose upstream was deleted as gone (issue 17, carried through the
    // branch snapshot).
    let ids = visible_root_ids(state);
    let mut roots: Vec<Root> = ids
        .iter()
        .filter_map(|id| state.multi.by_id(id).cloned())
        .collect();
    let mut tags_by_root = state.ui.branches_tags.clone();
    for root in &mut roots {
        let mut deco: std::collections::HashMap<String, RefState> =
            std::collections::HashMap::new();
        for group in state.caches.ref_groups(&root.id) {
            for r in group {
                deco.entry(r.name.clone()).or_insert(r.state);
            }
        }
        for b in &mut root.branches {
            if b.kind == BranchKind::Remote {
                // Decorations name remote refs `remote/branch`; the snapshot
                // carries the split form (`name` + `remote`).
                let full = match &b.remote {
                    Some(r) => format!("{r}/{}", b.name),
                    None => b.name.clone(),
                };
                b.gone = deco.get(&full).is_some_and(|st| *st == RefState::Gone);
            }
        }
        if let Some(tags) = tags_by_root.get_mut(&root.id) {
            for (name, st) in tags {
                if let Some(d) = deco.get(name) {
                    *st = *d;
                }
            }
        }
    }
    // This pane shows remotes for every repository alike.
    let view = build_branch_view(&roots, &tags_by_root, &|_| state.ui.log_tree.show_remotes);

    // The shared tree component with the pane's narrower capability set
    // (plan D7): no inline rename, no per-row actions, no repo-scope picker.
    let props = TreeProps {
        view: &view,
        filter: &state.ui.log_branch_filter,
        repo_filter: None,
        tags_by_root: &tags_by_root,
        multi_repo: state.multi.roots.len() > 1,
        busy: false,
        has_any_data: true,
        merge_in_progress: false,
        last_fetch: None,
        now: chrono::Utc::now(),
        allows_rename: false,
        allows_context_menu: false,
        id_salt: "log_branch_tree",
        full_height: false,
        collapse_remotes_by_default: true,
    };
    let events = branch_tree_view::branch_tree(ui, &props, &mut state.ui.log_tree);
    for event in events {
        apply_log_tree_event(state, event);
    }

    ui.add_space(8.0);
    roots_filter_section(ui, state);
}

/// The Log pane's tree-event policy (plan D7): row activation scopes the
/// graph to that ref; group/remote toggles keep the pane's own tree state;
/// the repo header's Fetch covers the same scope as the Branches window's.
fn apply_log_tree_event(state: &mut AppState, event: TreeEvent) {
    // A modal surface owns the keyboard and pointer: tree events are ignored
    // while a dialog or confirmation is up.
    if state.ui.dialog.is_some() || state.ui.confirm.is_some() {
        return;
    }
    match event {
        TreeEvent::RowClicked { root, branch } | TreeEvent::RowActivated { root, branch } => {
            state.ui.log_ref_scope = Some((root.clone(), branch));
            state.selected_root = Some(root);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        TreeEvent::GroupToggled(group) => match group {
            TreeGroup::Local => {
                let on = state.ui.log_tree.groups.local;
                state.ui.log_tree.groups.local = !on;
            }
            TreeGroup::Tags => {
                let on = state.ui.log_tree.groups.tags;
                state.ui.log_tree.groups.tags = !on;
            }
        },
        TreeEvent::RemoteToggled { root, remote } => {
            let key = (root.clone(), remote.clone());
            if !state.ui.log_tree.collapsed_remotes.remove(&key) {
                state.ui.log_tree.collapsed_remotes.insert(key);
            }
        }
        TreeEvent::RemoteRevealToggled { .. } => {
            // The pane forces remotes on every frame, so no repository here is
            // ever rolled up and nothing reveals.
        }
        TreeEvent::FetchRequested { .. } => fetch_scope(state),
        _ => {}
    }
}

/// Bottom-of-pane ROOTS filter (multi-root): All roots / per-root rows.
fn roots_filter_section(ui: &mut Ui, state: &mut AppState) {
    ui.separator();
    widgets::group_title(ui, "Roots");

    let all_active = state.ui.log_root_filter.is_none();
    let (rect, response) = allocate_row(ui);
    paint_log_row(ui, rect, all_active, response.hovered());
    // Painted via galley (not `ui.label`) so the row's widget label stays the
    // only accessibility node carrying "All roots".
    let galley = ui
        .painter()
        .layout_no_wrap("All roots".to_owned(), body_font(), row_name_ink());
    ui.painter().galley(
        Pos2::new(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        row_name_ink(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "All roots"));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        state.ui.log_root_filter = None;
    }

    for (idx, root) in state.multi.roots.iter().enumerate() {
        let active = state.ui.log_root_filter.as_ref() == Some(&root.id);
        let label = format!("Root {}", root.id.name());
        let (rect, response) = allocate_row(ui);
        paint_log_row(ui, rect, active, response.hovered());
        if std::env::var("TG_PROBE_TRACE").as_deref() == Ok("1") {
            eprintln!(
                "[root {label}] pointer={:?} rect={rect:?} hovered={}",
                ui.input(|i| i.pointer.hover_pos()),
                response.hovered()
            );
        }
        let child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        // The root's swatch in the filter row — the same `root_color` every
        // other per-root marker asks, so the filter and the table agree.
        let cy = rect.center().y;
        child
            .painter()
            .circle_filled(Pos2::new(rect.left() + 7.0, cy), 3.5, root_color(idx));
        // Galley text again: keep the widget label unique in the tree. The ink is
        // the row's own identity step, chosen or not.
        let text_galley =
            child
                .painter()
                .layout_no_wrap(label.clone(), body_font(), row_name_ink());
        child.painter().galley(
            Pos2::new(rect.left() + 16.0, cy - text_galley.size().y / 2.0),
            text_galley,
            row_name_ink(),
        );
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Root {}", root.id.name()))
        });
        widgets::focus_ring(ui, &response);
        if response.clicked() {
            state.ui.log_root_filter = Some(root.id.clone());
        }
    }
}

// --- Pane 2: graph ------------------------------------------------------------------

fn graph_pane(ui: &mut Ui, state: &mut AppState) {
    // The pane's own header (R7), so the commit table is headed like every other
    // pane rather than by a search box that happens to sit on top of it. No
    // count chip: the window's row count is stated once, on the status line
    // under the list, and a number in two places at once is two numbers to keep
    // in step. The search box below is the pane's content, not its action slot
    // — the slot is for verbs, and this pane's one verb ("Load more") is a
    // pagination affordance that belongs with the list it pages.
    widgets::pane_header(ui, "COMMITS", None, |_ui| {});
    // Filter toolbar: live search over message / hash / author / code
    // change, with the path scope (issue #19) rendered as a removable chip
    // on the right (issue 17).
    ui.horizontal(|ui| {
        widgets::search_input(ui, "Search commits", &mut state.ui.log_filter);
        if let Some(path) = state.ui.log_path_scope.clone() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Painted as a compact ×; the accessibility label carries
                // the full verb (kittest drives it by that label).
                let remove = ui
                    .small_button(RichText::new("×").size(crate::theme::TYPE_DETAIL_TITLE))
                    .on_hover_text("Remove the path filter");
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, "Remove path filter")
                });
                if remove.clicked() {
                    state.ui.log_path_scope = None;
                }
                ui.label(
                    RichText::new(format!("Path filter: {}", path.display()))
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::BRAND),
                );
            });
        }
        if let Some((_, ref_name)) = state.ui.log_ref_scope.clone() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // The ref scope's chip — the same removable-chip gesture as
                // the path filter (plan D7/D9).
                let remove = ui
                    .small_button(RichText::new("×").size(crate::theme::TYPE_DETAIL_TITLE))
                    .on_hover_text("Remove the ref filter");
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, "Remove ref filter")
                });
                if remove.clicked() {
                    state.ui.log_ref_scope = None;
                }
                ui.label(
                    RichText::new(format!("Ref filter: {ref_name}"))
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::BRAND),
                );
            });
        }
    });

    // The display window, held: the whole loaded window sorted, live-filtered
    // and unioned with the pickaxe hits, derived when the loaded window or one
    // of the four inputs moved and read as it stands this frame
    // (log-view-scaling 04). An idle frame computes nothing here, and the
    // lane colours come with it — walked over the whole loaded window, so a
    // commit's colour does not change as it crosses the viewport.
    let display = state.sync_log_display();
    // Scroll-to-selected (issue 08): the request is one-shot, and it is taken
    // here and answered INSIDE the scroll area below. Once rows outside the
    // viewport are never built there is no realized row left to ask to scroll
    // itself into view, so the list resolves the request to a row index and
    // aims at it — the same resolution ADR-0014 reached for the diff viewer's
    // hunk navigation.
    let scroll_to = state.ui.log_scroll_to.take();
    let colors = display.lanes();
    let date_mode = state.settings.date_format;
    let multi_root = shows_root_gutter(state);

    // Pagination (issue 17, log-view-scaling 03): "Load more" is offered while
    // any window behind the displayed listing says its history continues past
    // what it holds — the unscoped union, a path or ref scope's own window, or
    // a pickaxe term's window per root. The scope suppression this replaced was
    // true only while a scope was fetched whole. The click is deferred like the
    // row selections, so the list is drawn before anything is applied to it.
    let windows = listed_windows(state);
    let may_have_more = windows.iter().any(|window| window.has_more(state));
    let mut load_more = false;

    // The ROOTS-column legend: one swatch per root, in the root's own colour,
    // under the name of the repository it belongs to. It is the legend for the
    // column below, and it asks the same `root_color` the column's cells ask —
    // which is the whole of "the same root is the same colour everywhere", and
    // is asserted from the paint rather than from the names.
    if multi_root {
        ui.horizontal_wrapped(|ui| {
            for root in &state.multi.roots {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(crate::theme::MARK_RADIUS),
                    root_color(root_index_of(state, &root.id)),
                );
                ui.label(
                    RichText::new(root.id.name())
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::INK_3),
                );
            }
        });
    }

    // Column micro-headers aligned with the cells below.
    header_cells(ui, multi_root);

    // Row clicks are deferred (plan §1.3): rows render against a shared
    // AppState — the held window is read through a handle, and the selection
    // lands after the scroll pass ends. A press carries its own root, because
    // the listing is a union across every visible root and a commit id alone
    // does not say which repository it belongs to.
    let mut clicked: Option<(RootId, String)> = None;
    let mut opened: Option<(RootId, String)> = None;
    // The height the list is given, captured before it takes it: egui 0.36
    // reports only the content's own rect back out, and an overflowing list
    // fills exactly this.
    let viewport_height = ui.available_rect_before_wrap().height();
    // Named id salt: the tool window hands every tab's body the same child id, so
    // the Worktrees and Submodules lists — which also put an unnamed
    // `ScrollArea` straight on it — would share persisted scroll offset and
    // scrollbar visibility with this one, and flip-flop them (a zero-delay
    // repaint loop). The diff viewer in the same window took the same
    // precaution for the same reason (ADR-0014).
    let scrolled = ScrollArea::vertical().id_salt("log_commit_list").show_rows(
        ui,
        crate::theme::FILE_ROW_HEIGHT,
        display.len(),
        |ui, visible| {
            // Issued inside the closure because that is where egui consumes a
            // scroll target: one set before the area begins is stashed for an
            // outer area instead. `show_rows` also hands over the range it
            // built, which is what the target's offset is measured from.
            if let Some(id) = scroll_to.as_deref()
                && let Some(index) = commit_index(display.rows(), id)
            {
                let pitch = crate::theme::FILE_ROW_HEIGHT + ui.spacing().item_spacing.y;
                let offset = row_offset_from_built(index, visible.clone(), pitch);
                // The row's band and the viewport, both read off THIS Ui, so the
                // two are in one coordinate space and cannot need a mapping
                // between them: the row sits at the built window's top plus the
                // offset, and the viewport is the clip rect.
                let row_centre = ui.max_rect().top() + offset + crate::theme::FILE_ROW_HEIGHT / 2.0;
                let viewport = ui.clip_rect();
                let view_top = viewport.top() - ui.max_rect().top();
                if !row_is_in_view(
                    offset,
                    crate::theme::FILE_ROW_HEIGHT,
                    view_top,
                    viewport.height(),
                ) {
                    let viewport_centre = viewport.top() + viewport.height() / 2.0;
                    ui.scroll_with_delta(Vec2::new(
                        0.0,
                        centre_scroll_delta(row_centre, viewport_centre),
                    ));
                }
            }
            // Only the rows inside the viewport are built: the scroll area
            // is given the whole window's row count and the row pitch, and
            // hands back the range it can show. A commit row is a fixed
            // height that paints its cells directly, so uniform height is
            // already true and the list can page over its rows.
            for index in visible {
                // A row count that moved under us mid-frame is a miss, not
                // a panic: the window is derived once per frame, but the
                // range came from a count the pane read a moment earlier.
                let Some(c) = display.row(index) else {
                    continue;
                };
                match commit_row(ui, state, c, colors, date_mode, multi_root) {
                    RowIntent::None => {}
                    RowIntent::Select => {
                        if clicked.is_none() {
                            clicked = Some((c.root.clone(), c.id.clone()));
                        }
                    }
                    RowIntent::SelectAndOpenMenu => {
                        if clicked.is_none() {
                            clicked = Some((c.root.clone(), c.id.clone()));
                            opened = Some((c.root.clone(), c.id.clone()));
                        }
                    }
                }
            }
        },
    );

    // "No commits match" is the one variable-height thing in the list, so it is
    // drawn outside the paged rows: an empty result must not have to claim a row
    // slot it does not have, and the message stays put where the pane has always
    // shown it instead of scrolling away with a list that has nothing in it.
    if display.is_empty() {
        ui.label(
            RichText::new("No commits match.")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
    }

    // Auto-load-more (P6): the trigger is a settled bottom on an overflowing
    // list, not a scroll gesture — so the wheel, the scrollbar and the
    // keyboard all arrive here, and a list that fits its viewport never fires.
    // `state.offset.y` counts down the list, so it is the scrolled distance.
    if may_have_more
        && settled_at_bottom(
            scrolled.state.offset.y,
            viewport_height,
            scrolled.content_size.y,
            crate::theme::FILE_ROW_HEIGHT,
        )
    {
        load_more = true;
    }

    // Pagination status line (issue 17): what is shown + the Load more
    // affordance while the fetched window may not cover the whole history. The
    // count is the held display window's own length (log-view-scaling 04), so in
    // a scope it reports the SCOPE's loaded window rather than the number of
    // rows the list drew — the list builds only its viewport's rows, and the
    // window is the thing the gate and the pager both work from, so the three
    // describe the same listing.
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if may_have_more && ui.small_button("Load more").clicked() {
                load_more = true;
            }
            ui.label(
                RichText::new(format!("{} shown", display.len()))
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        });
    });

    if load_more {
        for window in &windows {
            window.load_more(state);
        }
    }
    if let Some((root, id)) = clicked {
        // A press means this row's repository as well as this row's commit.
        select_commit_row(state, root, id);
        // A left click selects and closes any open menu: the press always does
        // what it looks like it does.
        state.ui.log_commit_menu = None;
    }
    if let Some(target) = opened {
        state.ui.log_commit_menu = Some(target);
    }

    commit_context_menu(ui, state);
}

/// What a press on a commit row means for the app: this row's repository, this
/// row's commit, and no changed file carried over from the commit before it.
///
/// The same shape as [`apply_log_tree_event`]'s row activation — set the
/// repository, clear the changed file — with one deliberate difference, and the
/// difference is the whole point of a commit row: a *branch* row is a scope
/// control, so it sets `log_ref_scope` and clears `selected_commit` because no
/// commit is what the developer asked to look at. A commit row names a commit,
/// so it sets `selected_commit` and leaves the scope alone — the developer asked
/// to see this commit, not to browse a different history. Both end with the pane
/// describing what was pressed.
///
/// `selected_root` is what every dialog the menu opens reads, and the listing is
/// a union across roots, so this is also what makes a dialog act on the
/// repository the row came from rather than on whichever one happened to be
/// selected when the row was painted.
fn select_commit_row(state: &mut AppState, root: RootId, id: String) {
    state.selected_root = Some(root);
    state.ui.selected_commit = Some(id);
    state.ui.log_selected_file = None;
}

/// What a press on a commit row asks for. Right-click selects **and** opens the
/// menu, so the menu and the details pane cannot describe different commits
/// (ADR-0024); there is deliberately no "menu without selection" intent. Both
/// intents are applied by [`select_commit_row`], so neither can end up naming a
/// different repository from the other.
enum RowIntent {
    None,
    Select,
    SelectAndOpenMenu,
}

/// The gates are evaluated per repository root, so a dirty root does not
/// disable an action on a clean one. Everything here is state the log already
/// holds; none of it costs a git call.
fn commit_facts<'a>(state: &'a AppState, root: &'a RootId, cid: &str) -> CommitFacts<'a> {
    let snapshot = state.multi.by_id(root);
    CommitFacts {
        dirty: snapshot.is_some_and(|r| !r.status.changes.is_empty()),
        protected_branch: snapshot
            .and_then(|r| r.current_branch.clone())
            .is_some_and(|b| sync_service::is_protected(&state.settings, &b)),
        // A plan is built from the commit to the current branch's tip, so a
        // commit outside this root's listing cannot be rewritten from it. A root
        // with no named branch checked out has no such history either, and the
        // item states that rather than looking broken.
        on_current_branch: snapshot.is_some_and(|r| r.current_branch.is_some())
            && state
                .caches
                .log(root)
                .is_some_and(|cs| cs.iter().any(|c| c.id == cid)),
        multi_root: state.multi.roots.len() > 1,
        repo_name: root_display_name(root),
    }
}

/// The repository's own name — its directory's, which is what the branches
/// pane's ROOTS filter and the shell already call it. Borrowed off the root id
/// so the menu can name the repository a commit belongs to without copying a
/// `String` per frame per row.
fn root_display_name(root: &RootId) -> &str {
    let path = root.as_path();
    path.file_name()
        .and_then(|name| name.to_str())
        .or_else(|| path.to_str())
        .unwrap_or_default()
}

/// The Git Log's commit context menu: the one place a commit action lives. The
/// host owns the lifecycle; this surface owns which row's menu is open and what
/// each pick means.
fn commit_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some((root_id, cid)) = state.ui.log_commit_menu.clone() else {
        return;
    };
    let target = widgets::menu_host::target_of(&root_id, &cid);
    let ctx = ui.ctx().clone();
    let mut dismiss = false;
    let picked = widgets::menu_host::host_menu(
        ui,
        widgets::menu_host::MenuId::new("log_commit", &target),
        true,
        &mut dismiss,
        |ui| {
            find_commit(state, &root_id, &cid)
                .cloned()
                .and_then(|commit| commit_menu(ui, &commit, &commit_facts(state, &root_id, &cid)))
        },
    );
    if dismiss {
        state.ui.log_commit_menu = None;
    }
    if let Some(action) = picked.flatten() {
        apply_commit_action(state, &ctx, &root_id, &cid, action);
    }
}

/// One dispatcher for all eleven items — the ruling ADR-0023 made for branches
/// applied to commits. It closes the menu first, so no action can be taken
/// against a menu the developer can no longer see.
fn apply_commit_action(
    state: &mut AppState,
    ctx: &egui::Context,
    root: &RootId,
    cid: &str,
    action: CommitMenuAction,
) {
    state.ui.log_commit_menu = None;
    // Every verb acts on the commit this menu was opened on and none of them
    // changes the selection, so the list is asked to show that commit again
    // (issue 08): the menu can outlive a scroll — it is app state, not a frame's
    // local — and without this the verb confirms "this commit" about a row the
    // list has since scrolled away from.
    state.ui.log_scroll_to = Some(cid.to_owned());
    match action {
        // Clipboard writes are not git work and never were an `Operation`; this
        // is the app's single clipboard call site and its toast convention, with
        // the text naming WHICH thing was copied so the two are distinguishable.
        CommitMenuAction::CopyHash => {
            ctx.copy_text(cid.to_owned());
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success(format!(
                "Copied {}",
                widgets::short_commit_ref(cid)
            )));
        }
        CommitMenuAction::CopyMessage => {
            let message = find_commit(state, root, cid)
                .map(|c| c.message.clone())
                .unwrap_or_default();
            ctx.copy_text(message);
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success("Copied commit message"));
        }
        CommitMenuAction::CherryPickTo => {
            state.ui.dlg.cherry_pick_commit = Some(cid.to_owned());
            state.ui.dialog = Some(Dialog::CherryPickTarget);
        }
        CommitMenuAction::CherryPickAcross => {
            state.open_cherry_across();
            state.ui.dialog = Some(Dialog::CherryPickAcross);
        }
        CommitMenuAction::RevertCommit => {
            state.ui.confirm = Some(PendingConfirm::RevertCommit {
                commit: cid.to_owned(),
            });
        }
        CommitMenuAction::NewBranch => {
            // The base is the commit that was right-clicked, not the tip — the
            // whole point of right-clicking a row in the middle of history — and
            // creating the branch does not move the checkout, which is why this
            // arm passes `false` where every other open path passes `true`.
            state.open_new_branch(NewBranchBase::Commit(cid.to_owned()), false, None);
        }
        CommitMenuAction::Checkout => {
            // The menu's gate reads the cached worktree; this is the last look
            // before git is touched. A tree that turned dirty in between goes to
            // the SAME bring-along / set-aside / cancel confirmation a branch
            // checkout raises, so the two checkouts behave identically.
            let dirty = state
                .multi
                .by_id(root)
                .is_some_and(|r| !r.status.changes.is_empty());
            if dirty {
                state.ui.confirm = Some(PendingConfirm::CheckoutDirty {
                    root: root.clone(),
                    target: cid.to_owned(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    detach: true,
                });
            } else {
                state.checkout_detached_op(root, cid);
            }
        }
        CommitMenuAction::NewTag => {
            // One open path, shared with the VCS palette's Tag: the dialog is
            // seeded with the commit that was right-clicked, and everything the
            // last visit worked out for itself is cleared.
            state.open_tag_dialog(Some(cid));
        }
        CommitMenuAction::DropCommit => {
            // A rewrite is never one click: the preflight is built from the same
            // repository the menu's own root names, and carries the plan the
            // confirm dispatches.
            state.open_rewrite_preflight(root, cid, turbogit_app::state::HistoryVerb::Drop);
        }
        CommitMenuAction::CreatePatch => {
            // A file write, not git work: no Operation, no activity entry.
            state.create_patch(root, cid);
        }
        CommitMenuAction::RewordCommit => {
            // Two steps, deliberately: the message is written here, and the cost
            // of writing it is shown by the preflight this editor's confirm
            // opens. The editor is seeded from the commit's own message, so a
            // correction starts from what the commit actually says.
            state.open_reword_editor(root, cid);
        }
    }
}

fn body_font() -> FontId {
    crate::theme::chrome_font(crate::theme::TYPE_BODY)
}

fn mono_font() -> FontId {
    FontId::new(MONO_TEXT, FontFamily::Monospace)
}

/// The one ink rule for every row this window paints: a row's identity text
/// reads at primary and does not change with the row's state.
///
/// Replaces the log's own `row_ink(active)`, which inverted ink with selection
/// — an unselected row's name at `INK_2` and the same name at `INK` the moment
/// it was chosen. That is the inversion the shared row grammar exists to
/// remove: a selected row keeps the ink it had, and the selection is named by
/// the band and the rail instead. Named as a function rather than spelled at
/// three sites so "the log's row ink does not depend on selection" is one
/// decision, and so the one construction-site rule that follows from it — a
/// row's own ink is never the muted step, which is illegal on every band a row
/// can be in — has a single place to be read from.
///
/// `pub(crate)` for the blame view, which replaces the commit table in the same
/// slot: its rows wear these two inks or they wear a third answer to a question
/// this table has already settled.
pub(crate) fn row_name_ink() -> Color32 {
    Palette::INK
}

/// The ink for a row's **secondary** text — its author, its date, the directory
/// a file lives in.
///
/// One step down from [`row_name_ink`], and specifically **not** the muted step:
/// a row's own secondary text sits on whichever band the row is in, and the
/// muted step is not legal on a raised or selected band (3.81:1 on the hover
/// fill, 3.52:1 on a chosen file row's band). Stepping up costs the row one
/// level of hierarchy and buys the text the same AA it has at rest.
///
/// `pub(crate)` for the same reason as [`row_name_ink`]: the blame view's rows
/// are in this table's slot, and its age cell is this table's date cell.
pub(crate) fn row_meta_ink() -> Color32 {
    Palette::INK_2
}

fn allocate_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
        Sense::click(),
    )
}

/// One two-line changed-file row (redesign issue 04).
fn allocate_file_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(Vec2::new(width, LOG_FILE_ROW_HEIGHT), Sense::click())
}

/// Lay `text` out no wider than `max_width`, dropping characters from the tail
/// and marking the cut with an ellipsis — a name is never trimmed in the
/// middle (redesign issue 04).
fn elide(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<Galley> {
    let full = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    if text.is_empty() || full.size().x <= max_width {
        return full;
    }
    // Start from a proportional guess and step down: this paints for every
    // row on every frame, so one layout per character is not affordable.
    let chars = text.chars().count();
    let guess = (chars as f32 * (max_width / full.size().x)) as usize;
    let mut keep = guess.saturating_sub(1).min(chars).max(1);
    loop {
        let cut: String = text.chars().take(keep).collect();
        let galley = painter.layout_no_wrap(format!("{cut}…"), font.clone(), color);
        if galley.size().x <= max_width || keep <= 1 {
            return galley;
        }
        keep -= 1;
    }
}

/// Every row in this window reaches the shared row shell through here: the
/// commit table's rows, the changed-files pane's rows, and the branches pane's
/// ROOTS filter rows.
///
/// Two decisions, both the shared ones, and neither of them this module's:
/// the **band** is `components::row_fill` (the translucent focus band while the
/// row is chosen, the raised-on-card hover fill otherwise), and the **rail** is
/// the one `components::paint_rail` at the row's leading edge, applied by the
/// shell's [`components::RowShell::Railed`] variant.
///
/// The focus band is the log's band by decision, not by omission: it composites
/// over the panel to the same opaque value as the selected-list-row fill
/// (`selection_bg()` over the app background is `#233455`, and `ROW_SELECTED` is
/// `#243456`) while letting the lane colours and the per-root swatch read
/// through, which is what a graph needs and a tree does not. The rail is
/// additive on top of it, so the chosen row is named the way every other chosen
/// row in the app is named.
///
/// `pub(crate)` because the blame view replaces the commit table in the same slot
/// and must paint its rows the same way: two row vocabularies in one region is a
/// visual reset at the moment of switching, which is the one thing this row
/// grammar exists to prevent.
pub(crate) fn paint_log_row(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    components::row_shell(
        ui,
        rect,
        if selected {
            components::RowState::FocusSelected
        } else {
            components::RowState::from_flags(false, hovered)
        },
        components::RowShell::Railed,
    );
}

/// Whether this listing is a multi-root union — the one predicate that decides
/// whether the ROOTS column and its gutter exist at all.
///
/// A scoped view is single-root by definition (a ref scope names its own root, a
/// path scope reads the selected one) and so is a listing narrowed by the roots
/// filter, so none of them has a per-root membership to report. Named rather
/// than inlined because the header row, every data row **and the blame view**
/// have to agree on it: a table whose cells sat at the gutter in one place and
/// beside it in another is a column that is narrow in the header and wide in the
/// rows, the exact bug the shared column table exists to prevent.
pub(crate) fn shows_root_gutter(state: &AppState) -> bool {
    state.multi.roots.len() > 1
        && state.ui.log_root_filter.is_none()
        && state.ui.log_path_scope.is_none()
        && state.ui.log_ref_scope.is_none()
}

/// The commit table's column-header row, measured from the same row rect the
/// cells below it measure from.
///
/// Before the shared row existed this was four hand-laid galleys at
/// `Palette::INK_3` with no underline, duplicating the commit table's offsets a
/// second time. It is now [`widgets::column_header`] reading
/// [`COMMIT_COLUMNS`], so the labels cannot drift from the cells — and the
/// labels' ink is pinned in `tests/widget_library.rs` to the muted step, not
/// the dim one, because 9px is normal-size text.
///
/// **The ROOTS label is in this row** because it is an entry in
/// [`COMMIT_COLUMNS`], not because something painted one above the gutter: a
/// free-floating label would carry none of this chrome's ink and would sit
/// outside the header row's rule. `tests/git_log.rs` pins that it shares this
/// row's baseline and its ink with the other four.
fn header_cells(ui: &mut Ui, multi_root: bool) {
    let available = ui.available_rect_before_wrap();
    let left = table_content_left(available, multi_root);
    widgets::column_header(
        ui,
        Rect::from_min_max(
            Pos2::new(left, available.top()),
            Pos2::new(available.right(), available.bottom()),
        ),
        &COMMIT_COLUMNS,
    );
}

/// Where a commit-table row's cells start: the row's left edge, plus the
/// leading gutter in multi-root views.
///
/// The header row and every data row go through here — and so does the blame
/// view, which occupies the same slot — so a column can never sit beside the
/// swatch in the header and over it in the rows. The gutter is the ROOTS column
/// in full (the rail *and* the swatch), so it is the same number whatever the
/// row's selection is: a row's columns are at the same x selected or not, which
/// is the same "paint, not layout" rule the rail itself obeys.
pub(crate) fn table_content_left(row: Rect, multi_root: bool) -> f32 {
    row.left() + if multi_root { GUTTER } else { 0.0 }
}

/// Paint the ROOTS column's cell for one commit row: a full-height swatch in
/// the root's own colour, filling the gutter immediately inside the rail.
///
/// **This is the fourth site that asks [`root_color`]** — after the ROOTS
/// filter's dot, the legend swatch and the graph lane node — and the point of
/// the ROOTS column is that all four answer for the same root. The colour comes
/// from the one table by construction: this function takes an *index* and
/// nothing else, so there is no argument through which a literal could arrive,
/// and `tests/git_log.rs` compares its painted rect against the lane node's and
/// the legend's for the same index.
///
/// The geometry is the rail-versus-stripe decision, not a new one: the swatch is
/// `STRIPE_WIDTH` wide at `row.left() + GUTTER - STRIPE_WIDTH`, so it is flush
/// against the rail's trailing edge and the cells begin at
/// [`table_content_left`]. The rail leads at `[row.left(), row.left() + 2)` and
/// never moves — not for a selected row, not for a root with a swatch, not for
/// the column's header. See [`GUTTER`].
fn paint_root_swatch(ui: &Ui, row: Rect, root_index: usize) {
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(row.left() + GUTTER - STRIPE_WIDTH, row.top()),
            Vec2::new(STRIPE_WIDTH, row.height()),
        ),
        CornerRadius::ZERO,
        root_color(root_index),
    );
}

/// One commit-table row: rail | ROOTS swatch | node | hash | author |
/// message(+refs) | date.
/// Renders against a shared [`AppState`] (the displayed union borrows the
/// caches) and reports what a press on it asked for; the caller applies the
/// selection after rendering (plan §1.3 defer pattern).
///
/// The band and the rail are [`paint_log_row`]'s, the cells are measured from
/// [`table_content_left`] and the columns the header reads, and every cell's ink
/// is the shared ramp's — none of them a function of the row's state.
fn commit_row(
    ui: &mut Ui,
    state: &AppState,
    c: &Commit,
    colors: &std::collections::HashMap<String, usize>,
    date_mode: DateFormat,
    multi_root: bool,
) -> RowIntent {
    let selected = state.ui.selected_commit.as_deref() == Some(c.id.as_str());
    let (rect, response) = allocate_row(ui);

    // The shared row shell, in its railed variant, over the rect this row has
    // already allocated: the focus band while the row is chosen, the raised
    // hover fill under the pointer, and the one accent rail at the leading edge
    // of a chosen row. The rail is paint over the band, not padding beside it,
    // so every column below is at the same x selected or not.
    paint_log_row(ui, rect, selected, response.hovered());

    // The ROOTS column's cell, immediately inside the rail — the two share the
    // leading gutter, and neither displaces the other. See [`GUTTER`].
    if multi_root {
        paint_root_swatch(ui, rect, root_index_of(state, &c.root));
    }
    // …and the origin the header row measured from, so a label sits over its
    // cell in a multi-root view exactly as it does in a single-root one.
    let content_left = table_content_left(rect, multi_root);

    // Graph cell: colored lane node (ring for merges).
    let lane = colors
        .get(&c.id)
        .map(|i| root_color(*i))
        .unwrap_or(Color32::GRAY);
    let center = Pos2::new(content_left + COL_GRAPH, rect.center().y);
    if c.parents.len() > 1 {
        ui.painter()
            .circle_stroke(center, 4.0, egui::Stroke::new(1.5, lane));
    } else {
        ui.painter().circle_filled(center, 4.0, lane);
    }

    // Hash | Author cells.
    //
    // Every ink here is a shared ramp step and **none of them depends on the
    // row's state**: a selected row's text is the same colour as the same text
    // on an unselected row, which is the whole content of "selection does not
    // invert ink". Two of the four are steps *up* from what they were, because
    // the band a chosen row takes is the selected value and the muted step is
    // not legal on it (3.76:1 measured) and neither is the accent (2.89:1):
    //
    // | cell | ink | on the resting panel | on the chosen row's band |
    // |---|---|---|---|
    // | hash | [`Palette::LINK`] | 6.39:1 | 4.79:1 |
    // | author | [`Palette::INK_2`] | 7.86:1 | 5.89:1 |
    // | subject | [`Palette::INK`] | 12.59:1 | 9.43:1 |
    // | date | [`Palette::INK_2`] | 7.86:1 | 5.89:1 |
    //
    // The hash is the one cell that moved off the accent: a hash is information
    // the user reads to name a commit, it answers no press, and the accent
    // measures 2.89:1 on the band a chosen row takes. `LINK` is the token the
    // details pane's hash chip already wears for exactly that reason.
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash_galley =
        painter.layout_no_wrap(widgets::short_commit_ref(&c.id), mono_font(), Palette::LINK);
    painter.galley(
        Pos2::new(content_left + COL_HASH, cy - hash_galley.size().y / 2.0),
        hash_galley,
        Palette::LINK,
    );
    let author_galley =
        painter.layout_no_wrap(truncate(&c.author.name, 10), body_font(), row_meta_ink());
    painter.galley(
        Pos2::new(content_left + COL_AUTHOR, cy - author_galley.size().y / 2.0),
        author_galley,
        row_meta_ink(),
    );

    // Date cell: the trailing column, right-aligned to the row's edge, at the
    // secondary step rather than the muted one for the reason in the table
    // above — the muted step is 3.76:1 on the band a chosen row takes.
    let date_galley =
        painter.layout_no_wrap(fmt_date(c.time, date_mode), body_font(), row_meta_ink());
    let date_x = rect.right() - DATE_RIGHT_PAD - date_galley.size().x;
    painter.galley(
        Pos2::new(date_x, cy - date_galley.size().y / 2.0),
        date_galley,
        row_meta_ink(),
    );

    // Message cell with one collapsed label pill — the wide column, sitting
    // left of the date. Everything is painted directly (galleys + the pill,
    // no child widgets) so the row itself stays the only interactive surface;
    // a child `ui.label` here would sit on top of the row in hit-testing and
    // swallow its clicks.
    let labels = state.caches.refs_for(&c.root, &c.id);
    let message_left = content_left + COL_MESSAGE;
    let budget = (date_x - 8.0 - if labels.is_empty() { 0.0 } else { PILL_RESERVE } - message_left)
        .max(24.0);
    let subject = c.message.lines().next().unwrap_or("");
    // Fitted to the column behind a width oracle, so the row spends a bounded
    // number of layouts on the fit rather than one per dropped character.
    let subject_fit = fit_to_budget(&truncate(subject, 44), budget, &|text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), body_font(), row_name_ink())
            .size()
            .x
    });
    let subject_galley = painter.layout_no_wrap(subject_fit, body_font(), row_name_ink());
    let subject_w = subject_galley.size().x;
    painter.galley(
        Pos2::new(message_left, cy - subject_galley.size().y / 2.0),
        subject_galley,
        row_name_ink(),
    );
    let mx = message_left + subject_w + 6.0;

    // Refs collapse into one ".tg-label" pill, shown only when the commit
    // actually carries labels (branch / remote / tag); the individual names
    // are revealed in a hover tooltip so decorated rows stay scannable.
    if !labels.is_empty() {
        let pill = paint_label_pill(&painter, mx, cy);
        if ui.rect_contains_pointer(pill) {
            // The popup anchors in global space; the row lives inside the
            // graph's scroll area, so map the pill rect to screen first.
            let screen_pill = ui
                .ctx()
                .layer_transform_to_global(ui.layer_id())
                .map(|t| t * pill)
                .unwrap_or(pill);
            Popup::new(
                ui.id().with(("log_label_tooltip", &c.id)),
                ui.ctx().clone(),
                screen_pill,
                ui.layer_id(),
            )
            .kind(PopupKind::Tooltip)
            .gap(4.0)
            .interactable(false)
            .show(|ui| {
                for r in labels {
                    ui.label(
                        RichText::new(&r.name)
                            .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                            .color(ref_kind(r.kind).accent()),
                    );
                }
            });
        }
    }

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("{} {}", widgets::short_commit_ref(&c.id), subject),
        )
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        RowIntent::Select
    } else if response.secondary_clicked() {
        // The anchor is the pointer position at the right-click, stashed in egui
        // memory for the host to read back at the end of this pane.
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        widgets::menu_host::note_anchor(
            ui,
            widgets::menu_host::MenuId::new(
                "log_commit",
                &widgets::menu_host::target_of(&c.root, &c.id),
            ),
            pos,
        );
        RowIntent::SelectAndOpenMenu
    } else {
        RowIntent::None
    }
}

/// Paint one collapsed ref marker (the tag icon on a neutral badge) and return
/// its rect. The ref names live in the hover tooltip (see `commit_row`).
/// Painter-only: registers no widget, because a child widget here would sit on
/// top of the row and swallow its clicks.
///
/// **This marker is the neutral *badge*, not a ref chip** — the answer
/// conformance issues 13/14 were asked to settle, and the reason is what the
/// marker carries rather than how it is drawn. A ref chip is a *ref name* on
/// [`widgets::REF_CHIP_COLORS`]'s raised-on-card fill at the compact chip radius;
/// this marker carries no ref name at all (the names are in the tooltip, and the
/// rows stay scannable because of it), so painting it at the chip radius would
/// spend the one role that means "a ref name" on a thing that names no ref, and
/// the compact radius would then mean two things in the app. It is the badge
/// family instead: [`widgets::BadgeKind::Neutral`]'s pair and the **pill slot**
/// ([`widgets::CHIP_GEOMETRY`]), which is the shape the module docs keep
/// distinct from the chips' compact slot on purpose.
///
/// So the geometry and the colours are read from those two shared values rather
/// than re-spelled, and no second chip constructor is created: the chip set stays
/// closed at three, `widgets::ref_chip` stays the only function that produces a
/// ref chip, and the log's marker is pinned to the badge pair by
/// `tests/git_log.rs` — which is the seam a painter-only helper is only provable
/// from.
///
/// The icon's centring is the icon rectangle arithmetic, which is a different
/// calculation from the two-axis text centring in [`widgets::paint_centered_text`]
/// and the reason this is not on [`widgets::ChipGeometry::paint`]: there is no
/// laid-out galley here to place.
fn paint_label_pill(painter: &egui::Painter, x: f32, cy: f32) -> Rect {
    const ICON_SIZE: f32 = 12.0;
    let colors = BadgeKind::Neutral.colors();
    let geometry = widgets::CHIP_GEOMETRY;
    let rect = Rect::from_min_size(
        Pos2::new(x, cy - geometry.height / 2.0),
        Vec2::new(ICON_SIZE + geometry.pad_x * 2.0, geometry.height),
    );
    painter.rect_filled(rect, widgets::chip_radius(), colors.bg);
    icons::paint_icon(
        painter,
        Pos2::new(
            rect.center().x - ICON_SIZE / 2.0,
            rect.center().y - ICON_SIZE / 2.0,
        ),
        ICON_SIZE,
        Icon::TAG,
        colors.fg,
    );
    rect
}

// --- Pane 3: changed files -----------------------------------------------------------

/// Deferred interaction from one changed-file row (plan §1.3): rows render
/// against a shared [`AppState`] — the selection and the cached file list are
/// borrowed — so mutations land after the pane finishes rendering.
enum FileAction {
    None,
    /// Row clicked: open its diff; the caller resolves root / commit / parent.
    OpenDiff(PathBuf),
    /// Footer link / menu item: blame the file at the selected commit.
    OpenBlame(PathBuf),
    /// Menu item: scope the whole workspace to this file's history.
    ScopeHistory(PathBuf),
    /// Row right-clicked: open the shared menu on this row.
    OpenMenu(PathBuf),
}

fn files_pane(ui: &mut Ui, state: &mut AppState) {
    // Split-borrow the selection up front (plan §1.3): no owned RootId /
    // CommitId clones per frame, and the cached file list is iterated in
    // place instead of copied into a fresh Vec.
    let selection = state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref());
    let files = selection
        .and_then(|(root, cid)| state.caches.files_for(root, cid))
        .unwrap_or(&[]);
    // Line counts arrive one worker round-trip behind the file list (redesign
    // issue 02); until they land, the rows simply carry no numbers.
    let stats = selection
        .and_then(|(root, cid)| state.caches.file_stats_for(root, cid))
        .unwrap_or(&[]);

    // The pane's own header (R7): the title, and the file count as the shared
    // count chip beside it rather than a fourth badge of its own. Without a
    // selection there is no count to state, which is the `None` case the header
    // was built for.
    let count = selection.map(|_| files.len().to_string());
    widgets::pane_header(ui, "CHANGED FILES", count.as_deref(), |_ui| {});

    let Some((root_id, cid)) = selection else {
        ui.label(
            RichText::new("Select a commit to see its changed files.")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let parent = find_commit(state, root_id, cid).and_then(|c| c.parents.first().cloned());

    // Filter row (redesign issue 04): the pane's own narrowing of a long
    // change list, matched case-insensitively against the path.
    widgets::search_input(ui, "Filter changed files", &mut state.ui.log_file_filter);
    let filter = state.ui.log_file_filter.trim().to_lowercase();
    let row_shown = |change: &turbogit_domain::model::Change| {
        filter.is_empty()
            || change
                .path
                .to_string_lossy()
                .to_lowercase()
                .contains(&filter)
    };
    let shown = files.iter().filter(|c| row_shown(c)).count();

    let mut action = FileAction::None;
    ScrollArea::vertical().show(ui, |ui| {
        for ch in files {
            if !row_shown(ch) {
                continue;
            }
            let row_action = file_row(ui, state, root_id, ch, file_stat(stats, &ch.path));
            // The first row that reports an intent owns the frame: a later
            // non-clicked row must not clear it (only one row can be clicked
            // per frame, but the loop keeps painting after it).
            if matches!(action, FileAction::None) {
                action = row_action;
            }
        }
        if files.is_empty() {
            ui.label(
                RichText::new("No changed files.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        } else if shown == 0 {
            // A filter that hides every row is its own state — never the
            // "this commit changed nothing" message (issue 02).
            ui.label(
                RichText::new("No file matches the filter.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
    });

    // Footer links (screen 09): Open diff · Blame · Full path history,
    // acting on the selected changed file. Rendered only with a selection —
    // without one none of the verbs has a subject.
    if let Some(selected) = state.ui.log_selected_file.clone() {
        ui.separator();
        ui.horizontal(|ui| {
            if ui.link("Open diff").clicked() {
                action = FileAction::OpenDiff(selected.clone());
            }
            ui.label(RichText::new("·").color(Palette::INK_3));
            if ui.link("Blame").clicked() {
                action = FileAction::OpenBlame(selected.clone());
            }
            ui.label(RichText::new("·").color(Palette::INK_3));
            if ui.link("Full path history").clicked() {
                action = FileAction::ScopeHistory(selected);
            }
        });
    }

    match action {
        FileAction::None => {}
        FileAction::OpenMenu(path) => {
            state.ui.log_file_menu = Some((root_id.clone(), cid.to_owned(), path));
        }
        FileAction::OpenDiff(path) => {
            state.ui.log_selected_file = Some(path.clone());
            state.ui.diff = Some(DiffTarget {
                root: root_id.clone(),
                left: parent,
                right: Some(cid.to_owned()),
                path: Some(path),
            });
        }
        FileAction::OpenBlame(path) => {
            state.ui.blame = Some(BlameTarget {
                root: root_id.clone(),
                path,
                rev: cid.to_owned(),
            });
        }
        FileAction::ScopeHistory(path) => {
            state.ui.log_path_scope = Some(path);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
    }

    file_row_context_menu(ui, state);
}

/// The changed-file row's context menu, on the same host the commit menu and the
/// branches list use. It used to be the window's third menu dialect — stock egui
/// with raw buttons, no shared surface, and no way to show a blocked item with a
/// reason. Both verbs do exactly what they did before; the surface is shared now,
/// so the whole window answers "why can't I do this" the same way.
fn file_row_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some((root_id, cid, path)) = state.ui.log_file_menu.clone() else {
        return;
    };
    let target = widgets::menu_host::target_of(&root_id, &path.to_string_lossy());
    let mut dismiss = false;
    let picked = widgets::menu_host::host_menu(
        ui,
        widgets::menu_host::MenuId::new("log_file_row", &target),
        true,
        &mut dismiss,
        |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_min_width(240.0);
            // Scoping to a file whose history is already on screen is a no-op, so
            // the item stays rendered and says why rather than vanishing.
            let already_scoped = state.ui.log_path_scope.as_deref() == Some(path.as_path());
            let mut picked = None;
            if menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::FILE_CODE,
                    label: "Show blame",
                    data: None,
                    shortcut: None,
                    enabled: true,
                    disabled_reason: None,
                    kind: MenuItemKind::Default,
                },
            )
            .clicked()
            {
                picked = Some(FileAction::OpenBlame(path.clone()));
            }
            let history = menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::CLOCK,
                    label: "Show history for file…",
                    data: None,
                    shortcut: None,
                    enabled: !already_scoped,
                    disabled_reason: already_scoped
                        .then_some("already showing this file's history"),
                    kind: MenuItemKind::Default,
                },
            );
            if history.clicked() {
                picked = Some(FileAction::ScopeHistory(path.clone()));
            }
            picked
        },
    );
    if dismiss {
        state.ui.log_file_menu = None;
    }
    if let Some(action) = picked.flatten() {
        // The menu closes before its verb runs, exactly as the commit menu's does.
        state.ui.log_file_menu = None;
        apply_file_action(state, &root_id, &cid, action);
    }
}

/// Run one changed-file row's verb against the root and commit the menu was
/// opened on — not whatever is selected now.
fn apply_file_action(state: &mut AppState, root: &RootId, cid: &CommitId, action: FileAction) {
    match action {
        FileAction::OpenBlame(path) => {
            state.ui.blame = Some(BlameTarget {
                root: root.clone(),
                path,
                rev: cid.to_owned(),
            });
        }
        FileAction::ScopeHistory(path) => {
            state.ui.log_path_scope = Some(path);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        _ => {}
    }
}

fn badge_kind(status: ChangeStatus) -> BadgeKind {
    match status {
        ChangeStatus::Added => BadgeKind::Added,
        ChangeStatus::Deleted => BadgeKind::Deleted,
        ChangeStatus::Modified => BadgeKind::Modified,
        _ => BadgeKind::Neutral,
    }
}

/// One changed-file row (redesign issue 04): status badge, file name on the
/// first line, its directory underneath, and the commit's `+N −M` for this
/// path at the right. Click opens the diff preview, context menu scopes the
/// workspace to the file's history (issue #19).
/// Renders against a shared [`AppState`] and reports its intent; the caller
/// applies it after rendering (plan §1.3 defer pattern).
fn file_row(
    ui: &mut Ui,
    state: &AppState,
    root: &RootId,
    change: &turbogit_domain::model::Change,
    stat: Option<(usize, usize)>,
) -> FileAction {
    let selected = state.ui.log_selected_file.as_ref() == Some(&change.path);
    let (rect, response) = allocate_file_row(ui);
    paint_log_row(ui, rect, selected, response.hovered());

    // Painter-only contents so the row owns the pointer (see commit_row).
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let mut mx = rect.left() + 4.0;

    // Status badge pill (mirrors `widgets::badge` metrics).
    let kind = badge_kind(change.status);
    let colors = kind.colors();
    let badge_galley = painter.layout_no_wrap(
        change.status.short().to_owned(),
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        colors.fg,
    );
    let badge_rect = Rect::from_min_size(
        Pos2::new(mx, cy - STATUS_PILL.height / 2.0),
        STATUS_PILL.size(&badge_galley),
    );
    STATUS_PILL.paint(&painter, badge_rect, badge_galley, colors.bg, colors.fg);
    mx = badge_rect.right() + 6.0;

    // Right-aligned churn (redesign issue 04): each number paints only when
    // git counted it, so a pure addition shows no `−0`, and a row measured
    // nothing at all — a binary file, or counts still in flight — shows
    // neither.
    let added = stat.filter(|(ins, _)| *ins > 0).map(|(ins, _)| {
        painter.layout_no_wrap(format!("+{ins}"), mono_font(), Palette::STATE_SUCCESS)
    });
    let removed = stat.filter(|(_, dels)| *dels > 0).map(|(_, dels)| {
        painter.layout_no_wrap(format!("−{dels}"), mono_font(), Palette::STATE_ERROR)
    });
    let mut text_right = rect.right() - 4.0;
    for galley in added.iter().chain(removed.iter()) {
        text_right -= galley.size().x + STAT_GAP;
    }

    // Line 1: the file name, tail-elided only when the pane cannot fit it. The
    // name is the row's identity text and reads at primary whether the row is
    // chosen or not — selection is the band's and the rail's to name.
    let ink = row_name_ink();
    let name = change
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| change.path.to_string_lossy().into_owned());
    let name_galley = elide(&painter, &name, body_font(), ink, text_right - mx);
    painter.galley(Pos2::new(mx, rect.top() + 7.0), name_galley, ink);

    // Line 2: the directory it lives in, de-emphasized. A file at the repo
    // root has none, so that row carries the name line alone. The directory is
    // secondary text on the row's own band, so it takes the secondary step
    // rather than the muted one (see `row_meta_ink`).
    if let Some(dir) = change.path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let dir_font = FontId::new(MICRO_TEXT, FontFamily::Proportional);
        let dir_text = dir.to_string_lossy().replace('\\', "/");
        let dir_ink = row_meta_ink();
        let dir_galley = elide(&painter, &dir_text, dir_font, dir_ink, text_right - mx);
        painter.galley(Pos2::new(mx, rect.top() + 22.0), dir_galley, dir_ink);
    }

    let mut churn_right = rect.right() - 4.0;
    if let Some(galley) = removed {
        churn_right -= galley.size().x;
        painter.galley(
            Pos2::new(churn_right, rect.top() + 7.0),
            galley,
            Palette::STATE_ERROR,
        );
        churn_right -= STAT_GAP;
    }
    if let Some(galley) = added {
        churn_right -= galley.size().x;
        painter.galley(
            Pos2::new(churn_right, rect.top() + 7.0),
            galley,
            Palette::STATE_SUCCESS,
        );
    }
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, true, change.path.display().to_string())
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        return FileAction::OpenDiff(change.path.clone());
    }
    // Right-click opens the shared menu (ticket 09): path-scoped file history
    // (issue #19) and blame (issue 18) are its two items.
    if response.secondary_clicked() {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        let target = widgets::menu_host::target_of(root, &change.path.to_string_lossy());
        widgets::menu_host::note_anchor(
            ui,
            widgets::menu_host::MenuId::new("log_file_row", &target),
            pos,
        );
        return FileAction::OpenMenu(change.path.clone());
    }
    FileAction::None
}

// --- Pane 4: commit details -----------------------------------------------------------

/// The one interaction left in the details pane (plan §1.3: it renders against
/// the borrowed cached commit, so the mutation lands after rendering).
///
/// It used to carry `CherryPick`, `CherryPickAcross`, `Revert`, `NewBranchHere`
/// and `CopyHash`. Those are the actions, and ADR-0024 moved every one of them
/// into the commit's context menu, leaving the pane to say things about a commit
/// and do nothing to one. `SelectParent` stays: a parent hash link is navigation
/// inside the metadata, and no menu item replaces it.
enum DetailAction {
    None,
    /// Jump to a parent commit via its hash link (issue 17).
    SelectParent(CommitId),
}

/// One line of the details pane's key/value list: a key, a value, and — for the
/// parents only — the one thing in this pane that is a link.
///
/// The three fields rather than a `String` is what keeps the parents *behaving*
/// like the pane's one navigation affordance instead of being flattened into
/// text that looks like every other value. A root commit carries no parents, so
/// the row still appears with `empty` set: the key says what would be there and
/// an em dash says there is nothing, which is a fact about the commit and not a
/// gap in the layout.
struct DetailRow {
    key: &'static str,
    value: String,
    parents: Vec<CommitId>,
    empty: bool,
}

impl DetailRow {
    /// A row whose value is text.
    fn text(key: &'static str, value: impl Into<String>) -> Self {
        Self {
            key,
            value: value.into(),
            parents: Vec::new(),
            empty: false,
        }
    }

    /// The parents row: a list of hashes, each one a link that jumps the
    /// selection to that commit.
    fn parents(key: &'static str, parents: &[CommitId]) -> Self {
        Self {
            key,
            value: String::new(),
            parents: parents.to_vec(),
            empty: parents.is_empty(),
        }
    }
}

/// The commit's own facts, as the details pane's key/value list: author, date,
/// committer, parents — in that order, above the message body.
///
/// **This is not [`CommitFacts`], and the name is deliberately not borrowed from
/// it.** `CommitFacts` is the gate set a commit *action* reads — is the
/// repository dirty, is its branch protected, is the commit reachable from the
/// current branch, is the project multi-root, what is the repository called.
/// Every one of those answers "may this verb run", and not one of them is a
/// fact *about the commit*; the two questions are unrelated, and reusing the
/// type would have meant the pane's contents changed shape every time an action
/// was added or removed. So the list is built here, over the [`Commit`] the log
/// already holds: the author, the committer, the timestamps, the parents and the
/// signature state are all fields of the commit the pane is about to describe.
/// **No new data plumbing, no git call, no cache read** — the previous
/// arrangement showed the same information in a different shape.
///
/// The signature suffix stays on the committer's value (issue 17): which
/// signature a commit carries is a fact about the committer's act, and there is
/// nowhere else in this pane for it to live.
fn commit_detail_rows(commit: &Commit) -> Vec<DetailRow> {
    let sig_suffix = match commit.signature {
        SignatureState::Unsigned => String::new(),
        SignatureState::Good => " · signed ✓".to_string(),
        SignatureState::Bad => " · signature BAD".to_string(),
        SignatureState::Unverified => " · signed (unverified)".to_string(),
    };
    vec![
        DetailRow::text(
            "Author",
            format!("{} <{}>", commit.author.name, commit.author.email),
        ),
        DetailRow::text("Date", fmt_time(commit.time)),
        DetailRow::text(
            "Committer",
            format!(
                "{} <{}>{}",
                commit.committer.name, commit.committer.email, sig_suffix
            ),
        ),
        DetailRow::parents("Parents", &commit.parents),
    ]
}

/// The pane's key/value list: one shared key column so every value starts at
/// the same x, keys on the **label** ink and values on the **value** ink.
///
/// The inks are the two roles this pane already had and are now stated as roles
/// rather than as a per-call decision: a key is a field label — the muted step,
/// the same one `micro_text` has always worn, legal on this content surface and
/// never the dim step, because a commit's own facts may not be rendered only at
/// 3.2:1 — and a value is the thing the label names, at primary.
///
/// It is a [`Grid`] with two columns and no raised container, which is what a
/// key/value list *is*: the values align because they share a key column, not
/// because each one was measured. The `SURFACE_3` panel the rows used to sit in
/// is the nested-box shape R2 removes, and with the list no longer three loose
/// rows in the corner of a box there is nothing for it to divide.
fn paint_detail_list(ui: &mut Ui, rows: &[DetailRow], action: &mut DetailAction) {
    Grid::new("log_commit_facts")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for row in rows {
                ui.label(micro_text(row.key));
                if row.empty {
                    ui.label(RichText::new("—").font(body_font()).color(Palette::INK_3));
                } else if row.parents.is_empty() {
                    ui.label(
                        RichText::new(&row.value)
                            .font(body_font())
                            .color(Palette::INK),
                    );
                } else {
                    ui.horizontal(|ui| {
                        for parent in &row.parents {
                            if ui
                                .link(
                                    RichText::new(widgets::short_commit_ref(parent))
                                        .font(mono_font())
                                        .color(Palette::INK),
                                )
                                .clicked()
                            {
                                *action = DetailAction::SelectParent(parent.clone());
                            }
                        }
                    });
                }
                ui.end_row();
            }
        });
}

fn details_pane(ui: &mut Ui, state: &mut AppState) {
    // Every interaction defers (plan §1.3): the pane borrows the cached
    // commit below, so clicks set `action` and mutations land at the end.
    let mut action = DetailAction::None;
    // The pane's own header (R7), over no raised parent (see [`pane_card`]).
    widgets::pane_header(ui, "COMMIT DETAILS", None, |_ui| {});
    // Compact vertical rhythm so the full message fits the pane.
    ui.style_mut().spacing.item_spacing.y = 3.0;

    // Split-borrow the selection (plan §1.3): the commit is looked up over
    // the cache slice and the file summary iterates the cached list in place.
    //
    // The empty state is a page-owned answer — nothing is selected, so there is
    // no commit to describe — and it is the one branch of this pane that paints
    // no facts at all. Reshaping the populated state is exactly the edit that
    // quietly deletes it, so `tests/git_log.rs` asserts it from a frame with
    // nothing selected.
    let Some((root_id, cid)) = state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref())
    else {
        ui.label(
            RichText::new("Select a commit…")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let Some(commit) = find_commit(state, root_id, cid) else {
        return;
    };

    // Subject first (redesign issue 05): the commit's identity, taking over
    // the top of the key-value wall it replaces.
    ui.label(
        RichText::new(commit.message.lines().next().unwrap_or_default())
            .font(widgets::bold_font_if_available(ui))
            .color(Palette::INK),
    );

    // Hash: still shown, no longer clickable. Copying it lives in the commit's
    // context menu, which states the same short reference on the item itself.
    ui.horizontal(|ui| {
        widgets::hash_chip(ui, &widgets::short_commit_ref(&commit.id), "");
    });

    // The key/value list, above the message body: a commit's facts are
    // scannable without opening anything (spec §user-story 30). It replaced an
    // avatar card that said the author's name and the day in prose, wrapped
    // around a meta grid that said the same two things again as labelled rows —
    // so the author and the date are now each stated once, in one shape, and the
    // keys line up where an eye can run down them.
    ui.add_space(4.0);
    paint_detail_list(ui, &commit_detail_rows(commit), &mut action);

    // Churn: the bar git measured, then the file count and the `+X −Y` totals.
    // Before the stats land only the file count is known, so that is all that
    // shows (issue 02's "no stat" rule, applied to the aggregate too).
    let files = state.caches.files_for(root_id, &commit.id).unwrap_or(&[]);
    let stats = state
        .caches
        .file_stats_for(root_id, &commit.id)
        .unwrap_or(&[]);
    let added: usize = stats.iter().filter_map(|f| f.insertions).sum();
    let removed: usize = stats.iter().filter_map(|f| f.deletions).sum();
    ui.add_space(6.0);
    widgets::churn_bar(ui, added, removed);
    ui.horizontal(|ui| {
        ui.label(micro_text(format!(
            "{} {} changed",
            files.len(),
            if files.len() == 1 { "file" } else { "files" }
        )));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if removed > 0 {
                ui.label(
                    RichText::new(format!("−{removed}"))
                        .font(mono_font())
                        .color(Palette::STATE_ERROR),
                );
            }
            if added > 0 {
                ui.label(
                    RichText::new(format!("+{added}"))
                        .font(mono_font())
                        .color(Palette::STATE_SUCCESS),
                );
            }
        });
    });

    // The message body: everything under the subject, which leads the pane
    // (issue 05) — repeating it here would spend the pane's last inches on a line
    // the user has already read. It is deliberately LAST: the key/value list
    // above it is a commit's facts, and a fact is worth more than prose when
    // both fit; when they do not, the list is the part that must not be pushed
    // off the bottom of the pane. This stays the pane's only scrolling region,
    // with the viewport capped to what is left so the `ScrollArea` cannot claim
    // the whole pane and clip the metadata above.
    let body: Vec<&str> = commit
        .message
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect();
    if !body.is_empty() {
        let message_height = ui.available_height().max(40.0);
        ScrollArea::vertical()
            .max_height(message_height)
            .show(ui, |ui| {
                for line in body {
                    ui.label(RichText::new(line).font(body_font()).color(Palette::INK));
                }
            });
    }

    // Deferred action application (plan §1.3): the borrow of the cached
    // commit ended above, so `state.ui` is free to mutate here.
    if let DetailAction::SelectParent(parent) = action {
        state.ui.selected_commit = Some(parent.clone());
        state.ui.log_selected_file = None;
        // Following history has to show where it landed (issue 08): the parent
        // is the next row DOWN, which the list may not have built, and the
        // scroll area is the only thing that can bring it into view.
        state.ui.log_scroll_to = Some(parent);
    }
}

/// Muted micro label — **the details pane's key ink**.
///
/// This helper is named as if it belonged to the column header, and it does
/// not: the commit table's header cells are [`widgets::column_header`], which
/// owns its own ink, and every one of this helper's callers is in the
/// commit-details pane. After the key/value reshape it has three, and all three
/// are the same role:
///
/// - the `Author` / `Date` / `Committer` / `Parents` **keys** in
///   [`paint_detail_list`];
/// - the em dash standing in for a value the commit does not have;
/// - the "N files changed" count under the churn bar.
///
/// So "a key is the muted step" is now the pane's stated rule rather than six
/// coincident call sites, and the values it names are [`Palette::INK`].
///
/// [`Palette::INK_3`] is also the right step for it rather than a stale one: R3
/// puts field labels and metadata on the muted step, at [`MICRO_TEXT`] on a
/// content surface, which clears 4.5:1. The dim step is reserved for
/// placeholders, dim path suffixes and hatches and may never be the only
/// rendering of something the user needs — a commit's author is exactly that.
/// `tests/git_log.rs` pins both halves: the ink is the ramp's, and it is never
/// the dim one.
fn micro_text(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
        .color(Palette::INK_3)
}

/// Whether a scrolled list sits settled at its bottom: `offset` pixels scrolled
/// from the top of a `content`-tall list inside a `viewport`-tall window, with
/// the last `tolerance` pixels counting as arrived.
///
/// Two rules make this the whole automatic trigger (plan P6): a list that does
/// not overflow its viewport is NEVER at the bottom — there is nothing to
/// scroll to, so a one-screen log never auto-pages — and the test is on where
/// the view rests, not on a scroll gesture, which is what lets the wheel, the
/// scrollbar and the keyboard all fire it.
pub fn settled_at_bottom(offset: f32, viewport: f32, content: f32, tolerance: f32) -> bool {
    content > viewport && offset + tolerance >= content - viewport
}

/// The longest prefix of `text` that is at most `budget` wide, with the text
/// layout pushed behind `measure` so the search itself needs no egui.
///
/// Cuts land on `char` boundaries, so a multibyte subject is shortened whole
/// chars rather than split mid-codepoint, and a subject too wide even on its
/// own keeps its first char rather than vanishing. A prefix only ever grows
/// wider, so the char count is found by binary search: the row pays a handful
/// of layouts where dropping one char at a time paid one per dropped char.
fn fit_to_budget(text: &str, budget: f32, measure: &dyn Fn(&str) -> f32) -> String {
    if measure(text) <= budget {
        return text.to_owned();
    }
    let prefix = |chars: usize| -> String { text.chars().take(chars).collect() };
    // `hi` is known not to fit (the whole text was measured a moment ago) and
    // `lo` is the one char kept unconditionally, so the search closes on the
    // last prefix that fits — or on that first char when none does.
    let (mut lo, mut hi) = (1usize, text.chars().count());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if measure(&prefix(mid)) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    prefix(lo)
}

#[cfg(test)]
mod tests {
    use super::{
        centre_scroll_delta, commit_index, fit_to_budget, row_is_in_view, row_offset_from_built,
        settled_at_bottom,
    };
    use std::sync::Arc;
    use turbogit_domain::model::{Commit, RootId, Signature, SignatureState};

    /// A row the index lookup can find, built without a repository: the lookup
    /// reads ids, so that is all a fixture has to be.
    fn row(id: &str) -> turbogit_app::log_display::LogRow {
        Arc::new(Commit {
            id: id.to_owned(),
            parents: vec![],
            author: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time: 0,
            },
            committer: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time: 0,
            },
            message: id.to_owned(),
            time: 0,
            root: RootId(Arc::from(std::path::Path::new("alpha"))),
            signature: SignatureState::Unsigned,
        })
    }

    /// A commit row's height — the theme's own value, so these tests are about
    /// ratios and cannot drift from the row model.
    const ROW_H: f32 = crate::theme::FILE_ROW_HEIGHT;
    /// The row pitch the log list pages at: the row height plus the row spacing
    /// egui adds, which ticket 07's suite measures off a live frame at 30.0.
    const PITCH: f32 = 30.0;

    /// Width oracle standing in for `layout_no_wrap` in the subject-fit tests:
    /// 6px per ASCII char, 10px per full-width one, 20px for the emoji — small
    /// hand-checkable numbers, so a fit can be verified by counting chars
    /// rather than by trusting a font.
    fn advance(c: char) -> f32 {
        if c == '🎉' {
            20.0
        } else if c.is_ascii() {
            6.0
        } else {
            10.0
        }
    }

    fn width_of(text: &str) -> f32 {
        text.chars().map(advance).sum()
    }

    /// A subject exactly as wide as the column is painted whole — the fit cuts
    /// only what does not fit.
    #[test]
    fn a_subject_that_fits_its_budget_is_returned_whole() {
        let subject = "alpha: second commit";
        let budget = width_of(subject);
        assert_eq!(fit_to_budget(subject, budget, &width_of), subject);
    }

    /// A subject far wider than the column comes back as the widest prefix
    /// that still fits — no ellipsis, and not one char shorter than it needs.
    #[test]
    fn a_subject_far_too_long_is_cut_to_the_widest_prefix_that_fits() {
        // 200 chars at 6px apiece: only ten of them (60px) fit a 60px column.
        let long = "a".repeat(200);
        assert_eq!(fit_to_budget(&long, 60.0, &width_of), "a".repeat(10));
        assert_eq!(fit_to_budget(&long, 59.0, &width_of), "a".repeat(9));
        assert_eq!(fit_to_budget(&long, 1_200.0, &width_of), long);
    }

    /// A multibyte subject is cut between chars and never through one. The old
    /// one-char-at-a-time drop was safe here only because each drop was a whole
    /// `char`; a byte-wise cut would split a codepoint.
    #[test]
    fn a_multibyte_subject_is_cut_on_a_char_boundary() {
        // Eight full-width chars (80px), then the emoji (20px), then four ASCII
        // chars (24px): 124px whole, and every cut below falls between chars.
        let subject = "日本語のコミット🎉done";
        let fitted = fit_to_budget(subject, 60.0, &width_of);
        assert_eq!(
            fitted, "日本語のコミ",
            "six full-width chars is exactly 60px"
        );
        assert!(
            subject.starts_with(&fitted),
            "the fit is a prefix of the subject, got {fitted:?}"
        );
        assert!(
            !fitted.contains('\u{fffd}'),
            "a char was split rather than dropped: {fitted:?}"
        );
        // The emoji is wide enough to matter: 99px stops short of it, 100px
        // keeps it whole.
        assert_eq!(fit_to_budget(subject, 99.0, &width_of), "日本語のコミット");
        assert_eq!(
            fit_to_budget(subject, 100.0, &width_of),
            "日本語のコミット🎉"
        );
    }

    /// A subject too wide on its own keeps its first char: the column is cut
    /// short before the text is cut away, and the old loop stopped at one char
    /// too rather than emptying the row.
    #[test]
    fn a_single_char_subject_is_never_cut_away_entirely() {
        assert_eq!(fit_to_budget("x", 1.0, &width_of), "x");
        assert_eq!(fit_to_budget("🎉", 1.0, &width_of), "🎉");
    }

    /// An empty subject stays empty — the row still paints, just nothing.
    #[test]
    fn an_empty_subject_stays_empty() {
        assert_eq!(fit_to_budget("", 24.0, &width_of), "");
    }

    /// The fit is bounded: a subject at the row's 44-char cap costs a handful
    /// of layouts, where dropping one char at a time cost one layout per
    /// dropped character — the reason this rule was worth changing.
    #[test]
    fn a_subject_at_the_row_cap_costs_a_bounded_number_of_layouts() {
        let long = "a".repeat(44);
        let calls = std::cell::Cell::new(0usize);
        let fitted = fit_to_budget(&long, 60.0, &|text: &str| {
            calls.set(calls.get() + 1);
            width_of(text)
        });
        assert_eq!(fitted, "a".repeat(10), "still the widest prefix that fits");
        assert!(
            calls.get() <= 7,
            "a 44-char subject must not cost a layout per dropped char, got {}",
            calls.get()
        );
    }

    const VIEWPORT: f32 = 300.0;
    const CONTENT: f32 = 1_000.0;
    /// The offset that puts the list's last pixel at the window's last pixel.
    const BOTTOM: f32 = CONTENT - VIEWPORT;
    const ROW: f32 = 24.0;

    /// A log that fits its pane has nothing left to scroll to, so it must not
    /// page itself — however far its rows sit from the bottom edge.
    #[test]
    fn a_list_that_fits_its_viewport_is_never_at_the_bottom() {
        assert!(!settled_at_bottom(0.0, VIEWPORT, 120.0, ROW));
        assert!(
            !settled_at_bottom(0.0, VIEWPORT, VIEWPORT, ROW),
            "an exact fit is still not an overflowing list"
        );
    }

    #[test]
    fn an_overflowing_list_arrives_within_one_row_of_its_end() {
        assert!(!settled_at_bottom(0.0, VIEWPORT, CONTENT, ROW));
        assert!(!settled_at_bottom(BOTTOM / 2.0, VIEWPORT, CONTENT, ROW));
        assert!(
            settled_at_bottom(BOTTOM - ROW, VIEWPORT, CONTENT, ROW),
            "one row short still counts as arrived"
        );
        assert!(settled_at_bottom(BOTTOM, VIEWPORT, CONTENT, ROW));
        assert!(
            !settled_at_bottom(BOTTOM - ROW - 1.0, VIEWPORT, CONTENT, ROW),
            "a row and a pixel short does not"
        );
    }

    // --- scroll-to-selected (issue 08) -----------------------------------------

    /// The offset of a row that the scroll area BUILT is zero, whatever its
    /// index: the reference is the first built row, not the top of the list.
    #[test]
    fn a_row_in_the_built_range_offsets_from_the_first_built_row() {
        assert_eq!(row_offset_from_built(10, 10..30, PITCH), 0.0);
        assert_eq!(row_offset_from_built(29, 10..30, PITCH), 19.0 * PITCH);
    }

    /// A row just above the window is a NEGATIVE offset — the arithmetic is
    /// signed, because a row index under the window's start would wrap a usize
    /// subtraction into a huge positive scroll and throw the list to the end of
    /// the history.
    #[test]
    fn a_row_above_the_visible_range_is_a_negative_offset() {
        assert_eq!(
            row_offset_from_built(9, 10..30, PITCH),
            -PITCH,
            "one row above the window is one pitch up"
        );
        assert_eq!(
            row_offset_from_built(0, 10..30, PITCH),
            -10.0 * PITCH,
            "the first row of the window, ten rows above the view, is ten pitches up"
        );
    }

    /// …and a row below it is the same distance the other way, measured from the
    /// window's FIRST built row rather than from its bottom edge: the row one
    /// past the end of a `10..30` window is twenty pitches down, not one.
    #[test]
    fn a_row_below_the_visible_range_is_a_positive_offset() {
        assert_eq!(
            row_offset_from_built(30, 10..30, PITCH),
            20.0 * PITCH,
            "the row one past the end of the window is twenty pitches below its first row"
        );
        assert_eq!(
            row_offset_from_built(140, 10..30, PITCH),
            130.0 * PITCH,
            "a row far below the window keeps its distance from the window's start"
        );
    }

    /// The last row of a window is reachable from the top, and the offset is the
    /// whole way there — which is what makes a selection from the blame view
    /// work on a commit far outside the drawn rows.
    #[test]
    fn the_last_row_of_a_window_is_reachable_from_the_top() {
        assert_eq!(
            row_offset_from_built(152, 0..20, PITCH),
            152.0 * PITCH,
            "the last row of a 153-row window is 152 pitches below the first built row"
        );
    }

    /// The same row asks for a different scroll depending on where the list
    /// already is, which is what makes the request land on the row rather than
    /// past it.
    #[test]
    fn the_same_row_scrolls_differently_from_a_different_offset() {
        let row = 100;
        assert_eq!(
            row_offset_from_built(row, 0..20, PITCH),
            100.0 * PITCH,
            "with the window at the top, row 100 is a hundred pitches down"
        );
        assert_eq!(
            row_offset_from_built(row, 95..99, PITCH),
            5.0 * PITCH,
            "with the window five rows above it, the same row is five pitches down"
        );
    }

    /// A row wholly inside the viewport is on screen, and the list is left
    /// alone: a selection made on screen must not move the list under the
    /// pointer. The viewport here starts 100 points below the first built row —
    /// i.e. the area built a row or two above the top edge, as it does.
    #[test]
    fn a_row_inside_the_viewport_is_left_alone() {
        // offset 150 → band 150..174, viewport 100..700: well inside.
        assert!(row_is_in_view(150.0, ROW_H, 100.0, 600.0));
        // Flush with both edges: inside, exactly.
        assert!(row_is_in_view(100.0, ROW_H, 100.0, 600.0));
        assert!(row_is_in_view(600.0 - ROW_H, ROW_H, 100.0, 600.0));
    }

    /// A row the scroll area built PAST the bottom edge is not on screen. This
    /// is the case that makes the viewport the test rather than the built range:
    /// `show_rows` builds one row beyond the fold, and treating that row as
    /// visible would leave a selection just out of sight — the one outcome the
    /// whole scroll-to-selected path exists to prevent.
    #[test]
    fn a_row_built_past_the_bottom_edge_is_not_on_screen() {
        let viewport_top = 100.0;
        let viewport_height = 600.0;
        let bottom = viewport_top + viewport_height;
        // The band hangs one point over the bottom edge.
        assert!(!row_is_in_view(
            bottom - ROW_H + 1.0,
            ROW_H,
            viewport_top,
            viewport_height
        ));
        // The band is entirely below it.
        assert!(!row_is_in_view(
            bottom + 10.0,
            ROW_H,
            viewport_top,
            viewport_height
        ));
        // The band is above the top edge, and a negative offset.
        assert!(!row_is_in_view(
            viewport_top - ROW_H,
            ROW_H,
            viewport_top,
            viewport_height
        ));
        // …and the row that ends exactly on the edge IS on screen, which is what
        // makes the last row of a window reachable without a scroll once the
        // list is at its end.
        assert!(row_is_in_view(
            bottom - ROW_H,
            ROW_H,
            viewport_top,
            viewport_height
        ));
    }

    /// A viewport with no height shows nothing, so nothing can be "already on
    /// screen" — a degenerate frame must not silence the scroll.
    #[test]
    fn an_empty_viewport_shows_no_rows() {
        assert!(!row_is_in_view(0.0, ROW_H, 0.0, 0.0));
    }

    /// A row below the middle scrolls towards the end of the history, and a row
    /// above it scrolls back — the sign is what `scroll_with_delta` inverts, so
    /// it is pinned here rather than left to be discovered at the call site.
    #[test]
    fn a_scroll_delta_points_from_the_row_towards_the_viewport_centre() {
        assert_eq!(
            centre_scroll_delta(100.0, 300.0),
            200.0,
            "a row 200 points ABOVE the middle scrolls back down the history"
        );
        assert_eq!(
            centre_scroll_delta(500.0, 300.0),
            -200.0,
            "a row 200 points BELOW the middle scrolls forward"
        );
        assert_eq!(
            centre_scroll_delta(300.0, 300.0),
            0.0,
            "centred needs no scroll"
        );
    }

    /// The commit→index lookup is over the held window's own order, so the index
    /// it returns is the index the list pages over. A commit the window does not
    /// hold has no index, and asking for a scroll to it must be a miss rather
    /// than a guess.
    #[test]
    fn a_commit_index_is_its_position_in_the_held_window() {
        let rows: Vec<_> = ["a", "b", "c"].into_iter().map(row).collect();
        assert_eq!(commit_index(&rows, "a"), Some(0));
        assert_eq!(commit_index(&rows, "c"), Some(2));
        assert_eq!(
            commit_index(&rows, "not-in-the-window"),
            None,
            "a commit outside the window has no index to scroll to"
        );
        assert_eq!(
            commit_index(&[], "a"),
            None,
            "an empty window scrolls nowhere"
        );
    }
}
