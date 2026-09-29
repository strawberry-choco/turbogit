//! Worktrees tool tab (issue 14, screen 01): a real browser over the
//! focused root's linked worktrees — path, checked-out branch, dirty
//! status — with actions to add (dialog) and remove (confirm) a worktree.
//! Reads the cached worktree list (`RootCaches::worktrees`), which the
//! shell keeps filled for the focused root.
//!
//! The pane's chrome is entirely borrowed: the header is the shared pane
//! header, the labels are the shared column-header row, the offsets both of
//! those and the rows below read are the one [`WORKTREE_COLUMNS`] table, the
//! branch is the shared ref chip, and each row's state is the shared mark
//! pair. Nothing here decides a colour, a radius or an x offset of its own.

use egui::{Align, Color32, Layout, RichText, ScrollArea, Ui, Vec2, WidgetInfo, WidgetType};
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::Worktree;

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{self, KitButton};
use crate::ui::widgets::{self, PaneColumn};

/// The Worktrees pane's columns: one table, read by **both** the shared
/// column-header row and every data row below it.
///
/// That is the whole reason it is a table and not a list of numbers pasted into
/// two places — a column cannot be narrow in the header and wide in the rows,
/// because [`PaneColumn::origin`] is consulted by the header's paint and by
/// [`widgets::column_cell`] with the same number.
///
/// **HEAD is declared and left empty.** The story-43 set is path, branch, head,
/// state and actions, and the cached [`Worktree`] carries no commit for the
/// branch it has checked out — a per-worktree head is a git read this pane does
/// not make. So the column is here, the label is over it, and the cell paints
/// **nothing at all**: an em dash or an ellipsis in that cell would read as a
/// value the pane knows, and the honest rendering of "this pane has no head for
/// this worktree" is a column with nothing in it. The state cell beside it is
/// the one that is never empty, and the two claims are kept apart on purpose.
const WORKTREE_COLUMNS: [PaneColumn; 5] = [
    PaneColumn::start("PATH", 8.0),
    PaneColumn::start("BRANCH", 244.0),
    PaneColumn::start("HEAD", 344.0),
    PaneColumn::start("STATE", 404.0),
    PaneColumn::end("ACTIONS", 0.0),
];

/// Air between two adjacent cells' columns, so a long cell in one column does
/// not read as belonging to the next.
///
/// Applied to the *path* cell, which is the one cell allowed to be as wide as
/// the table gives it, and which is therefore the only cell that can reach the
/// column beside it.
const CELL_GAP: f32 = 8.0;

/// What a worktree whose dirty probe has not returned says.
///
/// A state word, not a spinner: nothing here animates, and the word is the
/// information. Before this existed the `None` arm was an empty match, so a
/// blank state cell was ambiguous between "clean" and "not known yet" — and
/// "clean" is the one word in this column that a reader is entitled to act on,
/// so it cannot be the word a missing answer falls back to.
const PROBING: &str = "probing…";

pub fn show(ui: &mut Ui, state: &mut AppState) {
    // The list is read **before** the header, because the header's count chip is
    // that list's own length and not a second opinion about it. The chip is
    // painted only once the list has landed: a count that appears as 0 and
    // becomes 3 a frame later is a flickering number, and the pane already says
    // "Loading worktrees…" underneath, which is the honest thing to show while
    // the answer is not known.
    let root = state.selected_root.clone();
    let loaded: Option<Vec<Worktree>> = root
        .as_ref()
        .and_then(|id| state.caches.worktrees(id))
        .map(<[Worktree]>::to_vec);
    let count = loaded.as_ref().map(|worktrees| worktrees.len().to_string());

    widgets::pane_header(ui, "WORKTREES", count.as_deref(), |ui| {
        // The pane's one primary action, in the shared header's right-aligned
        // slot. It is the **only** blue in this pane, which is what makes the
        // ref chip below it worth having: a worktree row that painted its
        // branch in the brand would put a second blue on the screen and the
        // reader would have to read labels to work out which one is clickable.
        //
        // The *band-height* primary is the one, and that is not a preference. A
        // pane header's band is a fixed height that grows to fit its tallest
        // child, so a taller control in this slot makes the Worktrees header
        // taller than every other pane's and pushes its own hairline down with
        // it. That is precisely what the cross-pane header-geometry ratchet in
        // `tests/widget_library.rs` exists to catch, and it is right to catch
        // it: "every tool pane has the same header" stops being true the moment
        // one pane's action slot is taller than the band. The shared vocabulary
        // now carries that button — `widgets::compact_primary_button`, the same
        // brand ladder as the 32px `primary_button` at the band's height — so
        // the R1 primary has **one** implementation instead of one shared plus a
        // page-local stand-in borrowed from the branch kit.
        if widgets::compact_primary_button(ui, "Add worktree").clicked() {
            state.ui.dlg.wt_path.clear();
            state.ui.dlg.wt_branch.clear();
            state.ui.dialog = Some(Dialog::NewWorktree);
        }
    });
    ui.add_space(4.0);

    let Some(id) = root else {
        return;
    };
    let worktrees = loaded.unwrap_or_default();
    if state.caches.worktrees(&id).is_none() {
        ui.weak("Loading worktrees…");
    }

    widgets::column_header(ui, ui.available_rect_before_wrap(), &WORKTREE_COLUMNS);
    ui.add_space(4.0);

    ScrollArea::vertical().show(ui, |ui| {
        if worktrees.is_empty() && state.caches.worktrees(&id).is_some() {
            ui.weak("No linked worktrees. Use “Add worktree” to check a branch out into its own directory.");
        }
        for wt in &worktrees {
            worktree_row(ui, state, wt);
        }
    });
}

/// One worktree row: path, branch, dirty status, and the remove action.
///
/// **Name collision, deliberately kept:** `worktree_row` *contains* the retired
/// `widgets::tree_row` as a substring but has nothing to do with it. This is a
/// page-local hand-laid row for one worktree, not the retired general
/// fixed-height tree/list wrapper. A blind name search for `tree_row` in the
/// dead-widget sweep would have deleted live code; do not let a future sweep do
/// that.
///
/// The columns are read from [`WORKTREE_COLUMNS`] through
/// [`widgets::column_cell`] and never from a number written here, which is what
/// keeps a row's cells under the header's labels. The trailing ACTIONS column is
/// placed by the row's own right-aligned layout, which lands on the same
/// trailing edge the header's label for it is measured back from.
fn worktree_row(ui: &mut Ui, state: &mut AppState, wt: &Worktree) {
    let name = wt
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<worktree>")
        .to_string();
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 0, |ui| path_cell(ui, wt));
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 1, |ui| {
            // The branch is a **ref name**, so it is the ref chip: the
            // raised-on-card fill with secondary ink, never the brand. A chip
            // here is not decoration — it is the one object in the app that
            // means "this is a ref", and a bare blue string is what made a
            // worktree row look like a fifth call to action.
            widgets::ref_chip(ui, &wt.branch);
        });
        // HEAD: reserved, and empty. See `WORKTREE_COLUMNS` for why an empty
        // cell is the correct rendering here and what it must never become.
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 2, |_ui| {});
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 3, |ui| {
            let (word, ink) = probe_state(wt.dirty);
            components::state_mark(ui, word, ink);
        });
        // The trailing column is placed from the pane's own right edge, so the
        // label its header shows and the control it names share that edge.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // A **quiet row action**, not a button. A full button on every row
            // is the loudest thing in a list whose usual job is reading, and a
            // destructive verb painted that way invites the press. The quiet
            // variant is transparent at rest, steps to the raised fill on hover,
            // and registers the same labelled Button node, so the row action is
            // still reachable by name from a keyboard and a screen reader.
            let remove = components::kit_button(ui, KitButton::Quiet, &format!("Remove {name}"));
            if remove.clicked() {
                state.ui.confirm = Some(PendingConfirm::RemoveWorktree {
                    path: wt.path.clone(),
                });
            }
        });
    });
    ui.add_space(2.0);
}

/// The PATH cell: the row's **primary column**, in the data face.
///
/// Primary ink because the path is what tells one row from another — it is the
/// one thing in the row the reader is looking for. The monospaced data face
/// because a path is data, and because a monospaced column of paths is
/// scannable in a way a proportional one is not (same rule `ref_chip` and
/// `hash_chip` follow).
///
/// The width is **derived, never stated**: it is the gap from this cell's
/// origin to the next column's, both read from [`WORKTREE_COLUMNS`], so the path
/// takes exactly the room the other columns leave it and no more. A long path
/// elides rather than pushing the columns beside it out of line.
fn path_cell(ui: &mut Ui, wt: &Worktree) {
    let next_column = WORKTREE_COLUMNS[1].origin(ui.max_rect());
    let width = (next_column - ui.cursor().left() - CELL_GAP).max(0.0);
    // Laid out in a band of its own rather than through `add_sized`, because
    // `add_sized` **centres** its widget in the size it is given: a path would
    // then start wherever its own length left it, and a column whose cells do
    // not start at the column are not a column. `left_to_right(Align::Center)`
    // is the one layout that starts the text on the cell's left edge and still
    // centres it against the chip and the state mark in the same 18px band.
    ui.allocate_ui_with_layout(
        Vec2::new(width, widgets::CHIP_HEIGHT),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(wt.path.display().to_string())
                        .font(data_font(TYPE_BODY))
                        .color(Palette::INK),
                )
                .truncate(),
            );
        },
    );
}

/// The STATE cell's word and ink for one worktree's dirty probe.
///
/// **Three arms, and none of them is empty.** A cell in this column always says
/// something, because the two answers a reader can act on — clean and dirty —
/// are the two the pane is certain about, and a third arm that rendered nothing
/// made "clean" the default reading of a blank.
///
/// All three go through the shared mark pair, a leading dot plus the state in
/// words, and the pair holds **no** state vocabulary: the colour arrives as an
/// argument. That is why the probing arm can join the other two without inventing
/// a fourth state colour of its own — the word is what distinguishes it, which
/// is R6's claim that a state is text, stated once rather than per surface.
///
/// The probing ink is deliberately the same muted step `clean` uses. Probing is
/// not a severity: it says the pane does not know yet, and a colour that
/// competed with `dirty`'s warning would be the pane reporting a problem it does
/// not have. It is not the dim `INK_4` step either — the word "probing…" is
/// information the user is owed, and `INK_4` is reserved for the things they
/// already know they can ignore.
fn probe_state(dirty: Option<bool>) -> (&'static str, Color32) {
    match dirty {
        Some(true) => ("dirty", Palette::STATE_WARNING),
        Some(false) => ("clean", Palette::INK_3),
        None => (PROBING, Palette::INK_3),
    }
}

/// The New worktree dialog body (rendered by `dialogs::show`). Path is
/// resolved against the focused root when relative; the branch is created
/// at the worktree.
///
/// Both inputs carry an accessibility label of their own, distinct from each
/// other and from the visible label above them, attached to the text edit rather
/// than to the label: a screen reader reading the dialog hears "Path (relative
/// to the focused repo)" twice and cannot tell the two fields apart. This is the
/// dialog's existing contract and the conformance work does not change it.
pub fn new_worktree_dialog(ui: &mut Ui, state: &mut AppState) {
    ui.label("Path (relative to the focused repo):");
    let path_edit = ui.text_edit_singleline(&mut state.ui.dlg.wt_path);
    path_edit.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::TextEdit,
            true,
            "Worktree path input".to_string(),
        )
    });
    ui.label("New branch to check out:");
    let branch_edit = ui.text_edit_singleline(&mut state.ui.dlg.wt_branch);
    branch_edit.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::TextEdit,
            true,
            "Worktree branch input".to_string(),
        )
    });
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() {
            let raw = state.ui.dlg.wt_path.trim().to_string();
            let branch = state.ui.dlg.wt_branch.trim().to_string();
            if !raw.is_empty() && !branch.is_empty() {
                let path = if let Some(root) = state.selected_path() {
                    resolve_wt_path(&root, &raw)
                } else {
                    raw.into()
                };
                state.add_worktree(path, branch);
                state.ui.dialog = None;
            }
        }
        if ui.button("Cancel").clicked() {
            state.ui.dialog = None;
        }
    });
}

/// Resolve the typed worktree path: absolute stays as-is, relative joins
/// the focused root.
fn resolve_wt_path(root: &std::path::Path, raw: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}
