//! VCS Operations Popup (M1) + Command Palette / "Find Action" (Epic F5).
//!
//! `Alt+\`` opens the context-sensitive VCS operations list. `Ctrl+Shift+A`
//! opens the command palette: a fuzzy-searchable list of every action, the
//! IntelliJ "Find Action" hallmark.

use crate::state::{AppState, Dialog};
use egui::Ui;

/// Every globally-invokable action, reused by both the VCS popup and the
/// command palette.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Refresh,
    Fetch,
    Pull,
    Push,
    Branches,
    NewBranch,
    Merge,
    Rebase,
    Stash,
    Shelve,
    Tag,
    CommitTab,
    Settings,
    Clone,
}

impl Action {
    fn label(self) -> &'static str {
        match self {
            Action::Refresh => "Refresh",
            Action::Fetch => "Fetch",
            Action::Pull => "Pull",
            Action::Push => "Push…",
            Action::Branches => "Branches…",
            Action::NewBranch => "New Branch…",
            Action::Merge => "Merge…",
            Action::Rebase => "Rebase…",
            Action::Stash => "Stash…",
            Action::Shelve => "Shelve…",
            Action::Tag => "Tag…",
            Action::CommitTab => "Go to Commit",
            Action::Settings => "Settings…",
            Action::Clone => "Clone…",
        }
    }

    fn all() -> &'static [Action] {
        &[
            Action::Refresh,
            Action::Fetch,
            Action::Pull,
            Action::Push,
            Action::Branches,
            Action::NewBranch,
            Action::Merge,
            Action::Rebase,
            Action::Stash,
            Action::Shelve,
            Action::Tag,
            Action::CommitTab,
            Action::Settings,
            Action::Clone,
        ]
    }
}

fn run_action(state: &mut AppState, action: Action) {
    let root = state.selected_path();
    match action {
        Action::Refresh => {
            state.rescan();
            if let Some(id) = &state.selected_root {
                let id = id.clone();
                state.fetch_log(id);
            }
        }
        Action::Fetch => {
            let r = root.clone();
            state.run_git("Fetch".into(), move |v| {
                if let Some(r) = &r {
                    v.fetch(r, None)
                } else {
                    Ok(())
                }
            });
        }
        Action::Pull => {
            let r = root.clone();
            let rebase = state.settings.update_method == crate::model::UpdateMethod::Rebase;
            state.run_git("Pull".into(), move |v| {
                if let Some(r) = &r {
                    v.pull(r, rebase)
                } else {
                    Ok(())
                }
            });
        }
        Action::Push => state.ui.dialog = Some(Dialog::Push),
        Action::Branches => state.ui.branches_popup = true,
        Action::NewBranch => state.ui.dialog = Some(Dialog::NewBranch),
        Action::Merge => state.ui.dialog = Some(Dialog::Merge),
        Action::Rebase => state.ui.dialog = Some(Dialog::Rebase),
        Action::Stash => state.ui.dialog = Some(Dialog::Stash),
        Action::Shelve => state.ui.dialog = Some(Dialog::Shelve),
        Action::Tag => state.ui.dialog = Some(Dialog::Tag),
        Action::CommitTab => state.ui.tab = crate::state::Tab::Commit,
        Action::Settings => state.ui.settings_open = true,
        Action::Clone => {
            // Focus the clone field in the left panel (best-effort).
            state.ui.toast = Some("Use the Clone field in the left panel.".into());
        }
    }
}

pub fn vcs_operations(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.vcs_popup {
        return;
    }
    let ctx = ui.ctx().clone();
    let mut open = state.ui.vcs_popup;
    egui::Window::new("VCS Operations")
        .open(&mut open)
        .show(&ctx, |ui| {
            for a in Action::all() {
                if ui.button(a.label()).clicked() {
                    run_action(state, *a);
                }
            }
        });
    if !open {
        state.ui.vcs_popup = false;
    }
}

/// Command palette (Epic F5 / "Find Action"). Searchable, keyboard-friendly.
pub fn command_palette(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.command_palette {
        return;
    }
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Find Action")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 60.0))
        .default_width(420.0)
        .resizable(false)
        .show(&ctx, |ui| {
            ui.text_edit_singleline(&mut state.ui.command_query)
                .request_focus();
            ui.separator();
            let q = state.ui.command_query.to_lowercase();
            let matches: Vec<Action> = Action::all()
                .iter()
                .copied()
                .filter(|a| q.is_empty() || a.label().to_lowercase().contains(&q))
                .collect();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for &a in &matches {
                        if ui.selectable_label(false, a.label()).clicked() {
                            run_action(state, a);
                            state.ui.command_palette = false;
                        }
                    }
                    if matches.is_empty() {
                        ui.label("No matching actions.");
                    }
                });
        });
    if !open {
        state.ui.command_palette = false;
    }
}
