//! Branch widget (status bar) + Branches popup (E1/E2/E3–E10).

use crate::core::branch_service;
use crate::model::BranchKind;
use crate::state::{AppState, Dialog, PendingConfirm};
use egui::Ui;

/// Compact branch indicator in the status bar; opens the popup on click.
pub fn widget(ui: &mut Ui, state: &mut AppState) {
    let branch = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .and_then(|r| r.current_branch.clone())
        .unwrap_or_else(|| "<detached>".to_string());
    if ui.button(format!("⎇ {branch}")).clicked() {
        state.ui.branches_popup = true;
    }
}

/// The Branches popup: Recent / Local / Remote / Tags, favorite (Space),
/// checkout, compare, delete.
pub fn branches_popup(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.branches_popup {
        return;
    }
    let ctx = ui.ctx().clone();
    let mut open = state.ui.branches_popup;
    egui::Window::new("Branches")
        .open(&mut open)
        .default_width(420.0)
        .show(&ctx, |ui| {
            ui.text_edit_singleline(&mut state.ui.branch_filter);
            if ui.button("＋ New Branch…").clicked() {
                state.ui.dialog = Some(Dialog::NewBranch);
            }
            let id = match &state.selected_root {
                Some(id) => id.clone(),
                None => return,
            };
            let root = match state.multi.by_id(&id) {
                Some(r) => r.clone(),
                None => return,
            };
            let filter = state.ui.branch_filter.to_lowercase();
            let current = root.current_branch.clone();

            ui.separator();
            ui.heading("Local");
            // Ahead/behind for the current branch (Epic D3 / F1).
            if let Some((ahead, behind)) = state.ahead_behind.get(&id) {
                ui.horizontal(|ui| {
                    ui.label("Current:");
                    if *ahead > 0 {
                        ui.colored_label(egui::Color32::from_rgb(120, 200, 120), format!("↑{ahead}"));
                    }
                    if *behind > 0 {
                        ui.colored_label(egui::Color32::from_rgb(230, 160, 90), format!("↓{behind}"));
                    }
                });
            }
            for b in root
                .branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .filter(|b| filter.is_empty() || b.name.to_lowercase().contains(&filter))
            {
                ui.horizontal(|ui| {
                    let star = if b.favorite { "★" } else { "☆" };
                    if ui.button(star).clicked() {
                        branch_service::toggle_favorite(&mut state.multi, &id, &b.name);
                    }
                    let label = if Some(&b.name) == current.as_ref() {
                        format!("▶ {name}", name = b.name)
                    } else {
                        b.name.clone()
                    };
                    if ui.selectable_label(false, label).clicked() {
                        let rootp = id.0.clone();
                        let nm = b.name.clone();
                        state.run_git(format!("Checkout {nm}"), move |v| {
                            v.branch_checkout(&rootp, &nm)
                        });
                        state.ui.branches_popup = false;
                    }
                    if ui.button("⟲ Compare").clicked() {
                        state.ui.diff = Some(crate::state::DiffTarget {
                            root: id.clone(),
                            left: Some(b.name.clone()),
                            right: None,
                            path: None,
                        });
                        state.ui.branches_popup = false;
                    }
                    if ui.button("🗑").clicked() {
                        // Gate behind confirmation (Epic C8) — destructive.
                        state.ui.confirm = Some(PendingConfirm::DeleteLocalBranch {
                            name: b.name.clone(),
                        });
                    }
                });
            }

            ui.separator();
            ui.heading("Remote");
            for b in root
                .branches
                .iter()
                .filter(|b| b.kind == BranchKind::Remote)
                .filter(|b| filter.is_empty() || b.name.to_lowercase().contains(&filter))
            {
                ui.horizontal(|ui| {
                    let label = format!("↳ {name}", name = b.name);
                    if ui.selectable_label(false, label).clicked() {
                        let rootp = id.0.clone();
                        let nm = b.name.clone();
                        let start = format!("origin/{name}", name = b.name);
                        state.run_git(format!("Checkout {nm} (new local)"), move |v| {
                            v.branch_create(&rootp, &nm, true, Some(&start))
                        });
                        state.ui.branches_popup = false;
                    }
                    if ui.button("🗑").clicked() {
                        // Gate behind confirmation (Epic C8) — destructive.
                        let remote = b
                            .tracking
                            .as_ref()
                            .and_then(|t| t.split('/').next())
                            .unwrap_or("origin")
                            .to_string();
                        state.ui.confirm = Some(PendingConfirm::DeleteRemoteBranch {
                            remote,
                            name: b.name.clone(),
                        });
                    }
                });
            }

            ui.separator();
            ui.heading("Tags");
            if let Ok(tags) = state.vcs.tag_list(&id.0) {
                for t in tags
                    .iter()
                    .filter(|t| filter.is_empty() || t.to_lowercase().contains(&filter))
                {
                    if ui.selectable_label(false, format!("🔖 {t}")).clicked() {
                        let rootp = id.0.clone();
                        let nm = t.clone();
                        state.run_git(format!("Checkout {nm}"), move |v| {
                            v.tag_checkout(&rootp, &nm)
                        });
                        state.ui.branches_popup = false;
                    }
                }
            }
        });
    state.ui.branches_popup = open;
}
