use egui::{Align, Color32, Layout, Ui, WidgetInfo, WidgetType};
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{DisplayName, Worktree};

use crate::theme::Palette;
use crate::ui::column_table::{self, PaneTable};
use crate::ui::components::{self, KitButton};
use crate::ui::widgets::{self, PaneColumn};

const WORKTREE_COLUMNS: [PaneColumn; 5] = [
    PaneColumn::start("PATH", 8.0),
    PaneColumn::start("BRANCH", 244.0),
    PaneColumn::start("HEAD", 344.0),
    PaneColumn::start("STATE", 404.0),
    PaneColumn::end("ACTIONS", 0.0),
];

const PROBING: &str = "probing…";

pub fn show(ui: &mut Ui, state: &mut AppState) {
    column_table::column_table_pane(
        ui,
        state,
        PaneTable {
            title: "WORKTREES",
            columns: &WORKTREE_COLUMNS,
            loading: "Loading worktrees…",
            empty: "No linked worktrees. Use “Add worktree” to check a branch out into its own directory.",
            cached: |state, id| state.caches.worktrees(id).map(<[_]>::to_vec),
            actions: &mut |ui, state| {
                if widgets::compact_primary_button(ui, "Add worktree").clicked() {
                    state.ui.dlg.wt_path.clear();
                    state.ui.dlg.wt_branch.clear();
                    state.ui.dialog = Some(Dialog::NewWorktree);
                }
            },
            row: &mut worktree_row,
        },
    );
}

fn worktree_row(ui: &mut Ui, state: &mut AppState, wt: &Worktree) {
    let name = wt.path.display_name_or("<worktree>");
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 0, |ui| {
            column_table::path_cell(ui, &WORKTREE_COLUMNS, &wt.path)
        });
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 1, |ui| {
            widgets::ref_chip(ui, &wt.branch);
        });
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 2, |_ui| {});
        widgets::column_cell(ui, &WORKTREE_COLUMNS, 3, |ui| {
            let (word, ink) = probe_state(wt.dirty);
            components::state_mark(ui, word, ink);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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

fn probe_state(dirty: Option<bool>) -> (&'static str, Color32) {
    match dirty {
        Some(true) => ("dirty", Palette::STATE_WARNING),
        Some(false) => ("clean", Palette::INK_3),
        None => (PROBING, Palette::INK_3),
    }
}

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

fn resolve_wt_path(root: &std::path::Path, raw: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}
