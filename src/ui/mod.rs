//! UI layout (egui 0.35 API).
//!
//! Issue #9 replaced the old panel layout outright with the IntelliJ-style
//! IDE shell (spec §6): a 38px topbar, 34px toolbar, 48px sidebar rail,
//! 32px tab strip and ~24px status bar — all composed in [`shell`]. The
//! central body routes between the Welcome placeholder ([`welcome`], shown
//! when no project is open) and the active tool window (Commit / Log /
//! Settings).
//!
//! This module owns what wraps the shell: global shortcut dispatch lives in
//! `shell::render`, and the floating surfaces below are rendered on top of
//! it every frame — Branches popup, VCS operations popup, command palette,
//! modal dialogs, confirm prompts, the Settings window, and the toast.

#![allow(dead_code)]

pub mod branch_widget;
pub mod commit_window;
pub mod conflicts;
pub mod dialogs;
pub mod diff;
pub mod icons;
pub mod log_window;
pub mod popups;
pub mod push_dialog;
pub mod shell;
pub mod welcome;
pub mod widgets;

use crate::state::{AppState, Dialog, PendingConfirm};
use egui::{Color32, Context, Ui};

/// Render one full frame: the IDE shell plus its floating surfaces.
pub fn render(ui: &mut Ui, state: &mut AppState) {
    // Shell frame + central body (Welcome placeholder or tool window).
    // The five frozen shortcuts (ADR-0009) dispatch inside the shell.
    shell::render(ui, state);

    // Floating surfaces.
    branch_widget::branches_popup(ui, state);
    popups::vcs_operations(ui, state);
    popups::command_palette(ui, state);
    if let Some(d) = state.ui.dialog {
        if d == Dialog::Push {
            // Issue #20: the redesigned push dialog lives in its own module.
            push_dialog::show(ui, state);
        } else {
            dialogs::show(ui, state, d);
        }
    }
    render_confirm(ui, state);
    settings_window(ui, state);
    render_toast(ui, state);
}

// -------------------------------------------------------------- settings ---

fn settings_inline(ui: &mut Ui, state: &mut AppState) {
    let s = &mut state.settings;
    ui.separator();
    ui.checkbox(&mut s.staging_area, "Staging-area mode (Unstaged/Staged)");
    ui.checkbox(&mut s.synchronous_branches, "Synchronous branch control");
    ui.checkbox(
        &mut s.restore_workspace,
        "Restore workspace context per branch",
    );
    ui.checkbox(&mut s.gutter_markers, "Show gutter change markers");
    ui.checkbox(&mut s.warn_crlf, "Warn before committing CRLF");
    ui.checkbox(&mut s.warn_detached, "Warn in detached HEAD");
    ui.checkbox(
        &mut s.no_commit_hooks,
        "Disable git commit hooks (IDE-wide)",
    );
    ui.separator();
    ui.label("Commit message:");
    ui.horizontal(|ui| {
        ui.label("Template file:");
        ui.text_edit_singleline(&mut s.commit_template);
    });
    ui.label("Date format in log:");
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut s.date_format,
            crate::model::DateFormat::Relative,
            "Relative",
        );
        ui.radio_value(
            &mut s.date_format,
            crate::model::DateFormat::Absolute,
            "Absolute",
        );
        ui.radio_value(&mut s.date_format, crate::model::DateFormat::Iso, "ISO");
    });
    ui.separator();
    ui.label("Update method:");
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut s.update_method,
            crate::model::UpdateMethod::Merge,
            "Merge",
        );
        ui.radio_value(
            &mut s.update_method,
            crate::model::UpdateMethod::Rebase,
            "Rebase",
        );
    });
    ui.label("Clean working tree on update:");
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut s.clean_tree_method,
            crate::model::CleanTreeMethod::Stash,
            "Stash",
        );
        ui.radio_value(
            &mut s.clean_tree_method,
            crate::model::CleanTreeMethod::Shelve,
            "Shelve",
        );
    });
    ui.label("Incoming-check mode:");
    ui.horizontal(|ui| {
        ui.radio_value(
            &mut s.incoming_check,
            crate::model::IncomingCheckMode::Auto,
            "Auto",
        );
        ui.radio_value(
            &mut s.incoming_check,
            crate::model::IncomingCheckMode::Always,
            "Always",
        );
        ui.radio_value(
            &mut s.incoming_check,
            crate::model::IncomingCheckMode::Never,
            "Never",
        );
    });
    ui.label("Protected branch patterns (comma separated):");
    let mut pats = s.protected_branch_patterns.join(", ");
    if ui.text_edit_singleline(&mut pats).changed() {
        s.protected_branch_patterns = pats
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
    }
    ui.label("Git executable (blank = PATH):");
    ui.text_edit_singleline(&mut s.git_executable);
    if ui.button("Save settings").clicked() {
        let _ = crate::persistence::save_settings(&state.project_dir, &state.settings);
        // Reflect a changed git executable in the live engine (ADR-0001:
        // rebuild behind the seam).
        state.executor = std::sync::Arc::new(crate::engine::cli::CliExecutor {
            settings: state.settings.clone(),
        });
        state.persist_ui();
        state.ui.toast = Some("✓ Settings saved".into());
    }
}

fn settings_window(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.settings_open {
        return;
    }
    let mut open = state.ui.settings_open;
    egui::Window::new("Settings")
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            settings_inline(ui, state);
        });
    state.ui.settings_open = open;
}

// ----------------------------------------------------------------- toast ---

fn render_toast(ui: &mut Ui, state: &mut AppState) {
    if let Some(msg) = state.ui.toast.clone() {
        // Track when the toast first appeared so it can auto-dismiss (Epic H1).
        let now = ui.ctx().input(|i| i.time);
        if state.ui.toast_shown_at.is_none() {
            state.ui.toast_shown_at = Some(now);
        }
        let shown = now - state.ui.toast_shown_at.unwrap_or(now);
        if shown > 4.0 {
            state.ui.toast = None;
            state.ui.toast_shown_at = None;
            return;
        }
        let ctx: Context = ui.ctx().clone();
        egui::Window::new("Notice")
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -40.0))
            .resizable(false)
            .title_bar(false)
            .show(&ctx, |ui| {
                let col = if msg.starts_with("✗") {
                    Color32::RED
                } else {
                    Color32::GREEN
                };
                ui.horizontal(|ui| {
                    ui.colored_label(col, &msg);
                    if ui.small_button("Dismiss").clicked() {
                        state.ui.toast = None;
                        state.ui.toast_shown_at = None;
                    }
                });
            });
    }
}

/// Confirmation dialog for destructive actions (Epic C8).
fn render_confirm(ui: &mut Ui, state: &mut AppState) {
    let confirm = match &state.ui.confirm {
        Some(c) => c.clone(),
        None => return,
    };
    let msg = match &confirm {
        PendingConfirm::Discard { changes } => {
            format!(
                "Discard changes to {} file(s)? This cannot be undone.",
                changes.len()
            )
        }
        PendingConfirm::DeleteLocalBranch { name } => {
            format!("Delete local branch '{name}'? This cannot be undone.")
        }
        PendingConfirm::DeleteRemoteBranch { remote, name } => {
            format!("Delete remote branch '{remote}/{name}'? This cannot be undone.")
        }
        PendingConfirm::InitHere => "Initialize a git repository in this directory?".to_string(),
        PendingConfirm::CloneRepo => "Clone the repository at the given URL?".to_string(),
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Confirm")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .show(&ctx, |ui| {
            ui.label(&msg);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    state.run_confirmed(confirm.clone());
                    state.ui.confirm = None;
                }
                if ui.button("Cancel").clicked() {
                    state.ui.confirm = None;
                }
            });
        });
    if !open {
        state.ui.confirm = None;
    }
}
