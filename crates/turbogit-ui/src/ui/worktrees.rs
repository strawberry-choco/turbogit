//! Worktrees tool tab (issue 14, screen 01): a real browser over the
//! focused root's linked worktrees — path, checked-out branch, dirty
//! status — with actions to add (dialog) and remove (confirm) a worktree.
//! Reads the cached worktree list (`RootCaches::worktrees`), which the
//! shell keeps filled for the focused root.

use egui::{Align, FontFamily, FontId, Layout, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::Worktree;

use crate::theme::Palette;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    // Header: surface label + the add affordance.
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("WORKTREES")
                .strong()
                .font(FontId::new(11.0, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        if ui.button("Add worktree").clicked() {
            state.ui.dlg.wt_path.clear();
            state.ui.dlg.wt_branch.clear();
            state.ui.dialog = Some(Dialog::NewWorktree);
        }
    });
    ui.add_space(4.0);

    let Some(id) = state.selected_root.clone() else {
        return;
    };
    // Snapshot the list so the row callbacks can borrow `state` mutably.
    let worktrees: Vec<Worktree> = state.caches.worktrees(&id).unwrap_or(&[]).to_vec();
    if state.caches.worktrees(&id).is_none() {
        ui.weak("Loading worktrees…");
    }

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
fn worktree_row(ui: &mut Ui, state: &mut AppState, wt: &Worktree) {
    let name = wt
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<worktree>")
        .to_string();
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(
            RichText::new(wt.path.display().to_string())
                .font(FontId::new(12.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        ui.label(
            RichText::new(&wt.branch)
                .font(FontId::new(11.0, FontFamily::Proportional))
                .color(Palette::BRAND),
        );
        if wt.dirty {
            ui.label(
                RichText::new("dirty")
                    .font(FontId::new(11.0, FontFamily::Proportional))
                    .color(Palette::STATE_WARNING),
            );
        } else {
            ui.label(
                RichText::new("clean")
                    .font(FontId::new(11.0, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button(format!("Remove {name}")).clicked() {
                state.ui.confirm = Some(turbogit_app::state::PendingConfirm::RemoveWorktree {
                    path: wt.path.clone(),
                });
            }
        });
    });
    ui.add_space(2.0);
}

/// The New worktree dialog body (rendered by `dialogs::show`). Path is
/// resolved against the focused root when relative; the branch is created
/// at the worktree.
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
