use egui::{Align, Color32, Layout, ScrollArea, Ui};

use turbogit_app::integrate_service::BranchFileStatus;
use turbogit_app::state::{AppState, PendingConfirm};

use crate::theme::{Palette, TYPE_BODY, data_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

pub const VERB: &str = "Get from Branch";

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
    let source = state.ui.dlg.get_from_branch_source.clone();
    let current = state.ui.dlg.get_from_branch_current.clone();
    let swapped = state.ui.dlg.get_from_branch_swapped;
    let rows = state.ui.dlg.get_from_branch_rows.clone();
    let error = state.ui.dlg.get_from_branch_error.clone();
    let root = state.ui.dlg.get_from_branch_root.clone();

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
        ScrollArea::vertical()
            .id_salt("compare_branch_files")
            .max_height(300.0)
            .show(ui, |ui| {
                for row in &rows {
                    let take = file_row(ui, row);
                    if take {
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
        if ui.button("Swap Branches").clicked() {
            state.swap_get_from_branch();
        }
    });
}

fn file_row(ui: &mut Ui, row: &turbogit_app::integrate_service::BranchFileRow) -> bool {
    let mut take = false;
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

fn status_word(status: BranchFileStatus) -> &'static str {
    match status {
        BranchFileStatus::Deleted => "Deleted",
        BranchFileStatus::Added => "Added",
        BranchFileStatus::Modified => "Modified",
    }
}

fn status_ink(status: BranchFileStatus) -> Color32 {
    match status {
        BranchFileStatus::Deleted => Palette::STATUS_DELETED,
        BranchFileStatus::Added => Palette::STATUS_ADDED,
        BranchFileStatus::Modified => Palette::STATUS_MODIFIED,
    }
}
