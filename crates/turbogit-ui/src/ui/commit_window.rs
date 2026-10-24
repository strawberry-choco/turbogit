//! The Commit tool window: one tree that mirrors Git's index, beside the Shelf
//! and Stash panes.
//!
//! Layout: a sub-tab strip (Local Changes / Shelf / Stash) above two zones — a
//! fixed-width Commit panel of collapsible repo groups with count badges on the
//! left, and a permanent diff-preview pane (above the message editor and the
//! Amend / Commit / Commit-and-Push action row) on the right. Local Changes is
//! **one flat, path-sorted list per repository** — unstaged, staged and
//! untracked files together — plus a Merge conflicts group. Empty buckets are
//! dropped, ignored files never enter the list, and staging state is said by
//! the per-file checkbox rather than by a heading, so a staging action never
//! re-sorts the list under the cursor.
//!
//! **The checkbox is a view of the index, not a set of its own.** Its three
//! states are derived per frame from the status read's staged/unstaged pair, and
//! clicking it stages or un-stages the whole file through plain git. Clicking
//! the row body is a browse, never an edit, and the modifier held at the click
//! picks the gesture out of [`changes_selection`]: a plain click selects that
//! one file and clears the rest of the set, Command/Ctrl (Cmd on macOS, Ctrl on
//! Windows and Linux) toggles one file in or out, and Shift+Click replaces the
//! set with the contiguous painted range from the anchor. Every gesture previews
//! the row it landed on, and a conflicted row is previewed like any other while
//! staying out of the set — that set is the input to Discard Changes, Shelve and
//! Create Patch, and none of the three guards an unmerged path.
//! `AppState::ui.selected` therefore means "the files I am looking at": it drives
//! the row highlight, Create Patch from Local Changes, Discard Changes and the
//! Shelve dialog, and the commit path does not read it at all. Commit commits the
//! index and is disabled with a stated reason when the index is empty.
//!
//! For multi-root projects each repository is its own collapsible group, which
//! is the only way to tell which repository a file belongs to. Shelf and Stash
//! are real panes — each root's own shelf records and its own `git stash`
//! listing, with the keep/remove verb pairs on every row — which supersedes
//! ADR-0008's labelled placeholders. The "Advanced options..." control renders
//! per the mockup but is deliberately inert in v1 (ADR-0010). User-created
//! changelists remain backlog.
//!
//! Design system v2: the changes card is headed by the one shared pane header
//! ([`widgets::pane_header`]) — a tracked title, a count chip for the *file*
//! count, a right-aligned action slot, one hairline — and its action slot holds
//! three controls (refresh, rollback, expand/collapse-all) rather than five. The
//! two that cannot act (commit options, group by) live in the header's overflow
//! menu as visibly inert rows, reachable and honest (ADR-0010). The file filter
//! is the card's first content row, under the hairline, at the full inner width.
//!
//! The **commit card** above the list is three controls, not six: the message
//! well (about 76 pt, on the raised-on-card fill because the card is a
//! content-surface card), one meta row carrying the subject counter, Amend,
//! Template and Clear, and one content-width Commit control with its chevron
//! *inside* it across a single 1 px rule. The commit panel's width is
//! unchanged in both cases: the fix was removing the constraints inside the
//! column, not widening the column.

use crate::theme::Palette;
use crate::ui::changes_selection::{self, ClickKind, RowSlot};
use crate::ui::column_table::{self, PaneTable};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use crate::ui::widgets::PaneColumn;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Key, Layout, Margin, Pos2, Rect, RichText,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::path::{Path, PathBuf};
use turbogit_app::root_caches::Affected;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, CommitSubTab, Dialog, PendingConfirm, Toast};
use turbogit_domain::model::{Change, ChangeRef, ChangeStatus, DisplayName, Root, RootId};

/// Fixed width of the Commit panel (redesign 03, screen 01): the Commit
/// window is exactly two zones — this fixed-width panel on the left and
/// a flexible diff preview on the right.
pub const COMMIT_PANEL_WIDTH: f32 = 340.0;

/// Name of the one flat bucket every non-conflicted change of a repository
/// falls into. It paints nothing — staging state and repository both have their
/// own header — and exists so the row block's id can tell the flat list from
/// the conflicts group below it.
const CHANGES: &str = "Changes";

/// The conflicts group's name, and the only heading the file list paints.
/// Conflicts are a different axis from staging and a distinct review surface,
/// so they stay visibly grouped.
pub const MERGE_CONFLICTS: &str = "Merge conflicts";

/// One canonical bucket: its name plus that root's changes. Borrows the
/// root's status snapshot — rows render against a shared [`AppState`] and
/// their clicks are deferred (plan §1.4), so no change is cloned per frame.
struct Bucket<'a> {
    name: &'static str,
    root: &'a Root,
    changes: Vec<&'a Change>,
}

/// How much of one file the index currently holds. The checkbox is a view of
/// this and never stores it, so "ticked but not staged" cannot be represented
/// and the box survives a refresh with no extra git call. An enum rather than
/// the `staged`/`unstaged` pair it is derived from, because that pair allows
/// the combination no row means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StageState {
    None,
    Partial,
    All,
}

impl StageState {
    /// Derive from the status read's pair: fully staged, partly staged, or not
    /// staged at all. `staged` alone is the index's half, `unstaged` the
    /// working tree's, and a file is "all" only when the first is on and the
    /// second is off.
    fn of(c: &Change) -> Self {
        match (c.staged, c.unstaged) {
            (true, false) => Self::All,
            (true, true) => Self::Partial,
            _ => Self::None,
        }
    }
}

/// Deferred row interaction collected while rendering borrowed buckets;
/// applied after the pane finishes (same pattern as the widget button seam).
enum RowAction {
    /// Stage or un-stage one whole file: root id so multi-root projects never
    /// conflate same-named files, repo-relative path so git accepts it. The
    /// direction is decided from the row's [`StageState`] when the action is
    /// applied, so the click and the painted box cannot disagree.
    ToggleStage { root: RootId, path: PathBuf },
    /// Expand / collapse one repo group of the one-tree (issue 04), keyed
    /// by root id. The focused repo is never the target — its group is
    /// always expanded ("focus = expand").
    ToggleGroup(turbogit_domain::model::RootId),
    /// One row-body click: the gesture [`changes_selection`] applies to the
    /// selection, the anchor and the diff preview. Keyed by the canonical
    /// root-scoped path because the selection is per-root, and carrying the
    /// [`ClickKind`] the held modifier picked at the click site, because which
    /// gesture this is cannot be recovered after the frame. The root is the row's
    /// own, for the reason [`Self::ToggleStage`] states: the preview has to be
    /// read in the repository the row belongs to.
    Select {
        key: PathBuf,
        root: RootId,
        path: PathBuf,
        kind: ClickKind,
    },
}

/// One root's changes as two buckets: the flat file list, then the conflicts
/// group. This used to do two jobs at once — ordering by staging state *and*
/// keeping noise out — and only the ordering went: ignored files are still
/// dropped here, untracked files now stay (they are new files the checkbox
/// stages, and a section for them would put staging state back in the page).
fn change_buckets(state: &AppState) -> Vec<Bucket<'_>> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let mut files = Vec::new();
        let mut conflicts = Vec::new();
        for c in &root.status.changes {
            match c.status {
                ChangeStatus::Conflicted => conflicts.push(c),
                ChangeStatus::Ignored => {}
                _ => files.push(c),
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
        for (name, changes) in [(CHANGES, files), (MERGE_CONFLICTS, conflicts)] {
            if !changes.is_empty() {
                out.push(Bucket {
                    name,
                    root,
                    changes,
                });
            }
        }
    }
    out
}

/// Apply the row interactions collected during one pane's render pass.
///
/// `order` is that same pass's paint order, so a range spans exactly the rows
/// the developer could see.
fn apply_actions(state: &mut AppState, actions: Vec<RowAction>, order: &[RowSlot]) {
    for action in actions {
        match action {
            RowAction::ToggleStage { root, path } => set_file_staged(state, &root, &path),
            RowAction::Select {
                key,
                root,
                path,
                kind,
            } => {
                changes_selection::apply(
                    &mut state.ui.selected,
                    &mut state.ui.selection_anchor,
                    order,
                    &key,
                    kind,
                );
                // The preview follows every gesture, range and toggle included:
                // the row the pointer landed on is the row to look at.
                state.ui.preview_change = Some(ChangeRef { root, path });
            }
            RowAction::ToggleGroup(id) => {
                if state.ui.changes_expanded.contains(&id) {
                    state.ui.changes_expanded.remove(&id);
                } else {
                    state.ui.changes_expanded.insert(id);
                }
            }
        }
    }
}

/// One checkbox click's whole effect: stage the file if the index does not
/// already hold all of it, un-stage it if it does.
///
/// Two callers — the row's checkbox and `Space` on the previewed row — and one
/// function, so the keyboard cannot take a different route than the mouse.
/// The direction is read from the status snapshot rather than from a stored
/// flag, which is the same fact the box paints.
fn set_file_staged(state: &mut AppState, root: &RootId, path: &Path) {
    let Some(change) = state.multi.by_id(root).and_then(|r| r.resolve_change(path)) else {
        return;
    };
    let stage = StageState::of(change) != StageState::All;
    state.dispatch(shell_writes::commit::set_staged(root, path, stage));
}

/// Space: toggle the staging checkbox of the row being previewed. The
/// previewed row is the single file this window is looking at, in whichever root
/// owns it, and a row click is what sets it. With nothing previewed there is no
/// focused row and the key does nothing; defaulting to the first row would stage
/// a file nobody aimed at.
pub fn toggle_previewed_row_checkbox(state: &mut AppState) {
    let Some(change) = state.ui.preview_change.clone() else {
        return;
    };
    if state
        .multi
        .by_id(&change.root)
        .and_then(|r| r.resolve_change(&change.path))
        .is_some_and(|c| c.status == ChangeStatus::Conflicted)
    {
        // A conflicted path has no checkbox, so it has no Space gesture either.
        return;
    }
    set_file_staged(state, &change.root, &change.path);
}

/// Collect the `Change` objects of the selected root the developer is looking
/// at — the input to Create Patch from Local Changes, Discard Changes and
/// Shelve. Not the commit's input: that is the index.
fn selected_changes(state: &AppState) -> Vec<Change> {
    let mut out = Vec::new();
    if let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
    {
        for c in &root.status.changes {
            if state.ui.selected.contains(&root.canonical_key(c)) {
                out.push(c.clone());
            }
        }
    }
    out
}

/// Whether the selected root's index holds anything — the Commit control's
/// half-gate, computed without building an owned change list (plan §1.4).
///
/// Not "is anything selected": a row click is a preview, and previewing a file
/// is not a promise to commit it.
fn has_staged_changes(state: &AppState) -> bool {
    state.selected_root.as_ref().is_some_and(|id| {
        state
            .multi
            .by_id(id)
            .is_some_and(|root| root.status.changes.iter().any(|c| c.staged))
    })
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    // Hunk-span statistics (issue 20) feed the diff pane's per-hunk staged state
    // and a whole-hunk click's resolution to index-facing section hunks; computed
    // on miss through the engine the app owns, keyed per root and invalidated
    // with the other root caches.
    if let Some(root_id) = state.selected_root.clone() {
        state.ensure_hunk_stats(&root_id);
    }
    // The pane previews, and a granular verb stages, in the root the previewed
    // change names — whose cold statistics are the same requirement.
    if let Some(root_id) = state.ui.preview_change.as_ref().map(|c| c.root.clone()) {
        state.ensure_hunk_stats(&root_id);
    }
    sub_tab_strip(ui, state);
    match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => local_changes_body(ui, state),
        // ADR-0008 made these two labelled placeholders because the panes had
        // nothing behind them. They have panes now: a shelf list and a stash
        // list, each with its verbs wired to the dispatch that does the work.
        CommitSubTab::Shelf => shelf_pane(ui, state),
        CommitSubTab::Stash => stash_pane(ui, state),
    }

    // Conflict resolution tools (ours / theirs / 3-way merge editor); the
    // renderer no-ops while nothing is conflicted.
    crate::ui::conflicts::render(ui, state);

    if let Some(err) = &state.last_error {
        ui.separator();
        widgets::inline_error(ui, format!("⚠ {err}"));
    }
}

// --------------------------------------------------------- sub-tab strip --

/// Sub-tabs in strip order. All three render a real pane; ADR-0008's placeholder
/// treatment is superseded for Shelf and Stash, which list this root's shelves
/// and git's stash entries. Unversioned Files was removed in the one-tree
/// redesign (issue 04).
const SUB_TABS: [(CommitSubTab, &str); 3] = [
    (CommitSubTab::LocalChanges, "Local Changes"),
    (CommitSubTab::Shelf, "Shelf"),
    (CommitSubTab::Stash, "Stash"),
];

fn sub_tab_strip(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        for (tab, label) in SUB_TABS {
            if ui
                .selectable_label(state.ui.commit_subtab == tab, label)
                .clicked()
            {
                state.ui.commit_subtab = tab;
            }
        }
    });
    ui.separator();
}

// ------------------------------------------------------------ file filter --

/// The changes card header's file filter (spec R7, CONTEXT.md "File filter"):
/// one input over the changed-file list, matched case-insensitively against
/// file paths. `/` focuses it (via
/// `focus_file_filter`, armed by the shell or the Filter Files palette
/// action); Esc while focused clears focus and text; otherwise the text
/// persists across root switches and refreshes within the session.
fn file_filter_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let resp = widgets::search_input(ui, "Filter files", &mut state.ui.file_filter);
        if state.ui.focus_file_filter {
            resp.request_focus();
            state.ui.focus_file_filter = false;
        }
        // egui surrenders focus on bare Escape; pair that transition with
        // clearing the query so Esc means "filter off".
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            state.ui.file_filter.clear();
        }
    });
}

/// Narrow `buckets` to changes whose path contains the file filter
/// (case-insensitive substring, log-search style). Returns the filtered
/// buckets plus the zero-match message to paint — empty when no filter is
/// active and the caller's own empty text applies.
fn filter_buckets<'a>(state: &AppState, mut buckets: Vec<Bucket<'a>>) -> (Vec<Bucket<'a>>, String) {
    let query = state.ui.file_filter.trim().to_lowercase();
    if query.is_empty() {
        return (buckets, String::new());
    }
    for bucket in &mut buckets {
        bucket
            .changes
            .retain(|c| widgets::filter_matches(&c.path.display().to_string(), &query));
    }
    buckets.retain(|b| !b.changes.is_empty());
    let shown = state.ui.file_filter.trim();
    (buckets, format!("No files match '{shown}'."))
}

// ------------------------------------------------------------ tab bodies --

fn local_changes_body(ui: &mut Ui, state: &mut AppState) {
    // Two zones (redesign 03): a fixed-width Commit panel on the left, a
    // flexible diff preview on the right. Both always render — the preview
    // is a permanent pane, not a toggleable tab. The left pane is the one
    // tree (repo groups + the bottom `Unversioned Files` group, issue 04);
    // there is no separate Unversioned Files sub-tab body anymore.
    two_zone(ui, state, changelist_pane, preview_and_editor_pane);
}

/// Layout helper for the two-zone Commit window (redesign 03): the left
/// pane gets the fixed [`COMMIT_PANEL_WIDTH`], the right pane (diff
/// preview + message editor) takes the flexible remainder.
fn two_zone(
    ui: &mut Ui,
    state: &mut AppState,
    left: impl FnOnce(&mut Ui, &mut AppState),
    right: impl FnOnce(&mut Ui, &mut AppState),
) {
    let avail = ui.available_rect_before_wrap();
    let left_rect = Rect::from_min_max(
        avail.min,
        Pos2::new(avail.min.x + COMMIT_PANEL_WIDTH, avail.max.y),
    );
    let right_rect = Rect::from_min_max(Pos2::new(left_rect.max.x, avail.min.y), avail.max);
    let mut left_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    left(&mut left_ui, state);
    let mut right_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    right(&mut right_ui, state);
    // Advance the caller's cursor by the panes' *used* height, matching
    // `ui.columns`: sections rendered after the body (e.g. the conflict
    // resolution tools) must flow beneath the content, not below the full
    // reserved rect (which would clip them past the window's bottom edge).
    let max_height = left_ui.min_size().y.max(right_ui.min_size().y);
    ui.advance_cursor_after_rect(Rect::from_min_size(
        avail.min,
        Vec2::new(avail.width(), max_height),
    ));
}

// ------------------------------------------------------ shelf & stash panes --
//
// Two panes, one column-table grammar each (ADR-0008's placeholders superseded
// the frame they were drawn for). The reference keeps them separate by default
// and the distinction is the point of having two: **a shelf** is this app's own
// named bucket of *selected* changes — its record lives in `.turbogit/shelf.ron`
// and its content is the stash entry the shelve parked — while **a stash** is
// git's stack of *every* uncommitted change, read from `git stash list`. The
// verbs say which mechanism a row belongs to and what happens to the copy: the
// keep/remove pair is named, not tooltiped.

/// The Shelf pane's columns, one table read by both the shared column-header row
/// and every row below it, for the reason `WORKTREE_COLUMNS` records.
const SHELF_COLUMNS: [PaneColumn; 4] = [
    PaneColumn::start("SHELF", 8.0),
    PaneColumn::start("FILES", 320.0),
    PaneColumn::start("SHELVED", 392.0),
    PaneColumn::end("ACTIONS", 0.0),
];

/// The Stash pane's columns. Same shape, different facts: git's own ref is the
/// identity the verbs hand the engine, so it leads.
const STASH_COLUMNS: [PaneColumn; 3] = [
    PaneColumn::start("STASH", 8.0),
    PaneColumn::start("MESSAGE", 112.0),
    PaneColumn::end("ACTIONS", 0.0),
];

fn shelf_pane(ui: &mut Ui, state: &mut AppState) {
    column_table::column_table_pane(
        ui,
        state,
        PaneTable {
            title: "SHELVES",
            columns: &SHELF_COLUMNS,
            // The shelf store is app state rather than a git read, so it always
            // answers; this line is the pane's own absence word, never a promise
            // of an arrival that cannot be pending.
            loading: "No shelf store for this project yet.",
            empty: "Nothing is shelved in this repository. Use “Shelve…” to park the selected \
                    changes and take the working tree back to clean.",
            cached: |state, id| Some(state.shelves_for(id)),
            actions: &mut |ui, state| {
                if widgets::ghost_button(ui, None, "Shelve\u{2026}")
                    .on_hover_text("Shelve selected changes")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::Shelve);
                }
                // The shelf accepts patch FILES as a source, not only changes this
                // application shelved. Importing is the deferral: the bytes are parked,
                // nothing is applied, and `Unshelve` is what later puts them in this
                // root's tree. It sits in **this pane**, whose rows are already one
                // root's, because that is what makes the root it parks under a stated
                // fact rather than a guess at the focus.
                if widgets::ghost_button(ui, None, "Import Patches\u{2026}")
                    .on_hover_text("Park a patch file on this repository's shelf")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::ImportPatches);
                }
                // The dialog route sits beside the verbs rather than replacing them:
                // the row buttons carry the keep/remove choice in their names, while
                // this window is where the choice is *asked for* — which shelf, and
                // whether it survives the apply.
                if widgets::ghost_button(ui, None, "Unshelve\u{2026}")
                    .on_hover_text("Unshelve a shelf from this repository")
                    .clicked()
                {
                    state.open_unshelve_dialog();
                }
            },
            row: &mut shelf_row,
        },
    );
}

fn stash_pane(ui: &mut Ui, state: &mut AppState) {
    // **A cached git value, asked for through the app.** The pane never touches
    // the executor: it asks, the app admits at most one listing per root in
    // flight, and the frame before the answer arrives paints the loading word
    // rather than an absence git has not confirmed.
    let cold = state
        .selected_root
        .clone()
        .filter(|id| state.caches.stashes(id).is_none());
    if let Some(id) = cold {
        state.fetch_stashes(id);
    }
    column_table::column_table_pane(
        ui,
        state,
        PaneTable {
            title: "STASHES",
            columns: &STASH_COLUMNS,
            loading: "Loading stashes\u{2026}",
            empty: "Nothing is stashed in this repository. Use “Stash…” to park every uncommitted \
                    change, in or out of this application.",
            cached: |state, id| state.caches.stashes(id).map(|rows| rows.to_vec()),
            actions: &mut |ui, state| {
                if widgets::ghost_button(ui, None, "Stash\u{2026}")
                    .on_hover_text("Stash all changes")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::Stash);
                }
            },
            row: &mut stash_row,
        },
    );
}

/// One shelf row: its name, the files it holds, when it was shelved, and the
/// three verbs.
///
/// **The two unshelve verbs are one pair with opposite defaults**, named so the
/// consequence travels in the label — `Unshelve` keeps the shelf (the captured
/// default: `Remove successfully applied files from the shelf` is **off**), and
/// `Unshelve and Remove` destroys the shelved copy with it. A single control with
/// a checkbox would lose exactly the distinction the reference spells out with
/// two menu items, so they are two rects with two fills.
fn shelf_row(ui: &mut Ui, state: &mut AppState, shelf: &turbogit_domain::model::Shelf) {
    let root = shelf.root.clone();
    let name = shelf.name.clone();
    let files = components::counted(shelf.changes.len(), "file", "files");
    let shelved = shelf.created_at.format("%Y-%m-%d %H:%M").to_string();
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &SHELF_COLUMNS, 0, |ui| {
            ui.label(RichText::new(&shelf.name).color(Palette::INK));
        });
        widgets::column_cell(ui, &SHELF_COLUMNS, 1, |ui| {
            ui.label(RichText::new(files).color(Palette::INK_2));
        });
        widgets::column_cell(ui, &SHELF_COLUMNS, 2, |ui| {
            ui.label(RichText::new(shelved).color(Palette::INK_2));
        });
        // Right-to-left, so the painted left-to-right order is keep, destroy,
        // delete: the reference's own menu order, and the safe verb first.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if components::kit_button(ui, components::KitButton::Quiet, "Delete Shelf").clicked() {
                state.ui.confirm = Some(PendingConfirm::DeleteShelf {
                    root: root.clone(),
                    name: name.clone(),
                });
            }
            if components::kit_button(ui, components::KitButton::Danger, "Unshelve and Remove")
                .clicked()
            {
                state.unshelve(root.clone(), name.clone(), true);
            }
            if components::kit_button(ui, components::KitButton::Secondary, "Unshelve").clicked() {
                state.unshelve(root.clone(), name.clone(), false);
            }
        });
    });
}

/// One stash row: git's ref, the entry's message, and the three verbs.
///
/// `Apply Stash (keeps)` and `Pop Stash (removes)` are the pair the reference
/// disambiguates **only by tooltip**, which the parity spec records as a
/// discoverability trap; here the consequence is in the label and the fill, and
/// `Delete Stash` uses the shelf pane's one verb with its own explicit object
/// rather than a third word for removal.
fn stash_row(ui: &mut Ui, state: &mut AppState, stash: &turbogit_domain::model::Stash) {
    let root = stash.root.clone();
    let index = stash.index;
    let stash_ref = turbogit_app::shelve_stash::stash_ref(index);
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &STASH_COLUMNS, 0, |ui| {
            ui.label(RichText::new(&stash_ref).color(Palette::INK));
        });
        widgets::column_cell(ui, &STASH_COLUMNS, 1, |ui| {
            ui.label(RichText::new(&stash.message).color(Palette::INK_2));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if components::kit_button(ui, components::KitButton::Quiet, "Delete Stash").clicked() {
                state.ui.confirm = Some(PendingConfirm::DeleteStash {
                    root: root.clone(),
                    index,
                });
            }
            if components::kit_button(ui, components::KitButton::Danger, "Pop Stash (removes)")
                .clicked()
            {
                state.dispatch(shell_writes::stash::pop_at(&root, &stash_ref, index));
            }
            if components::kit_button(ui, components::KitButton::Secondary, "Apply Stash (keeps)")
                .clicked()
            {
                state.dispatch(shell_writes::stash::apply_at(&root, &stash_ref, index));
            }
        });
    });
}

// ------------------------------------------------------- changelist pane --

fn changelist_pane(ui: &mut Ui, state: &mut AppState) {
    // Issue 07: one set of commit controls — the message box + Amend, then the
    // single action row (primary `Commit ▾` plus grey `Shelve…` / `Stash…`).
    // Redesign Phase 1/2: they share one bordered card, which is what groups
    // them now that the heading and separators are gone, and the action row is
    // that card's footer.
    widgets::card(ui, widgets::CardFrame::default(), |ui| {
        commit_message_box(ui, state);
        recent_messages_row(ui, state);
        // The mockup rules the footer off from the message controls above it:
        // the action row is the card's footer, and it is where the card's third
        // part — the one Commit control — lives.
        ui.separator();
        commit_action_row(ui, state);
    });

    // Redesign Phase 3: the file list is its own card, headed by the shared pane
    // header (R7), so the three live controls read as acting on this list rather
    // than as commit-box chrome.
    widgets::card(ui, widgets::CardFrame::default(), |ui| {
        changes_card_header(ui, state);

        let Some(root_id) = state.selected_root.clone() else {
            ui.colored_label(Color32::GRAY, "Select a repository to see changes.");
            return;
        };
        if state.multi.by_id(&root_id).is_none() {
            return;
        }

        // Focus = expand (issue 04): whenever the selected root moves, manual
        // expansions reset so only the newly focused repo's files are visible.
        // Normalized before the buckets borrow the status snapshots (plan §1.4).
        if state.ui.changes_focus != state.selected_root {
            state.ui.changes_focus = state.selected_root.clone();
            state.ui.changes_expanded.clear();
        }

        let (buckets, no_match) = filter_buckets(state, change_buckets(state));
        // **This site produces the empty state's words and does not paint it.** The
        // sentence is the *filter's* answer — the clean-tree statement when nothing matched,
        // and the filter's own "nothing matches <query>" when a query excluded
        // everything — so the wording stays here. Moving the paint up to this scope
        // would make the tree unable to report "nothing painted", which is the fact
        // that decides whether the sentence appears at all.
        //
        // The clean-tree wording is the reference's `stage.default.status` (ticket
        // 19): an empty list and a sentence are not the same information. It
        // inherits the old sentence's one weakness — it is a claim about the last
        // refresh, so it can appear for a frame before a scan lands.
        let empty_text = if no_match.is_empty() {
            "Nothing to commit, working tree is clean."
        } else {
            &no_match
        };
        let mut actions = Vec::new();
        let mut order = Vec::new();
        changes_tree(ui, state, &buckets, empty_text, &mut actions, &mut order);
        apply_actions(state, actions, &order);
    });
}

/// The one tree (issue 04, visual doc §3): every repository renders as a
/// collapsible group — chevron + repo icon + name + dim branch + file-count
/// badge. Non-focused groups collapse by default so the selected repo's
/// files are the visual focus ("focus = expand"); the user can expand any
/// other group manually ([`AppState::ui`] `changes_expanded`). Each expanded
/// group lists that repo's files flat and path-sorted, with the conflicts
/// group below.
///
/// `order` collects one [`RowSlot`] per painted row, in this pass's paint order.
fn changes_tree(
    ui: &mut Ui,
    state: &AppState,
    buckets: &[Bucket<'_>],
    empty_text: &str,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    // Height-capped: the tree is the last region inside its card, so it fills
    // the rest of the pane rather than growing the card past the window
    // (redesign risk R1 — the action row it used to reserve room for now
    // heads the card above it). Uses the available rect instead of the capped
    // pane height so sections rendered after the two-zone body (e.g. conflict
    // resolution tools) keep real geometry.
    let remaining = ui.available_rect_before_wrap();
    let max_h = remaining.height().max(crate::theme::FILE_ROW_HEIGHT);
    egui::ScrollArea::vertical()
        .id_salt("changes_tree")
        .max_height(max_h)
        .show(ui, |ui| {
            let mut painted = false;
            for root in &state.multi.roots {
                let root_buckets: Vec<&Bucket> =
                    buckets.iter().filter(|b| b.root.id == root.id).collect();
                if root_buckets.is_empty() {
                    continue;
                }
                painted = true;
                repo_group(ui, state, root, &root_buckets, actions, order);
            }
            if !painted {
                widgets::empty_state(ui, empty_text);
            }
        });
}

/// One group header of the changes tree, as a value so the three call sites
/// cannot each invent their own header (conformance issue 10).
struct GroupHeader<'a> {
    /// The accessible label for the whole row: what a screen reader announces
    /// and what the suites around this window address the group by. The
    /// *painted* name is [`Self::name`], which is deliberately not the same
    /// string — the count is a chip beside the name, never characters inside
    /// it.
    label: &'a str,
    /// The name painted on the band, in the group-name type role.
    name: &'a str,
    /// Dim metadata painted after the name (a repo's branch), if any.
    meta: Option<&'a str>,
    /// How many files the group holds, as the shared count chip.
    count: usize,
    /// `Some(open)` for a collapsible group: the chevron states it and the row
    /// is a click target. `None` for a header that heads a list with no
    /// collapse of its own (the Merge conflicts group), which is a static band
    /// rather than a button that does nothing.
    expanded: Option<bool>,
    /// Whether this row takes the focus band — the selected repository's
    /// group, which is also the one that is always expanded.
    focused: bool,
    /// A leading glyph between the chevron and the name, where the group has
    /// one (a repository's folder).
    lead: Option<Icon>,
}

/// Paint one group header: the section band, the shared row's hover, the
/// chevron, the name, its metadata, and the file count as a count chip.
///
/// **The band is [`Palette::SECTION_BG`] at [`CornerRadius::ZERO`], not a
/// card.** A group is scaffolding on the changes card; a card inside a card
/// would put a floating border and a second surface around a list the card
/// already contains, and the square corners are what say so at a glance. The
/// row state paints *over* the band rather than replacing it, so a hovered or
/// focused group is still visibly on it.
///
/// Hover is read from [`Ui::rect_contains_pointer`] rather than the response:
/// the count chip below is a later-registered widget over the same rect, and
/// asking the response would make the band flicker off whenever the pointer
/// crossed the number.
fn group_header_row(ui: &mut Ui, id: egui::Id, header: GroupHeader<'_>) -> egui::Response {
    let width = ui.available_width();
    let row = Rect::from_min_size(
        ui.cursor().left_top(),
        Vec2::new(width, crate::theme::GROUP_ROW_HEIGHT),
    );
    ui.allocate_exact_size(row.size(), Sense::hover());

    let collapsible = header.expanded.is_some();
    let response = ui.interact(
        row,
        id,
        if collapsible {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        WidgetInfo::labeled(
            if collapsible {
                WidgetType::Button
            } else {
                WidgetType::Label
            },
            true,
            header.label,
        )
    });

    let painter = ui.painter().clone();
    painter.rect_filled(row, CornerRadius::ZERO, Palette::SECTION_BG);
    // The one row-state decision, over the band: the focus band for the
    // selected repository's group, the shared hover fill for every other.
    let state = if header.focused {
        components::RowState::FocusSelected
    } else if ui.rect_contains_pointer(row) {
        components::RowState::Hover
    } else {
        components::RowState::Default
    };
    widgets::paint_row(ui, row, state);

    let cy = row.center().y;
    if let Some(open) = header.expanded {
        icons::centered_icon(
            ui,
            if open {
                Icon::CHEVRON_DOWN
            } else {
                Icon::CHEVRON_RIGHT
            },
            Pos2::new(row.left() + GROUP_CHEVRON_X, cy),
            12.0,
            Palette::INK_3,
        );
    }
    if let Some(glyph) = header.lead {
        icons::centered_icon(
            ui,
            glyph,
            Pos2::new(row.left() + GROUP_ICON_X, cy),
            14.0,
            if header.focused {
                Palette::BRAND
            } else {
                Palette::INK_2
            },
        );
    }

    // The name is the group's answer, so it reads at the primary ink; the
    // branch beside it is metadata and dims.
    let name_x = row.left() + GROUP_NAME_X;
    let name_galley = painter.layout_no_wrap(
        header.name.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_GROUP_NAME),
        Palette::INK,
    );
    let name_w = name_galley.size().x;
    painter.galley_with_override_text_color(
        Pos2::new(name_x, cy - name_galley.size().y / 2.0),
        name_galley,
        Palette::INK,
    );
    if let Some(meta) = header.meta {
        let meta_galley = painter.layout_no_wrap(
            meta.to_owned(),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            Palette::INK_3,
        );
        painter.galley_with_override_text_color(
            Pos2::new(name_x + name_w + GROUP_GAP, cy - meta_galley.size().y / 2.0),
            meta_galley,
            Palette::INK_3,
        );
    }

    // The count is the shared **count chip**, right-aligned in a reserved
    // trailing zone: a number is a number in every pane, and a count is never
    // characters inside the group's name.
    let trailing = Rect::from_min_max(
        Pos2::new(row.right() - GROUP_COUNT_SLOT, row.top()),
        Pos2::new(row.right() - GROUP_EDGE, row.bottom()),
    );
    let mut chip_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(trailing)
            .layout(Layout::right_to_left(Align::Center)),
    );
    chip_ui.spacing_mut().item_spacing.x = 0.0;
    widgets::count_chip(&mut chip_ui, &header.count.to_string());

    response
}

/// One collapsible repo group of the one-tree (issue 04). The focused repo
/// is always expanded ("focus = expand"); every other group is collapsed
/// unless the user toggled it open. Clicking a non-focused group header
/// records a [`RowAction::ToggleGroup`]; clicking the focused group is a
/// no-op (its files are the visual focus).
fn repo_group(
    ui: &mut Ui,
    state: &AppState,
    root: &Root,
    buckets: &[&Bucket<'_>],
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    let focused = state.selected_root.as_ref() == Some(&root.id);
    let expanded = focused || state.ui.changes_expanded.contains(&root.id);
    let name = root.id.name();
    // Dim branch label; detached repos fall back to the sidebar's marker.
    let branch = root
        .current_branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    let count: usize = buckets.iter().map(|b| b.changes.len()).sum();

    let response = group_header_row(
        ui,
        ui.auto_id_with(("repo_group", &root.id)),
        GroupHeader {
            label: &format!("Toggle {name}"),
            name: &name,
            meta: Some(&branch),
            count,
            expanded: Some(expanded),
            focused,
            lead: Some(Icon::FOLDER_GIT),
        },
    );
    if response.clicked() && !focused {
        actions.push(RowAction::ToggleGroup(root.id.clone()));
    }

    if expanded {
        ui.indent(ui.id().with(("repo_group_body", &root.id)), |ui| {
            for bucket in buckets {
                group_section(ui, state, bucket, actions, order);
            }
        });
    }
}

/// Paint one tree indent guide (redesign Phase 5): a 1px `LINE_SUBTLE`
/// vertical connector down an expanded block, so a file row reads as hanging
/// off its repo group instead of floating beside it.
///
/// `block` is the row block's own rect, so the guide spans exactly the rows it
/// ties to and stops there. It lands in the gap between the row checkbox and
/// the status letter — never left of the checkbox, whose column it would
/// otherwise cut through (risk R3).
///
/// Drawn as a hairline fill rather than a stroked `line_segment`: the two are
/// indistinguishable on screen, but the harness's painted-path query reports
/// icon and focus-ring paths only, so a stroked segment would be invisible to
/// the test that owns this behaviour.
fn indent_guide(ui: &Ui, block: Rect) {
    let x = block.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP / 2.0;
    ui.painter().rect_filled(
        Rect::from_min_max(
            Pos2::new(x, block.top()),
            Pos2::new(x + 1.0, block.bottom()),
        ),
        CornerRadius::ZERO,
        Palette::LINE_SUBTLE,
    );
}

/// One bucket's file rows inside an expanded repo group. Only the conflicts
/// bucket paints a header: it is a different axis from staging and a distinct
/// review surface with no checkbox, so it stays visibly grouped — on the same
/// shared band every other group header wears, not as a bare bold line. The
/// flat list paints no heading of its own, which is the whole point of it.
fn group_section(
    ui: &mut Ui,
    state: &AppState,
    bucket: &Bucket<'_>,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    if bucket.name == MERGE_CONFLICTS {
        let count = bucket.changes.len();
        group_header_row(
            ui,
            ui.auto_id_with(("group_header", &bucket.root.id, bucket.name)),
            GroupHeader {
                label: &format!("{} ({count})", bucket.name),
                name: bucket.name,
                meta: None,
                count,
                // Conflicts have no collapse of their own: they are the rows
                // the user came here to resolve, so this is a static band
                // rather than a button that hides them.
                expanded: None,
                focused: false,
                lead: None,
            },
        );
    }
    let body = ui.indent(
        ui.id()
            .with(("group_section", &bucket.root.id, bucket.name)),
        |ui| {
            for c in &bucket.changes {
                change_row(ui, state, bucket.root, c, actions, order);
            }
        },
    );
    // The indented rect is the rows' own block, hung at their left edge.
    indent_guide(ui, body.response.rect);
}

/// Flat changes of the active Commit sub-tab in display order — the F7/Shift+F7
/// cross-file traversal list (spec R7). Unfiltered by the file filter:
/// navigation walks the real file list. Local Changes spans each repo's flat
/// list and its conflicts group; the placeholder tabs contribute nothing.
///
/// Each carries its own root, because the list does span every root: relative to
/// one root alone, navigation could never leave the first root's duplicate.
pub(crate) fn active_subtab_files(state: &AppState) -> Vec<ChangeRef> {
    let buckets = match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => change_buckets(state),
        CommitSubTab::Shelf | CommitSubTab::Stash => return Vec::new(),
    };
    buckets
        .iter()
        .flat_map(|b| {
            b.changes
                .iter()
                .map(|c| ChangeRef::new(b.root.id.clone(), c.path.clone()))
        })
        .collect()
}

/// Status glyph + semantic tint for a conflicted path's review row (Lucide
/// tokens, issue #7). Only the conflict row wears an icon: every other row
/// says its state in one letter, and a conflicted row is the one state a
/// letter alone would understate.
fn status_glyph(status: ChangeStatus) -> (Icon, Color32) {
    match status {
        ChangeStatus::Modified => (Icon::FILE, Palette::STATE_WARNING),
        ChangeStatus::Added => (Icon::FILE_PLUS, Palette::STATE_SUCCESS),
        ChangeStatus::Deleted => (Icon::FILE_MINUS, Palette::STATE_ERROR),
        ChangeStatus::Renamed => (Icon::ARROW_RIGHT_LEFT, Palette::BRAND),
        ChangeStatus::Copied => (Icon::FILES, Palette::STATE_SUCCESS),
        ChangeStatus::Unversioned => (Icon::PLUS_CIRCLE, Palette::INK_2),
        ChangeStatus::Ignored => (Icon::EYE_OFF, Palette::INK_3),
        ChangeStatus::Conflicted => (Icon::FILE_WARNING, Palette::STATE_ERROR),
    }
}

/// The change-kind badge letter: `N` new, `M` modified, `D` deleted, `C`
/// copied or conflicted, `R` renamed.
///
/// Derived from the whole `Change` rather than from its `ChangeStatus`,
/// because "new and modified at once" is not visible in the status alone —
/// git reports `A` for a file it has never seen even after its content
/// diverges from the index. One letter has to answer both facts, and modified
/// wins: a file that also has unstaged edits is a file being worked on, which
/// is the thing a developer scanning this list is looking for.
///
/// **Accepted loss**: a file staged for the first time and then edited again
/// reports `Modified`, so it reads `M` rather than `N`. Recovering the signal
/// would cost a git call and a cache per row to disambiguate two states the
/// letter deliberately collapses.
fn badge_letter(c: &Change) -> &'static str {
    match c.status {
        ChangeStatus::Conflicted | ChangeStatus::Copied => "C",
        ChangeStatus::Deleted => "D",
        ChangeStatus::Added | ChangeStatus::Unversioned if !c.unstaged => "N",
        _ => "M",
    }
}

/// Status-**letter** colour: the three-rung change-kind grammar — `N` added
/// green, `M` modified blue, `D` deleted grey — with renames on the brand,
/// ignored dimming to `INK_3` and conflicts staying error red. Every remaining
/// state maps onto a real semantic hue, so no row ever paints a fallback.
///
/// `D` is grey rather than the error red it used to be: a deleted file is a
/// fact about the base side, not work the developer has to answer for.
///
/// **The file *name* does not take this colour** (conformance issue 10). A
/// semantic hue is the answer to "what state is this file in", and the one
/// letter beside the name already answers it; a name wearing the hue too
/// meant every row in the list was a different colour, which is the opposite
/// of what the ink ramp is for — hierarchy by *step*, not by category. The
/// name reads at [`Palette::INK`] and the letter keeps its hue, so the state
/// is still one glance away and the list reads as one list.
fn status_color(c: &Change) -> Color32 {
    match c.status {
        ChangeStatus::Deleted => Palette::STATUS_DELETED,
        ChangeStatus::Renamed => Palette::BRAND,
        ChangeStatus::Ignored => Palette::INK_3,
        ChangeStatus::Conflicted => Palette::STATE_ERROR,
        _ => match badge_letter(c) {
            "N" => Palette::STATUS_ADDED,
            _ => Palette::STATUS_MODIFIED,
        },
    }
}

/// Row text split for the `[filename] … [dim location]` layout (issue 05):
/// `("app.rs", Some("src/"))` for `src/app.rs`; files at the repo root have
/// no location to paint (`None`). The location renders dim after the name —
/// it is the furniture around the answer, so it takes the muted step
/// (conformance issue 10) and never the name's own ink.
fn row_display(path: &Path) -> (String, Option<String>) {
    let name = path.display_name_lossy_or(&path.display().to_string());
    let location = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| format!("{}/", p.display()));
    (name, location)
}

/// The per-file checkbox's accessible label: what the click will do, then the
/// state it is acting on.
///
/// Both halves are load-bearing. `WidgetInfo::labeled` carries no checked flag
/// for this widget, so a screen reader would hear the same "stage this file" on
/// three boxes that mean three different things; and the verb is what makes the
/// label the query handle the suites address a row's box by. The
/// **repo-relative path**, not the bare file name: two rows in a multi-root
/// project can share a name.
fn stage_label(path: &Path, stage: StageState) -> String {
    let name = path.display();
    match stage {
        StageState::None => format!("Stage {name} (unstaged)"),
        StageState::Partial => format!("Stage all of {name} (partially staged)"),
        StageState::All => format!("Unstage {name} (staged)"),
    }
}

/// Paint the per-file checkbox glyph at `rect` in one of its three states:
/// the **quiet** checkbox, not a brand-filled one.
///
/// **Why quiet** (conformance issue 10, R1/R4). The brand token has exactly
/// three jobs in this app: the primary action's fill, the active tab's
/// underline, and a selected row's rail. A brand-filled checkbox in every
/// file row spent the loudest blue in the palette on the most-repeated control
/// in the window — and made the *selected* row's own accent rail compete with
/// the boxes in every other row for the user's attention. Brand is how the app
/// says "this is the one thing to do"; a checkbox says "this file is in your
/// index", which is a fact, not an instruction.
///
/// So the box is the raised-on-card surface with an ink border, and the
/// ticked state adds an `INK` tick: the same shape, one step further. The
/// hover answer is the border stepping up to `INK`, which is the one thing
/// about the box that responds to the pointer.
///
/// The middle state is a bar rather than a shrunken tick: a tick drawn short
/// enough to read as "half" is the same drawing as a tick drawn wrong. The bar
/// replaces the warning dot the partial state used to get.
fn paint_checkbox(ui: &Ui, rect: Rect, stage: StageState, resp: &egui::Response) {
    let radius = CornerRadius::same(crate::theme::CHIP_RADIUS);
    let border = if stage != StageState::None || resp.hovered() {
        Palette::INK
    } else {
        Palette::INK_2
    };
    // The raised-on-card rung rather than a bare `SURFACE_2`: a control on a
    // content-surface card has to step up from that card, and naming the rung
    // is what keeps the ladder rule (`BG < CONTENT_BG < SURFACE < SURFACE_2`)
    // true here rather than by coincidence.
    ui.painter()
        .rect_filled(rect, radius, Palette::RAISED_ON_CARD);
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    match stage {
        StageState::None => {}
        StageState::All => {
            let p1 = Pos2::new(rect.left() + 3.5, rect.center().y - 1.0);
            let p2 = Pos2::new(rect.left() + 6.0, rect.center().y + 2.0);
            let p3 = Pos2::new(rect.left() + 11.5, rect.center().y - 3.5);
            let check = Stroke::new(1.6, Palette::INK);
            ui.painter().line_segment([p1, p2], check);
            ui.painter().line_segment([p2, p3], check);
        }
        StageState::Partial => {
            let half = rect.center().y;
            ui.painter().line_segment(
                [
                    Pos2::new(rect.left() + 3.5, half),
                    Pos2::new(rect.right() - 3.5, half),
                ],
                Stroke::new(1.6, Palette::INK),
            );
        }
    }
}

// Fixed columns of one file row, expressed from the row's left edge. They are
// named constants because a row's text has to be measured — and its height
// settled — before the row rect is allocated.

/// Left padding before the checkbox glyph.
const ROW_PAD_X: f32 = 6.0;
/// Width of the per-file include checkbox.
const ROW_CHECKBOX: f32 = 16.0;
/// Gap between two row columns.
const ROW_GAP: f32 = 6.0;
/// Right padding of the row: the dim location column ends here.
const ROW_EDGE: f32 = 12.0;
/// Lead of the rename arrow (plus its gap) before the renamed-from path.
const RENAME_ARROW_W: f32 = 24.0;
/// Narrowest name column a row accepts before the dim location gives up its
/// first-line seat and moves onto its own line under the name.
const MIN_NAME_COLUMN: f32 = 96.0;

/// Leading offset of a group header's chevron, from the band's left edge.
const GROUP_CHEVRON_X: f32 = 14.0;
/// Leading offset of a group header's optional leading glyph (a repo folder).
const GROUP_ICON_X: f32 = 32.0;
/// Leading offset of a group header's name, from the band's left edge. One
/// column for every group kind, so a repo name, the conflicts name and the
/// unversioned name all start at the same x down the tree.
const GROUP_NAME_X: f32 = 46.0;
/// Gap between a group header's name and its metadata.
const GROUP_GAP: f32 = 8.0;
/// Inset of a group header's count chip from the band's trailing edge.
const GROUP_EDGE: f32 = 12.0;
/// Width reserved at a group header's trailing edge for the count chip, so the
/// chip is right-aligned into a zone rather than laid out after the name.
const GROUP_COUNT_SLOT: f32 = 44.0;

/// The gesture a row-body click is, from the modifier keys held while it
/// happened. Read here rather than inside [`changes_selection`], because that
/// module owns the rule and this is the one place egui is allowed to be part of
/// it.
fn row_click_kind(ui: &Ui) -> ClickKind {
    let modifiers = ui.ctx().input(|i| i.modifiers);
    changes_selection::click_kind(modifiers.shift, modifiers.command)
}

/// One file row (issue 05, design doc §4): `[checkbox] [status letter]
/// [filename] … [dim location]` on the 24 px file-row height — or taller: a
/// filename too long for its column **wraps onto continuation lines** and the
/// row grows one text line per extra row of text, so a long name is never
/// clipped at the pane edge or painted over the dim location.
///
/// The row is the shared row shell: the one row-state fill, and — for the
/// selected row — the 2px accent rail at its leading edge (conformance issue
/// 10). The rail is **paint over the row's own leading pad, never padding
/// beside it**: reserving the width would move every row's content the moment
/// it is selected, and the names would stop lining up down the column. The
/// checkbox therefore keeps the same x whether or not the row is selected.
///
/// The status letter says what happened to the file — N green / M blue / D
/// grey — while the filename reads at [`Palette::INK`] and the parent directory
/// renders dim at the pane edge. Clicking the checkbox stages or un-stages the
/// whole file; clicking the row body previews its diff and runs the selection
/// gesture, which [`changes_selection`] resolves from the modifier keys held at
/// the click: a plain click picks one row, Command/Ctrl toggles one in or out,
/// and Shift+Click replaces the set with the painted range from the anchor.
/// Neither gesture edits the index. Renames/copies keep a muted arrow plus the old
/// path; conflicts render as one review row through [`conflict_row`]. Interactions
/// go onto `actions` and this row's place in the painted list onto `order`, and
/// the caller reads both after rendering.
#[allow(clippy::too_many_lines)]
fn change_row(
    ui: &mut Ui,
    state: &AppState,
    root: &Root,
    c: &Change,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    let key = root.canonical_key(c);
    if c.status == ChangeStatus::Conflicted {
        conflict_row(ui, state, &root.id, c, &key, actions, order);
        return;
    }

    // Two independent facts about one row: which file the developer is
    // looking at (`selected`, which owns the highlight) and how much of it the
    // index holds (`stage`, which owns the checkbox). Browsing sets the first
    // and never touches the second.
    let selected = state.ui.selected.contains(&key);
    let stage = StageState::of(c);
    let tint = status_color(c);
    let (name, location) = row_display(&c.path);
    // The file-row name rides the shared body type (T2) — one size role for
    // filename/body text across tool windows, not a local fractional copy.
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional);
    let painter = ui.painter().clone();

    // ---- measure, then allocate --------------------------------------
    // The row's height is a function of its text, so everything is laid out
    // first: the name's wrap width depends on the fixed prefix columns and
    // on whether the dim location keeps its first-line seat.
    let letter_galley = painter.layout_no_wrap(badge_letter(c).to_owned(), font.clone(), tint);
    let (letter_w, letter_h) = letter_galley.size().into();
    let left = ui.cursor().left_top();
    let content_right = left.x + ui.available_width() - ROW_EDGE;
    // x of the name column: the row's left edge plus the fixed prefix — the
    // checkbox, the status letter, and one gap. The accent rail is *not* in
    // this sum: it is painted over the leading pad, so a chosen row's columns
    // are an unchosen row's columns. Nothing here is conditional, so every
    // row in the list names at the same x however its file is staged.
    let name_x = left.x + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP + letter_w + ROW_GAP;
    // The dim location keeps its own right-aligned column on the first line
    // as long as the name keeps a readable width; on deep paths in a narrow
    // pane it drops onto its own line beneath the name instead of squeezing
    // the name into a sliver.
    let loc_galley = location
        .as_ref()
        .map(|loc| painter.layout_no_wrap(loc.clone(), font.clone(), Palette::INK_3));
    let loc_w = loc_galley.as_ref().map_or(0.0, |g| g.size().x);
    let loc_h = loc_galley.as_ref().map_or(0.0, |g| g.size().y);
    let loc_gap = if loc_galley.is_some() { ROW_GAP } else { 0.0 };
    let full_column = content_right - name_x;
    let loc_below = loc_galley.is_some() && full_column - loc_w - loc_gap < MIN_NAME_COLUMN;
    let name_column = if loc_below {
        full_column
    } else {
        full_column - loc_w - loc_gap
    }
    .max(1.0);
    // A filename that does not fit its column wraps onto continuation lines
    // (breaking mid-word — file names rarely contain a space) rather than
    // overrunning the pane or the location column. The wrap is asserted from
    // the galley's *geometry* in `tests/commit_window.rs`, never from
    // `Galley::text()`, which returns the unwrapped input and so reads as
    // though nothing wrapped even when the row grew correctly.
    let name_galley = painter.layout(name.clone(), font.clone(), Palette::INK, name_column);

    // Renames/copies (spec R8) keep the muted arrow + old path: inline after
    // a single-line name that leaves room for them, else on their own line.
    let orig_galley = c
        .orig_path
        .as_ref()
        .map(|p| painter.layout_no_wrap(p.display().to_string(), font.clone(), Palette::INK_3));
    let rename_below = match (&orig_galley, name_galley.rows.len()) {
        (Some(orig), 1) => {
            name_x + name_galley.size().x + ROW_GAP + RENAME_ARROW_W + orig.size().x
                > name_x + name_column
        }
        (Some(_), _) => true,
        (None, _) => false,
    };

    // The row keeps the design's 24 px single-line height (design doc §8)
    // and grows exactly one text line per extra line of text.
    let line_h = name_galley
        .rows
        .first()
        .map_or(name_galley.size().y, |row| row.size.y);
    let name_h = name_galley.size().y;
    let name_lines = name_galley.rows.len().max(1);
    let below_lines = usize::from(rename_below) + usize::from(loc_below);
    let content_h = name_h + below_lines as f32 * line_h;
    let row_h = crate::theme::FILE_ROW_HEIGHT + (name_lines - 1 + below_lines) as f32 * line_h;
    let row = Rect::from_min_size(left, Vec2::new(ui.available_width(), row_h));
    ui.allocate_exact_size(row.size(), Sense::hover());

    // The shared row shell, and the only place in this list a fill is
    // chosen: the row-selected token for the selected row, the shared hover
    // fill under the pointer, and the 2px brand rail at the leading edge of
    // whichever row is selected. `RowState::from_flags` is the app's one
    // selected-or-hovered decision, so a file row cannot drift from the log
    // table, the sidebar tree or the rebase todo about what "selected" means.
    //
    // The band follows `selected`, never the checkbox: the highlight answers
    // "which file am I looking at", and the box answers "what is in the
    // index". Reading it off the box would put a staging action and a browse
    // in the same visual channel, which is the coupling this row no longer has.
    components::row_shell(
        ui,
        row,
        components::RowState::from_flags(selected, ui.rect_contains_pointer(row)),
        components::RowShell::Railed,
    );

    // The text block is centred in the row; the checkbox, the status letter
    // and the right-aligned location all ride the block's first line.
    let block_top = row.top() + (row_h - content_h) / 2.0;
    let cy = block_top + line_h / 2.0;

    // Checkbox: a direct control over the index. It reads the staged/unstaged
    // pair for this row and nothing else, so the box cannot tick itself, and
    // its click asks for the whole file — plain `git add` / `git restore
    // --staged`, never the granular pipeline's peek on the diff cache, which
    // makes a staging click a silent no-op on a cold slot.
    let cb = ROW_CHECKBOX;
    let cb_rect = Rect::from_min_max(
        Pos2::new(row.left() + ROW_PAD_X, cy - cb / 2.0),
        Pos2::new(row.left() + ROW_PAD_X + cb, cy + cb / 2.0),
    );
    let cb_resp = ui.interact(
        cb_rect,
        ui.auto_id_with(("row_checkbox", &key)),
        Sense::click(),
    );
    let cb_label = stage_label(&c.path, stage);
    cb_resp.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, cb_label.clone()));
    if cb_resp.clicked() {
        actions.push(RowAction::ToggleStage {
            root: root.id.clone(),
            path: c.path.clone(),
        });
    }
    paint_checkbox(ui, cb_rect, stage, &cb_resp);

    // Status letter, then the filename, then the dim location. The letter is
    // the row's state and keeps its semantic hue; the name is the row's
    // answer and reads at the primary ink (conformance issue 10), so no text
    // in this row changes ink with the row's selection.
    let letter_x = row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP;
    painter.galley_with_override_text_color(
        Pos2::new(letter_x, cy - letter_h / 2.0),
        letter_galley,
        tint,
    );

    painter.galley_with_override_text_color(
        Pos2::new(name_x, block_top),
        name_galley.clone(),
        Palette::INK,
    );

    // Lines below the name: the rename marker first, the dim location last.
    let rename_top = block_top + name_h;
    let loc_top = rename_top + if rename_below { line_h } else { 0.0 };
    if let Some(orig) = &orig_galley {
        let orig_h = orig.size().y;
        let (arrow_cy, orig_x, orig_y) = if rename_below {
            (
                rename_top + line_h / 2.0,
                name_x + RENAME_ARROW_W,
                rename_top + (line_h - orig_h) / 2.0,
            )
        } else {
            (
                cy,
                name_x + name_galley.size().x + ROW_GAP + RENAME_ARROW_W,
                cy - orig_h / 2.0,
            )
        };
        // Muted arrow then the renamed-from path, both dim (R8).
        icons::centered_icon(
            ui,
            Icon::ARROW_RIGHT,
            Pos2::new(orig_x - RENAME_ARROW_W / 2.0, arrow_cy),
            12.0,
            Palette::INK_3,
        );
        painter.galley_with_override_text_color(
            Pos2::new(orig_x, orig_y),
            orig.clone(),
            Palette::INK_3,
        );
    }

    // Dim location, right-aligned to the pane edge — on the first line while
    // the name keeps its column, otherwise under the name.
    if let Some(loc) = &loc_galley {
        let loc_y = if loc_below {
            loc_top + (line_h - loc_h) / 2.0
        } else {
            cy - loc_h / 2.0
        };
        painter.galley_with_override_text_color(
            Pos2::new(content_right - loc_w, loc_y),
            loc.clone(),
            Palette::INK_3,
        );
    }

    // Row body: one select gesture — the selection and the diff preview, and
    // nothing else. Which gesture it is comes from the keys held at the click.
    // Registered after the checkbox so the checkbox owns its own clicks. The body
    // rect starts *past* the checkbox, so neither widget can steal the other's
    // clicks and the register order is belt-and-braces.
    let body_rect = Rect::from_min_max(
        Pos2::new(row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP, row.top()),
        row.max,
    );
    let body = ui.interact(
        body_rect,
        ui.auto_id_with(("row_body", &key)),
        Sense::click(),
    );
    body.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    order.push(RowSlot {
        key: key.clone(),
        selectable: true,
    });
    if body.clicked() {
        actions.push(RowAction::Select {
            key,
            root: root.id.clone(),
            path: c.path.clone(),
            kind: row_click_kind(ui),
        });
    }
}

/// One conflicted path's review row.
///
/// A conflicted file is a **file row in every respect the row vocabulary
/// speaks of** (conformance issue 10): the same shared row shell, the same
/// leading-edge rail marking the previewed row, the same ink ramp. The one
/// thing it does not have is the checkbox, because an unmerged path cannot be
/// staged or included until it is resolved — a box that could be checked and
/// meant nothing would be worse than no box. The `C` badge beside the path
/// carries the state, in the same semantic hue every other status letter
/// wears.
///
/// Clicking its body previews its diff like any other row and puts nothing in
/// `AppState::ui.selected`. This is the row whose [`RowSlot`] reports itself
/// unselectable: it holds a place in a shift range, so a drag that passes over
/// the conflicts group still spans the files below it, and it never joins the set
/// the destructive verbs read.
///
/// It used to be a `ui.horizontal` of a `selectable_label` and an icon: the
/// stock selection fill plus a 1px brand stroke around one row in a list whose
/// other rows were doing something else. That is the loud, off-vocabulary
/// selection this list no longer has anywhere.
fn conflict_row(
    ui: &mut Ui,
    state: &AppState,
    root: &RootId,
    c: &Change,
    key: &PathBuf,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    // The rail marks the previewed *change*, so two roots holding the same
    // relative path light up one row, not both.
    let previewing =
        state.ui.preview_change.as_ref() == Some(&ChangeRef::new(root.clone(), c.path.clone()));
    let (glyph, glyph_tint) = status_glyph(c.status);
    let label = format!("{} {}", badge_letter(c), c.path.display());
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional);
    let painter = ui.painter().clone();

    let left = ui.cursor().left_top();
    let row = Rect::from_min_size(
        left,
        Vec2::new(ui.available_width(), crate::theme::FILE_ROW_HEIGHT),
    );
    ui.allocate_exact_size(row.size(), Sense::hover());

    // The same shared shell as every other file row, and the same rail for
    // the previewed row: this list has one selection vocabulary, not two.
    components::row_shell(
        ui,
        row,
        components::RowState::from_flags(previewing, ui.rect_contains_pointer(row)),
        components::RowShell::Railed,
    );

    // The badge letter and the path take the columns every other row uses, so
    // the list reads as one column of names down the pane. The checkbox slot is
    // left empty rather than painted over: an unmerged path cannot be staged,
    // and a box that could be ticked and meant nothing would be worse than no
    // box. The gap is reserved at the same `ROW_CHECKBOX` width so the name
    // lands where every other row's name lands.
    let cy = row.center().y;
    let letter_x = row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP;
    let letter_galley =
        painter.layout_no_wrap(badge_letter(c).to_owned(), font.clone(), glyph_tint);
    let (letter_w, letter_h) = letter_galley.size().into();
    painter.galley_with_override_text_color(
        Pos2::new(letter_x, cy - letter_h / 2.0),
        letter_galley,
        glyph_tint,
    );
    let galley = painter.layout_no_wrap(c.path.display().to_string(), font.clone(), Palette::INK);
    painter.galley_with_override_text_color(
        Pos2::new(letter_x + letter_w + ROW_GAP, cy - galley.size().y / 2.0),
        galley,
        Palette::INK,
    );
    icons::centered_icon(
        ui,
        glyph,
        Pos2::new(row.right() - ROW_EDGE, cy),
        14.0,
        glyph_tint,
    );

    let response = ui.interact(row, ui.auto_id_with(("conflict_row", key)), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.clone()));
    order.push(RowSlot {
        key: key.clone(),
        selectable: false,
    });
    if response.clicked() {
        actions.push(RowAction::Select {
            key: key.clone(),
            root: root.clone(),
            path: c.path.clone(),
            kind: row_click_kind(ui),
        });
    }
}

/// The card's recent-message shortcuts, when there are any.
///
/// Not one of the card's three **controls** — a past message is a different
/// thing from a commit input, and it is empty until the first commit, so the
/// resting card really is well + meta row + Commit. It keeps its own row for
/// the same reason it always did: six 22-character buttons cannot share a
/// 316-pt row with the counter, Amend, Template and Clear without one of them
/// losing its label.
///
/// The buttons wear the same [`meta_button`] as the meta row. They used to be
/// raw `ui.button`s, which painted egui's stock grey on the card and read as a
/// fourth visual vocabulary inside one region.
fn recent_messages_row(ui: &mut Ui, state: &mut AppState) {
    if state.ui.recent_messages.is_empty() {
        return;
    }
    // Collected first: the click arm needs `&mut state`, the list borrows it.
    let messages: Vec<String> = state.ui.recent_messages.iter().take(6).cloned().collect();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Recent:").color(Palette::INK_3));
        for m in messages {
            let short = if m.chars().count() > 22 {
                m.chars().take(22).collect::<String>()
            } else {
                m.clone()
            };
            if meta_button(ui, &short).clicked() {
                state.ui.commit_message = m;
            }
        }
    });
}

/// The pane title the shared header prints, in the app's one case.
///
/// The word, not the case rule: [`widgets::pane_header`] prints what it is
/// given, so the case is this caller's to get right — and the changes card is a
/// pane like the worktrees and submodules panes, so it says itself in upper.
const CHANGES_PANE_TITLE: &str = "CHANGES";

/// The two changes-header controls that cannot do anything yet, and why.
///
/// ADR-0010's rule is that a flow which has not shipped stays *reachable* and
/// stays honestly inert, and this codebase's rendering of "reachable but cannot
/// act" is a **disabled menu row**: painted, dimmed to 45 % ink, `is_disabled()`
/// in the accessibility tree, carrying its reason on hover. So both entries here
/// are `enabled: false` and neither has a `clicked` arm anywhere near it —
/// pressing one dispatches no operation, opens no dialog, and calls no git.
///
/// That is the whole point of the shape: an enabled-looking control that cannot
/// do anything *and cannot be read* is noise, and the previous version of this
/// header was five of them. The scope gap is recorded in this table rather than
/// hidden, and it is the same gap the spec keeps out of scope.
const INERT_HEADER_ACTIONS: [(Icon, &str, &str); 2] = [
    (
        Icon::SETTINGS,
        "Commit options",
        "No commit-options surface yet (ADR-0010)",
    ),
    (
        Icon::LAYERS,
        "Group by",
        "No grouping surface yet (ADR-0010)",
    ),
];

/// The changes card's header strip: the shared pane header (R7) — a 9px tracked
/// title, the *file* count as a count chip beside it, a right-aligned action
/// slot — then one hairline, then content.
///
/// **Three controls, and the trigger that opens the fourth thing.** The action
/// slot holds the three that do something: refresh, rollback (through the
/// destructive confirm), and the expand/collapse-all chevron. The two that
/// cannot — commit options and group by — are in
/// [`changes_header_overflow`], reachable and inert. The `⋯` that opens that
/// menu is the one non-control in the band: an affordance for the two rows that
/// left it, not a fourth thing this pane can do.
///
/// Removing the icons is also what un-squeezes the filter below, which now has
/// a row of its own under the hairline at the card's full inner width. The
/// old header recorded the arithmetic itself — a title plus five icons left the
/// filter about 24px on the title's row — so the fix is removing the icons, not
/// widening the column. [`COMMIT_PANEL_WIDTH`] is unchanged.
///
/// The count is a **file** count: how many changes the list holds, excluding
/// ignored entries. It is not a line count and not a diffstat total. The
/// `+N −M` pair is a different number — it describes one selected file, not the
/// list — and it rides the preview card, in its summary strip and on that
/// file's own row.
fn changes_card_header(ui: &mut Ui, state: &mut AppState) {
    let total = state
        .multi
        .roots
        .iter()
        .flat_map(|r| &r.status.changes)
        .filter(|c| !matches!(c.status, ChangeStatus::Ignored))
        .count();
    let count = total.to_string();

    widgets::pane_header(ui, CHANGES_PANE_TITLE, Some(&count), |ui| {
        // The slot is `Layout::right_to_left`, so the first control added is the
        // rightmost. The `⋯` leads so the three live controls keep the reading
        // order they had in the five-icon row: refresh, rollback, chevron.
        let overflow = widgets::icon_button(ui, Icon::MORE_HORIZONTAL);
        overflow.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, "Changes header overflow")
        });
        changes_header_overflow(&overflow);

        // Refresh: the full scoped refresh (same as the shell button).
        let refresh = widgets::icon_button(ui, Icon::REFRESH_CW);
        refresh.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Refresh changes"));
        if refresh.clicked() {
            state.refresh(Affected::All);
        }

        // Rollback (discard): destructive, confirmation-gated.
        let rollback = widgets::icon_button(ui, Icon::UNDO);
        rollback.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Rollback"));
        if rollback.clicked() {
            let ch = selected_changes(state);
            if ch.is_empty() {
                state.ui.toast = Some(Toast::warning("Select files to discard."));
            } else {
                state.ui.confirm = Some(PendingConfirm::Discard { changes: ch });
            }
        }

        // Expand / collapse all non-focused groups; the focused repo stays
        // expanded either way ("focus = expand").
        let all_expanded = state
            .multi
            .roots
            .iter()
            .filter(|r| state.selected_root.as_ref() != Some(&r.id))
            .all(|r| state.ui.changes_expanded.contains(&r.id));
        let (icon, label) = if all_expanded {
            (Icon::CHEVRON_UP, "Collapse all groups")
        } else {
            (Icon::CHEVRON_DOWN, "Expand all groups")
        };
        let expand_all = widgets::icon_button(ui, icon);
        expand_all.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
        if expand_all.clicked() {
            if all_expanded {
                state.ui.changes_expanded.clear();
            } else {
                for r in &state.multi.roots {
                    if state.selected_root.as_ref() != Some(&r.id) {
                        state.ui.changes_expanded.insert(r.id.clone());
                    }
                }
            }
        }
    });

    // The filter's own full row, under the header's hairline: it is the list's
    // own control, so it is the list's first content row rather than part of
    // the heading.
    file_filter_row(ui, state);
}

/// The changes header's overflow menu (ADR-0010): the two controls that cannot
/// act, as two **visibly inert** rows — rendered at the shared 45 %-ink
/// disabled state, `is_disabled()` in the accessibility tree, and carrying
/// [`INERT_HEADER_ACTIONS`]' reason on hover.
///
/// Every row is gated, so the menu has no arm that could dispatch: pressing
/// either item changes no state at all, which is the stronger claim than "it
/// is wired to a no-op" because there is nothing to wire.
///
/// The 1px stroke the menu frame keeps is R2 saying *this floats*, not a missed
/// rule — the only surfaces allowed a hairline are the ones that do.
fn changes_header_overflow(trigger: &egui::Response) {
    use egui::Popup;
    Popup::menu(trigger).show(|menu| {
        // Menu rows sit flush, exactly as the branch menu composes them, and
        // the width is pinned rather than left to the popup's own available
        // width — a menu that stretches to the window is a menu whose rows are
        // mostly air.
        menu.spacing_mut().item_spacing.y = 0.0;
        menu.set_min_width(200.0);
        menu.set_max_width(224.0);
        for (icon, label, reason) in INERT_HEADER_ACTIONS {
            widgets::menu_item(
                menu,
                widgets::MenuItemProps {
                    icon,
                    label,
                    data: None,
                    shortcut: None,
                    state: widgets::MenuItemState::disabled(reason),
                    kind: widgets::MenuItemKind::Default,
                },
            );
        }
    });
}

// --------------------------------------------- preview + editor pane ------

fn preview_and_editor_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::card(ui, widgets::CardFrame::default(), |ui| {
        let preview = state.ui.preview_change.clone();
        preview_header(ui, state, preview.as_ref());
        match preview {
            Some(change) => {
                let root = change.root.clone();
                crate::ui::diff::render_diff(ui, state, &root, &None, &None, &Some(change));
            }
            None => {
                ui.colored_label(
                    Color32::GRAY,
                    "Select a changed file to preview its unified diff.",
                );
            }
        }
    });
}

/// The preview card's header strip: the previewed file's path, vertically
/// centred, with the prev/next change navigation right-aligned. The nav
/// buttons retarget the preview to the adjacent changed file — the same path a
/// file-row click uses, so the diff read loads the new diff. The mode, the
/// counts and the comparison controls belong to the diff toolbar the body
/// renders; the header does not restate them. With nothing previewed there is
/// no file to name, so the strip is not painted at all.
fn preview_header(ui: &mut Ui, state: &mut AppState, change: Option<&ChangeRef>) {
    let Some(change) = change else {
        return;
    };
    widgets::card_header(ui, |ui| {
        let width = ui.available_width();
        // A fixed-height centred band, not a plain horizontal: a one-pass
        // layout centres a first, shorter item against the row height it
        // *starts* with, so the path hung high over the 28 pt chevrons that
        // grew the row after it.
        ui.allocate_ui_with_layout(
            Vec2::new(width, widgets::ICON_BUTTON_SIZE),
            Layout::left_to_right(Align::Center),
            |ui| {
                let path_text = change.path.display().to_string();
                let path_resp = ui.label(
                    RichText::new(&path_text)
                        .font(crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE))
                        .color(Palette::INK),
                );
                path_resp.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Label, true, format!("Previewing {path_text}"))
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    change_nav(ui, state, change);
                });
            },
        );
    });
}

/// Prev/next change navigation over the active sub-tab's changed-file list,
/// right-aligned to the card's edge.
fn change_nav(ui: &mut Ui, state: &mut AppState, change: &ChangeRef) {
    let files = active_subtab_files(state);
    let current = files.iter().position(|c| c == change);
    let prev_enabled = current.is_some_and(|i| i > 0);
    let next_enabled = current.is_some_and(|i| i + 1 < files.len());
    // Right-to-left: the Next chevron is allocated first so it sits at the
    // card edge, with Previous to its left.
    let next = widgets::icon_button_enabled(ui, Icon::CHEVRON_RIGHT, next_enabled);
    next.widget_info(|| WidgetInfo::labeled(WidgetType::Button, next_enabled, "Next change"));
    if next_enabled && next.clicked() {
        state.ui.preview_change = current.and_then(|i| files.get(i + 1)).cloned();
    }
    let prev = widgets::icon_button_enabled(ui, Icon::CHEVRON_LEFT, prev_enabled);
    prev.widget_info(|| WidgetInfo::labeled(WidgetType::Button, prev_enabled, "Previous change"));
    if prev_enabled && prev.clicked() {
        state.ui.preview_change = current.and_then(|i| files.get(i - 1)).cloned();
    }
}

// ------------------------------------------------- the commit card itself ---

/// The card's heading. It stays a **heading**, not a fourth control: it is the
/// message well's accessible name, and three regression claims in
/// `tests/commit_window.rs` are anchored to the string. The card's *controls*
/// are three — the well, the one meta row, the one Commit control.
const MESSAGE_HEADING: &str = "Commit message:";

/// The message well's accessible name, and the placeholder it lays down while
/// the message is empty.
const MESSAGE_WELL_LABEL: &str = "Commit message";
const MESSAGE_WELL_HINT: &str = "Summary (required)\n\nDescription";

/// Body lines the message well shows at rest.
///
/// Four is the number that makes the well *about* [`MESSAGE_WELL_HEIGHT`]
/// without being clipped: four body lines plus the well's own 8 pt of padding
/// top and bottom.
const MESSAGE_WELL_ROWS: usize = 4;

/// The well's own padding, on every side, in points.
///
/// A text well is the one place a fixed inner margin reads as right: the text
/// needs air above its first line and below its last, and 8 pt is the card's
/// own [`crate::theme::PANEL_PADDING`] so the well does not look inset from
/// the region it fills.
const MESSAGE_WELL_PAD: i8 = 8;

/// **The commit card's message well, at about 76 pt tall.**
///
/// The figure is the ticket's budget, and it is a budget rather than a
/// preference: the well shares the fixed [`COMMIT_PANEL_WIDTH`] column with the
/// file list below it, and the whole point of *not* widening the column was that
/// the well and the list could both be usable at the width the column already
/// has.
///
/// It is the same four body lines the old well showed, plus the 8 pt of padding
/// the well now carries on its own sides — the old one had none, and left its
/// text against egui's stock `extreme_bg_color` frame, which is why it measured
/// 64 pt. Four lines is a real subject plus the opening of a real body.
///
/// It is a **resting** height, not a cap. A longer message grows the well,
/// because a well that silently clipped the body of a commit message would be
/// a worse defect than a card that grows; `tests/commit_window.rs` measures the
/// resting height *and* the file list's remaining height together, so the
/// growth is visible rather than silent.
pub const MESSAGE_WELL_HEIGHT: f32 = 76.0;

/// Git's own bound on a commit subject, and the bound this card's counter
/// tracks. Named so the counter's `/50` and its over-length test read the same
/// number.
const SUBJECT_LIMIT: usize = 50;

/// The guidance shown beside the counter once the subject is over the bound.
const SUBJECT_GUIDANCE: &str = "(keep \u{2264} 50)";

/// Horizontal padding inside a meta-row button.
const META_BUTTON_PAD_X: f32 = 8.0;

/// Height of the card's one Commit control — the shared primary's own 32 pt
/// (`.tg-btn-primary`), not the 28 pt compact. The commit card is a region,
/// not a pane header, so nothing forces it down to a band height, and taking
/// the shared primary's height means the brand button keeps the geometry the
/// whole app's brand buttons have. The small ghost actions beside it are the
/// same 32 pt (`widgets::ghost_button`), so the action row is one height.
const COMMIT_CONTROL_H: f32 = 32.0;

/// Width of the Commit control's chevron half — the shared icon button's own
/// square, so the glyph has the same box it has everywhere else.
const COMMIT_CHEVRON_W: f32 = 28.0;

/// The rule between the two halves of the Commit control: one hairline, and
/// the only separation. `RAIL_WIDTH` is not the right constant here (this is a
/// divider on a fill, not a state rail on a row) and `widgets::BORDER_HAIRLINE_WIDTH`
/// is not exported, so the shared "1 px" reading is spelled once, here.
const COMMIT_RULE_W: f32 = 1.0;

/// Label of the Commit control's committing half.
const COMMIT_LABEL: &str = "Commit";

/// The two halves of the one Commit control, as the two hit regions inside it.
///
/// Two `Response`s and one painted button: the halves are addressable so the
/// menu can open from the chevron and so each half takes its own hover state,
/// but there is no gap, no second fill, and no second border between them.
struct CommitHalves {
    /// The committing half. Accessible name `Commit changes`.
    label: egui::Response,
    /// The chevron half, which opens the alternatives menu. Accessible name
    /// `Commit split options`.
    chevron: egui::Response,
}

/// The commit card's **two of its three parts**: the message well and the one
/// meta row. The third — the Commit control, in [`commit_action_row`] — is
/// rendered by the card's caller, after the footer rule, so "commit" reads as a
/// separate act from "write the message".
///
/// `Commit message:` remains the card's heading and the well's accessible name.
/// It is not one of the three *controls* — the card had six of those, and it has
/// three now: the well, the meta row (counter, Amend, Template, Clear) and the
/// Commit control.
fn commit_message_box(ui: &mut Ui, state: &mut AppState) {
    ui.label(RichText::new(MESSAGE_HEADING).color(Palette::INK_2));
    message_well(ui, &mut state.ui.commit_message);
    commit_meta_row(ui, state);
}

/// The message well: the card's one text surface, on the **raised-on-card**
/// fill.
///
/// The commit card is a content-surface card ([`Palette::CONTENT_BG`]), so a
/// control sitting on it has to step up to the *second* rung of the raised
/// ladder — [`Palette::RAISED_ON_CARD`], `#313438` — and not the first
/// ([`Palette::RAISED`], `#2B2D30`), which is the rung for controls on the app
/// or a panel background. `#313438` on `#232529` is a visible step; `#2B2D30` on
/// `#232529` is 1.1:1, which is to say it is not one. The well also does not get
/// the card's own `CONTENT_BG`: a well is a control, and a control that shares
/// its host's fill has no edge at all.
///
/// The frame is painted here rather than left to egui's `text_edit_multiline`,
/// whose stock frame took `extreme_bg_color` — a colour that has no name in the
/// palette and that the raised ladder therefore cannot govern. The text ink is
/// named for the same reason.
fn message_well(ui: &mut Ui, buf: &mut String) -> egui::Response {
    let out = egui::Frame::new()
        .fill(Palette::RAISED_ON_CARD)
        .corner_radius(CornerRadius::same(crate::theme::CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(MESSAGE_WELL_PAD, MESSAGE_WELL_PAD))
        .show(ui, |ui| {
            let resp = egui::TextEdit::multiline(buf)
                .hint_text(MESSAGE_WELL_HINT)
                .desired_width(f32::INFINITY)
                .desired_rows(MESSAGE_WELL_ROWS)
                .text_color(Palette::INK)
                .frame(egui::Frame::new())
                .show(ui);
            resp.response.widget_info(|| {
                WidgetInfo::labeled(WidgetType::TextEdit, true, MESSAGE_WELL_LABEL)
            });
            resp.response
        });
    // The one brand mark a non-primary control may wear while it holds focus —
    // the shared focus ring, and a *stroke*, so the brand-fill scan over the
    // card is unaffected by it.
    if out.inner.has_focus() {
        ui.painter().rect_stroke(
            out.response.rect.expand(1.0),
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
    out.response
}

/// **The card's one meta row: the subject counter, Amend, Template and Clear —
/// all four on one line, inside the column the ticket refused to widen.**
///
/// This is the change the whole card is named for. The counter and the three
/// actions used to be two rows, and the second row was the width the header had
/// been borrowing: the commit column never had room for the meta row it
/// actually needed, so the *fix* was to make the row smaller, not the column
/// wider. [`COMMIT_PANEL_WIDTH`] is unchanged, the well beside it is
/// [`MESSAGE_WELL_HEIGHT`], and this row fits the 316 pt that leaves — the
/// counter and its over-length guidance included, which is the load the old
/// layout spread over two lines.
///
/// The row is laid out against an allocated rect rather than through
/// `ui.horizontal` because every item on it has to be *centred on one line*:
/// egui's `horizontal` cross-aligns to the top, which would hang the counter's
/// text against the top edge of two 28 pt buttons beside it.
///
/// Left to right the row reads **counter | Amend | Template | Clear** — the
/// counter is the only thing that is not a control, so it leads; the three
/// actions are right-aligned as one group, so the row's leading edge is the
/// card's leading edge and nothing wanders into the middle of it.
fn commit_meta_row(ui: &mut Ui, state: &mut AppState) {
    let row_h = components::KIT_BUTTON_H;
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
    let painter = ui.painter().clone();
    let font = crate::theme::chrome_font(crate::theme::TYPE_CONTROL);
    let cy = row.center().y;

    // The counter. Its own ink is the card's muted step, and it *becomes* the
    // warning step the moment the subject is over git's own bound — so the
    // number and its guidance cannot disagree about whether there is a problem.
    let subject_len = state
        .ui
        .commit_message
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .count();
    let over = subject_len > SUBJECT_LIMIT;
    let counter_ink = if over {
        Palette::STATE_WARNING
    } else {
        Palette::INK_3
    };
    let counter = painter.layout_no_wrap(
        format!("Subject: {subject_len}/{SUBJECT_LIMIT}"),
        font.clone(),
        counter_ink,
    );
    let mut x = row.left();
    painter.galley_with_override_text_color(
        Pos2::new(x, cy - counter.size().y / 2.0),
        counter.clone(),
        counter_ink,
    );
    x += counter.size().x;
    if over {
        let guidance =
            painter.layout_no_wrap(SUBJECT_GUIDANCE.to_owned(), font.clone(), counter_ink);
        painter.galley_with_override_text_color(
            Pos2::new(x + ROW_GAP, cy - guidance.size().y / 2.0),
            guidance,
            counter_ink,
        );
    }

    // Amend / Template / Clear, right-aligned as one group.
    let clear_w = meta_button_width(ui, "Clear");
    let template_w = meta_button_width(ui, "Template");
    let amend_label = painter.layout_no_wrap(AMEND_LABEL.to_owned(), font, Palette::INK_2);
    let amend_w = ROW_CHECKBOX + ROW_GAP + amend_label.size().x;

    let mut right = row.right();
    let clear = meta_button_at(
        ui,
        Rect::from_min_max(
            Pos2::new(right - clear_w, row.top()),
            Pos2::new(right, row.bottom()),
        ),
        "Clear",
    );
    if clear.clicked() {
        state.ui.commit_message.clear();
    }

    right -= clear_w + ROW_GAP;
    let template = meta_button_at(
        ui,
        Rect::from_min_max(
            Pos2::new(right - template_w, row.top()),
            Pos2::new(right, row.bottom()),
        ),
        "Template",
    );
    if template.clicked() {
        apply_commit_template(state);
    }

    right -= template_w + ROW_GAP;
    amend_checkbox_at(
        ui,
        Rect::from_min_max(
            Pos2::new(right - amend_w, row.top()),
            Pos2::new(right, row.bottom()),
        ),
        &mut state.ui.amend,
    );
}

/// Put the configured commit template into the well, or say why it could not.
///
/// Split out of the meta row so the row's layout reads as layout and this
/// reads as the Template action's behaviour.
fn apply_commit_template(state: &mut AppState) {
    let tpl = state.settings.commit_template.clone();
    if tpl.is_empty() {
        state.ui.toast = Some(Toast::warning("No commit template configured."));
    } else if let Ok(content) = std::fs::read_to_string(&tpl) {
        state.ui.commit_message = content;
    } else {
        state.ui.toast = Some(Toast::error(format!("Could not read template: {tpl}")));
    }
}

/// The meta row's Amend control, at an already-allocated rect.
///
/// The whole control — box *and* label — is the hit region, and it paints
/// through [`paint_checkbox`], the very painter the file rows use. That is the
/// point: the commit card's checkbox and the changes list's checkboxes are one
/// checkbox, on the same card, on the same raised rung.
fn amend_checkbox_at(ui: &mut Ui, rect: Rect, amend: &mut bool) -> egui::Response {
    let painter = ui.painter().clone();
    let galley = painter.layout_no_wrap(
        AMEND_LABEL.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Palette::INK_2,
    );
    let box_rect = Rect::from_center_size(
        Pos2::new(rect.left() + ROW_CHECKBOX / 2.0, rect.center().y),
        Vec2::splat(ROW_CHECKBOX),
    );
    let response = ui.interact(rect, ui.auto_id_with("commit_amend"), Sense::click());
    let mut checked = *amend;
    if response.clicked() {
        checked = !checked;
        *amend = checked;
    }
    paint_checkbox(
        ui,
        box_rect,
        if checked {
            StageState::All
        } else {
            StageState::None
        },
        &response,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            box_rect.right() + ROW_GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        Palette::INK_2,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, AMEND_LABEL));
    widgets::focus_ring(ui, &response);
    response
}

/// Label of the meta row's Amend checkbox.
const AMEND_LABEL: &str = "Amend";

/// Width [`meta_button`] gives `label`: its horizontal padding either side plus
/// the measured text. Measured, never estimated — the meta row places three of
/// these from the row's trailing edge, so a wrong width overlaps the counter.
fn meta_button_width(ui: &Ui, label: &str) -> f32 {
    2.0 * META_BUTTON_PAD_X
        + ui.painter()
            .layout_no_wrap(
                label.to_owned(),
                crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
                Color32::WHITE,
            )
            .size()
            .x
}

/// One meta-row button at an already-allocated `rect`: 28 pt, the
/// **raised-on-card** rung, [`Palette::INK_2`] at rest stepping to
/// [`Palette::INK`] when engaged.
///
/// The shared `components::kit_button` is *not* used here, and the reason is
/// the whole point of the card: its `KitButton::Secondary` fill is
/// [`Palette::RAISED`], the first rung — the right fill for a control on the app
/// background or a panel, and the wrong one on a content-surface card, where it
/// measures 1.1:1 against the card and disappears. Widening the shared kit
/// button to a "which rung am I on" flag is a design-system change in a file
/// this ticket does not own, so the card's own compact raised control is
/// painted here, and pinned by the raised-ladder ratchet in
/// `tests/commit_window.rs`.
///
/// [`Palette::INK_2`] rather than the muted [`Palette::INK_3`] for the same
/// reason: this is a raised fill, and the muted step is not legal on one.
fn meta_button_at(ui: &mut Ui, rect: Rect, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Color32::WHITE,
    );
    let response = ui.interact(
        rect,
        ui.auto_id_with(("meta_button", label)),
        Sense::click(),
    );
    let state = widgets::WidgetState::from_response(true, &response);
    let painter = ui.painter().clone();
    let fill = match state {
        widgets::WidgetState::Idle => Palette::RAISED_ON_CARD,
        widgets::WidgetState::Hovered => {
            widgets::mix(Palette::RAISED_ON_CARD, Color32::WHITE, 0.10)
        }
        _ => widgets::mix(Palette::RAISED_ON_CARD, Color32::WHITE, 0.20),
    };
    painter.rect_filled(rect, CornerRadius::same(crate::theme::CONTROL_RADIUS), fill);
    let ink = match state {
        widgets::WidgetState::Idle => Palette::INK_2,
        _ => Palette::INK,
    };
    painter.galley_with_override_text_color(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    widgets::focus_ring(ui, &response);
    response
}

/// [`meta_button_at`] at the layout cursor, for a row that lays itself out.
fn meta_button(ui: &mut Ui, label: &str) -> egui::Response {
    let width = meta_button_width(ui, label);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, components::KIT_BUTTON_H), Sense::hover());
    meta_button_at(ui, rect, label)
}

/// One action row of commit controls (issue 07): the one Commit control plus
/// the two small secondary actions beside it — one primary action only. The old
/// `Commit` / `Commit and Push...` / cascade button queue collapsed into the
/// Commit control's chevron half (see [`commit_split_menu`]).
///
/// The two secondary actions are shared 32 pt ghosts rather than egui's stock
/// grey buttons, so the action row is one height and the brand fill on it
/// belongs to the Commit control alone — a card with two filled greys and a
/// filled blue reads as three actions of equal weight.
fn commit_action_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // The two gates are independent: an empty *message* is never
        // committable, and an empty *index* is not either. The index is what
        // the commit reads, so "something is selected in the file list" is not
        // a gate at all any more.
        let can_commit = !state.ui.commit_message.trim().is_empty() && has_staged_changes(state);
        commit_control(ui, state, can_commit);
        ui.add_space(10.0);
        if widgets::ghost_button(ui, None, "Shelve\u{2026}")
            .on_hover_text("Shelve selected changes")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Shelve);
        }
        if widgets::ghost_button(ui, None, "Stash\u{2026}")
            .on_hover_text("Stash all changes")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Stash);
        }
    });
}

/// **The one Commit control: a content-width split button with the chevron
/// inside it.**
///
/// The previous version was a brand `Commit` button with a *separate* chevron
/// button beside it, which is exactly why it read as two carets glued together
/// with a gap. This is one painted button: two fills that meet across a single
/// 1 px rule, no gap, and one brand run. What the chevron half opens is
/// unchanged — the alternatives menu still opens from the chevron.
///
/// - **Content-width.** The width is its own label's width plus the chevron's
///   own square, and nothing stretches it. The card's inner row is 316 pt and
///   this control is roughly a quarter of it; a full-width brand bar would be a
///   different control, not a bigger Commit.
/// - **The rule is on the button, not around it.** One hairline between the
///   halves, and no stroke anywhere near the button's own edge — a bordered
///   button is an R2 failure.
/// - **The halves are two hit regions inside one control.** That is what makes
///   "the menu opens from the chevron" addressable, and what lets each half take
///   its own hover state; it is not two buttons, because there is no gap, no
///   second border and no second edge to read as a seam.
/// - **The fill and ink decisions are the shared primary's**, through
///   [`widgets::ButtonVariant::Primary`]; only the geometry is local, because
///   the shared primary has no split variant.
///
/// The one Commit control, or why it is not one right now.
///
/// A refused control reports its reason on hover. It has to go through
/// `on_disabled_hover_text` rather than `on_hover_text`: a `Ui` that is
/// disabled reports no hover for the widgets inside it, so the ordinary call
/// would be exactly the one control in this window that cannot explain itself.
fn commit_control(ui: &mut Ui, state: &mut AppState, can_commit: bool) {
    let mut halves = widgets::disabled_child_scope(ui, can_commit, commit_control_paint);
    if !can_commit {
        halves.label = halves
            .label
            .on_disabled_hover_text(commit_blocked_reason(state));
    }
    if can_commit && halves.label.clicked() {
        do_commit(state, false);
    }
    if can_commit {
        commit_split_menu(&halves.chevron, state);
    }
}

/// Why the Commit control is refusing. A disabled button that cannot say why
/// reads as broken, and this one refuses for two independent reasons a
/// developer can fix from two different places.
fn commit_blocked_reason(state: &AppState) -> &'static str {
    if state.ui.commit_message.trim().is_empty() {
        "Enter a commit message to commit"
    } else {
        "Stage at least one file — Commit commits the index and nothing else"
    }
}

/// Paint the Commit control's one button and return its two hit regions.
fn commit_control_paint(ui: &mut Ui) -> CommitHalves {
    let enabled = ui.is_enabled();
    let pad_x = crate::theme::BUTTON_PADDING.x;
    let galley = ui.painter().layout_no_wrap(
        COMMIT_LABEL.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Color32::WHITE,
    );
    let label_w = 2.0 * pad_x + galley.size().x;
    let (button, _) = ui.allocate_exact_size(
        Vec2::new(label_w + COMMIT_CHEVRON_W, COMMIT_CONTROL_H),
        Sense::hover(),
    );

    // The rule owns the one pixel between the two fills, so the fills are
    // adjacent to it and to nothing else.
    let rule_x = button.left() + label_w;
    let left = Rect::from_min_max(button.min, Pos2::new(rule_x, button.max.y));
    let right = Rect::from_min_max(Pos2::new(rule_x + COMMIT_RULE_W, button.min.y), button.max);

    let label = ui.interact(left, ui.auto_id_with("commit_label_half"), Sense::click());
    let chevron = ui.interact(
        right,
        ui.auto_id_with("commit_chevron_half"),
        Sense::click(),
    );
    label.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, "Commit changes"));
    chevron
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, "Commit split options"));

    let radius = crate::theme::CONTROL_RADIUS;
    let painter = ui.painter().clone();
    let fill_of = |half: &egui::Response| {
        let state = widgets::WidgetState::from_response(enabled, half);
        widgets::ButtonVariant::Primary.fill(state)
    };
    // Complementary rounding: the outer corners round and the seam does not,
    // so the two fills meet on a straight edge and the control reads as one
    // shape rather than two pills with a notch between them. The shared
    // primary always has a fill, disabled included, so neither half is ever
    // transparent — the control is brand-filled in every state it renders in.
    painter.rect_filled(
        left,
        CornerRadius {
            nw: radius,
            ne: 0,
            se: 0,
            sw: radius,
        },
        fill_of(&label),
    );
    painter.rect_filled(
        right,
        CornerRadius {
            nw: 0,
            ne: radius,
            se: radius,
            sw: 0,
        },
        fill_of(&chevron),
    );
    // The single hairline between them — a rule *on* the button, in brand ink
    // over the brand fill so it reads as a seam rather than as a border.
    painter.rect_stroke(
        Rect::from_min_max(
            Pos2::new(rule_x, button.top()),
            Pos2::new(rule_x + COMMIT_RULE_W, button.bottom()),
        ),
        CornerRadius::ZERO,
        Stroke::new(
            COMMIT_RULE_W,
            widgets::mix(Palette::BRAND, Palette::BRAND_INK, 0.35),
        ),
        StrokeKind::Inside,
    );

    let ink = widgets::ButtonVariant::Primary.text(if enabled {
        widgets::WidgetState::Idle
    } else {
        widgets::WidgetState::Disabled
    });
    painter.galley_with_override_text_color(
        Pos2::new(
            left.center().x - galley.size().x / 2.0,
            left.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    icons::centered_icon(ui, Icon::CHEVRON_DOWN, right.center(), 14.0, ink);

    widgets::focus_ring(ui, &label);
    widgets::focus_ring(ui, &chevron);
    CommitHalves { label, chevron }
}

/// The split button's dropdown menu (issue 07): `Commit and Push...` always,
/// plus the multi-repo cascade entry when more than one repo is selected.
fn commit_split_menu(chevron: &egui::Response, state: &mut AppState) {
    use egui::Popup;
    Popup::menu(chevron).show(|menu| {
        if menu
            .button("Commit and Push...")
            .on_hover_text("Commit then open the push dialog")
            .clicked()
        {
            do_commit(state, true);
            menu.close();
        }
        let selection_count = state.ui.repo_selection.len();
        if selection_count > 1 {
            menu.separator();
            let footer = state.commit_rail_footer(&state.ui.commit_message);
            menu.label(footer);
            let button_label = format!("Also commit on {selection_count} selected repos");
            if menu
                .button(button_label)
                .on_hover_text("Commit the same message on every selected repo")
                .clicked()
            {
                let message = state.ui.commit_message.clone();
                let amend = state.ui.amend;
                state.run_commit_across(&message, amend);
                state.ui.commit_message.clear();
                state.save_durable_ui();
                menu.close();
            }
        }
    });
}

fn do_commit(state: &mut AppState, and_push: bool) {
    let root = state.selected_root.clone();
    let msg = state.ui.commit_message.clone();
    let amend = state.ui.amend;
    // Record the recent message before `msg` moves into the 'static closure
    // (plan Phase 3): no extra String clone per commit.
    if !state.ui.recent_messages.contains(&msg) {
        state.ui.recent_messages.insert(0, msg.clone());
        state.ui.recent_messages.truncate(12);
    }
    if let Some(root) = root {
        state.dispatch_commit(&root, &msg, amend, and_push);
    }
    // Only the message well resets. The selection is "the files I am looking
    // at", and a commit says nothing about what the developer is looking at —
    // clearing it would throw away their place in the list after every commit.
    state.ui.commit_message.clear();
    state.save_durable_ui();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_state_is_derived_from_the_staged_and_unstaged_pair() {
        let of = |staged, unstaged| {
            StageState::of(&Change {
                path: PathBuf::from("a.txt"),
                status: ChangeStatus::Modified,
                chunks: vec![],
                staged,
                unstaged,
                orig_path: None,
            })
        };
        assert_eq!(of(true, false), StageState::All);
        assert_eq!(of(true, true), StageState::Partial);
        assert_eq!(of(false, true), StageState::None);
        assert_eq!(of(false, false), StageState::None);
    }

    #[test]
    fn row_display_splits_filename_from_dim_location() {
        // Issue 05 (design doc §4): `[filename] … [dim location]` — the
        // filename is the basename, the location is its parent directory.
        // Files at the repo root have no location to paint.
        assert_eq!(
            row_display(Path::new("base.txt")),
            ("base.txt".to_owned(), None)
        );
        assert_eq!(
            row_display(Path::new("src/app.rs")),
            ("app.rs".to_owned(), Some("src/".to_owned()))
        );
        assert_eq!(
            row_display(Path::new("a/b/c.txt")),
            ("c.txt".to_owned(), Some("a/b/".to_owned()))
        );
    }
}
