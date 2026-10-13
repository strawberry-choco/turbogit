//! `Get from Branch`: the window the reference's `Show Diff with Working Tree`
//! opens, and the only place its fifth integration verb lives.
//!
//! The flow is: select the
//! branch that HAS the file → compare it with the working tree → read the list of
//! paths that differ, each on one of three rungs → pick a row → `Get from Branch`
//! → commit and push. "Take that one file from over there" is a daily act that
//! neither `cherry-pick` nor `merge` covers, and it is the only way to bring across
//! a path the target branch does not have at all.
//!
//! The window is a **reader of a snapshot**, never a caller of git: rows arrive on
//! AppState as plain data (`DialogState::get_from_branch_rows`) and the verbs hand
//! intents back. `Swap Branches` and the take both go through the app layer, which
//! re-reads, because the pane re-deriving either fact would put a git question on
//! the render thread.
//!
//! The three-rung grammar is the presentation contract here, and it is the same one
//! the diff viewer states for change kinds — green = Added, blue = Modified,
//! grey = Deleted — so every rung is a `Palette` token read in [`status_ink`] and
//! nothing at this call site owns a colour. The word and the ink are both functions
//! of the one [`BranchFileStatus`] value, which is what makes "row says Added in
//! Deleted's grey" unrepresentable rather than merely unlikely.

use egui::{Align, Color32, Layout, ScrollArea, Ui};

use turbogit_app::integrate_service::BranchFileStatus;
use turbogit_app::state::{AppState, PendingConfirm};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

/// The verb's label, in one place: the row's control and the confirmation's
/// question are the same act, and a window that called it one thing and the
/// confirm another would read as two verbs.
pub const VERB: &str = "Get from Branch";

/// Render the window while `state.ui.dialog` is `Dialog::GetFromBranch`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    let source = state.ui.dlg.get_from_branch_source.clone();
    egui::Window::new(format!("{VERB} · {source}"))
        .open(&mut open)
        .default_width(620.0)
        .show(&ctx, |ui| body(ui, state));
    if !open {
        state.ui.dialog = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    // Snapshot the frame's read-only inputs so the row handlers can take
    // `&mut AppState` freely (the deferred-intent shape this shell already uses).
    let source = state.ui.dlg.get_from_branch_source.clone();
    let current = state.ui.dlg.get_from_branch_current.clone();
    let swapped = state.ui.dlg.get_from_branch_swapped;
    let rows = state.ui.dlg.get_from_branch_rows.clone();
    let error = state.ui.dlg.get_from_branch_error.clone();
    let root = state.ui.dlg.get_from_branch_root.clone();

    // The header names the base side FIRST, because that is the one fact the two
    // rungs below are read against: `Deleted` means "on the base side, missing on
    // the other". A swap exchanges the two names rather than re-colouring the list,
    // so the sentence and the rows cannot drift apart.
    let (base, other) = if swapped {
        (current.as_str(), source.as_str())
    } else {
        (source.as_str(), current.as_str())
    };
    ui.label(format!("Comparing {base} with {other} (working tree)"));

    widgets::group_title(ui, "FILES THAT DIFFER");
    if let Some(message) = error {
        widgets::inline_error(ui, format!("Could not read the comparison — {message}"));
    } else if rows.is_empty() {
        widgets::empty_state(ui, "No file differs between the two sides.");
    } else {
        ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for row in &rows {
                let take = file_row(ui, row);
                if take {
                    // The window is per root, so the root that was compared is the
                    // root the take writes into — carried in the confirmation rather
                    // than re-read from whichever root is focused at OK time.
                    let Some(root) = root.clone() else { return };
                    state.ui.confirm = Some(PendingConfirm::GetFileFromBranch {
                        root,
                        source: source.clone(),
                        path: row.path.clone(),
                    });
                }
            }
        });
    }

    widgets::dialog_footer(ui, |ui| {
        if ui.button("Close").clicked() {
            state.ui.dialog = None;
        }
        // The reference's repeated affordance, spelled exactly as the Compare
        // Branches footer spells it so one link is one control across the two
        // windows.
        if ui.button("Swap Branches").clicked() {
            state.swap_get_from_branch();
        }
    });
}

/// One row: path, rung word, and the verb on the rungs it can run on. Returns
/// whether its control was clicked.
///
/// The verb is **withheld** — not disabled, not greyed — everywhere but the blue
/// rung, and that absence is the safety property: on a green row the base side has
/// no copy of the path at all, so "take its entire contents" would be a delete
/// wearing a take's label. The reference's own grammar is what prevents it, which is
/// why the rung is decided by the service's read rather than by a second opinion
/// here.
fn file_row(ui: &mut Ui, row: &turbogit_app::integrate_service::BranchFileRow) -> bool {
    let mut take = false;
    // One `horizontal` per row: the fill, the word and the control share the row's
    // rect, so a suite can aim at THIS row's control by position rather than at the
    // first button in the frame wearing the verb's label.
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(row.path.display().to_string())
                .font(data_font(TYPE_BODY))
                .color(Palette::INK_2),
        );
        ui.label(
            egui::RichText::new(status_word(row.status))
                .font(crate::theme::chrome_font(TYPE_BODY))
                .color(status_ink(row.status)),
        );
        if matches!(row.status, BranchFileStatus::Modified) {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if kit_button(ui, KitButton::Primary, VERB).clicked() {
                    take = true;
                }
            });
        }
    });
    take
}

/// The rung's word — the diff viewer's change-kind vocabulary, deliberately the
/// same three words rather than a compare-window dialect of its own.
fn status_word(status: BranchFileStatus) -> &'static str {
    match status {
        BranchFileStatus::Deleted => "Deleted",
        BranchFileStatus::Added => "Added",
        BranchFileStatus::Modified => "Modified",
    }
}

/// The rung's ink, and the only place this surface names a colour: green = Added,
/// blue = Modified, grey = Deleted. Every one is a shared token, so the grammar
/// moves with the palette instead of being re-derived per call site.
fn status_ink(status: BranchFileStatus) -> Color32 {
    match status {
        BranchFileStatus::Deleted => Palette::STATUS_DELETED,
        BranchFileStatus::Added => Palette::STATUS_ADDED,
        BranchFileStatus::Modified => Palette::STATUS_MODIFIED,
    }
}
