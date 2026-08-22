//! UI layout (egui 0.35 API).
//!
//! Layout:
//! - `Panel::bottom`  → status-bar **VCS widget** (branch + sync buttons).
//! - `Panel::left`    → repository list + changelist summary.
//! - `CentralPanel`   → tabbed area: Commit / Log / History.
//! - Floating `Window`s → Branches popup, VCS operations popup, dialogs,
//!   Settings, toast.
//!
//! Panels are shown with a parent `&mut Ui` (egui 0.35). The Commit / Log /
//! History panels live in dedicated submodules.

#![allow(dead_code)]

pub mod branch_widget;
pub mod commit_window;
pub mod conflicts;
pub mod dialogs;
pub mod diff;
pub mod icons;
pub mod log_window;
pub mod popups;

use crate::state::{AppState, Dialog, PendingConfirm, Tab};
use egui::{Align, Color32, Context, Key, Layout, Panel, ScrollArea, Ui};

pub fn render(ui: &mut Ui, state: &mut AppState) {
    // VCS Operations popup hotkey: Alt+` (Backquote).
    let open_popup = ui.input(|i| i.key_pressed(Key::Backtick) && i.modifiers.alt);
    if open_popup {
        state.ui.vcs_popup = true;
    }

    // Global keyboard shortcuts (Epic I1).
    let ks = ui.input(|i| Shortcut {
        commit: i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(Key::K),
        push: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::K),
        refresh: i.modifiers.ctrl && i.key_pressed(Key::T),
        find: i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::A),
    });
    if ks.commit {
        state.ui.tab = Tab::Commit;
        state.persist_ui();
    }
    if ks.push {
        state.ui.dialog = Some(Dialog::Push);
    }
    if ks.refresh {
        state.rescan();
        if let Some(id) = &state.selected_root {
            let id = id.clone();
            state.fetch_log(id);
        }
    }
    if ks.find {
        state.ui.command_palette = true;
        state.ui.command_query.clear();
    }

    render_status_bar(ui, state);
    render_left(ui, state);
    render_central(ui, state);

    // Floating surfaces.
    branch_widget::branches_popup(ui, state);
    popups::vcs_operations(ui, state);
    popups::command_palette(ui, state);
    if let Some(d) = state.ui.dialog {
        dialogs::show(ui, state, d);
    }
    render_confirm(ui, state);
    settings_window(ui, state);
    render_toast(ui, state);
}

struct Shortcut {
    commit: bool,
    push: bool,
    refresh: bool,
    find: bool,
}

// ----------------------------------------------------------------- status --

fn render_status_bar(ui: &mut Ui, state: &mut AppState) {
    Panel::bottom("status_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            branch_widget::widget(ui, state);
            ui.separator();
            if let Some(id) = &state.selected_root {
                if let Some(root) = state.multi.by_id(id) {
                    ui.label(format!(
                        "modified: {}   unversioned: {}   conflicts: {}",
                        root.status.modified(),
                        root.status.unversioned(),
                        root.status.conflicted.len(),
                    ));
                    if let Some((ahead, behind)) = state.ahead_behind.get(&root.id) {
                        if *ahead > 0 {
                            ui.colored_label(Color32::from_rgb(120, 200, 120), format!("↑{ahead}"));
                        }
                        if *behind > 0 {
                            ui.colored_label(Color32::from_rgb(230, 160, 90), format!("↓{behind}"));
                        }
                    }
                }
            } else {
                ui.label("No repository selected");
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button("⚙ Settings")
                    .on_hover_text("Open settings")
                    .clicked()
                {
                    state.ui.settings_open = true;
                }
                if ui
                    .button("⏏ Push")
                    .on_hover_text("Push commits to remote (Ctrl+Shift+K)")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::Push);
                }
                if ui
                    .button("⤓ Pull")
                    .on_hover_text("Pull updates from remote")
                    .clicked()
                {
                    let root = state.selected_path();
                    let rebase = state.settings.update_method == crate::model::UpdateMethod::Rebase;
                    state.run_git("Pull".into(), move |v| {
                        if let Some(r) = &root {
                            v.pull(r, rebase)
                        } else {
                            Ok(())
                        }
                    });
                }
                if ui
                    .button("⤒ Fetch")
                    .on_hover_text("Fetch from remote")
                    .clicked()
                {
                    let root = state.selected_path();
                    state.run_git("Fetch".into(), move |v| {
                        if let Some(r) = &root {
                            v.fetch(r, None)
                        } else {
                            Ok(())
                        }
                    });
                }
                if ui
                    .button("⎇ Branches")
                    .on_hover_text("Manage branches")
                    .clicked()
                {
                    state.ui.branches_popup = true;
                }
                if ui
                    .button("VCS ⌃`")
                    .on_hover_text("VCS operations (Alt+`)")
                    .clicked()
                {
                    state.ui.vcs_popup = true;
                }
                if state.ui.busy {
                    ui.spinner();
                }
            });
        });
    });
}

// ------------------------------------------------------------------ left ---

fn render_left(ui: &mut Ui, state: &mut AppState) {
    Panel::left("repositories").show(ui, |ui| {
        ui.heading("Repositories");
        if state.multi.roots.is_empty() {
            ui.label("No Git repositories detected.");
        }
        ScrollArea::vertical().show(ui, |ui| {
            let roots = state.multi.roots.clone();
            for root in &roots {
                let selected = state.selected_root.as_ref() == Some(&root.id);
                let branch = root
                    .current_branch
                    .clone()
                    .unwrap_or_else(|| "<detached>".to_string());
                let label = format!(
                    "{}  ⎇ {}\n  M:{} ?:{} !:{}",
                    root.id.name(),
                    branch,
                    root.status.modified(),
                    root.status.unversioned(),
                    root.status.ignored(),
                );
                if ui.selectable_label(selected, label).clicked() {
                    state.selected_root = Some(root.id.clone());
                    state.ui.tab = Tab::Commit;
                    // Track recently opened repos + persist (Epic J4).
                    if !state.ui.recent_repos.contains(&root.id.0) {
                        state.ui.recent_repos.insert(0, root.id.0.clone());
                        state.ui.recent_repos.truncate(12);
                    }
                    state.persist_ui();
                    if let Some(id) = &state.selected_root {
                        let id = id.clone();
                        state.fetch_log(id);
                    }
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button("＋ Branch")
                .on_hover_text("Create a new branch")
                .clicked()
            {
                state.ui.dialog = Some(Dialog::NewBranch);
            }
            if ui
                .button("⟳ Refresh")
                .on_hover_text("Rescan repositories (Ctrl+T)")
                .clicked()
            {
                state.rescan();
                if let Some(id) = &state.selected_root {
                    let id = id.clone();
                    state.fetch_log(id);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button("Init here")
                .on_hover_text("Initialize a git repository here")
                .clicked()
            {
                state.init_repo();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Clone:");
            // Bounded width: an unconstrained TextEdit here fills to the panel
            // edge and pushes the Go button past it, so the panel's persisted
            // size grows every frame (layout never settles).
            ui.add(egui::TextEdit::singleline(&mut state.clone_url).desired_width(140.0));
            if ui.button("Go").on_hover_text("Clone repository").clicked() {
                state.clone_repo();
            }
        });
    });
}

// --------------------------------------------------------------- central ---

fn render_central(ui: &mut Ui, state: &mut AppState) {
    // Ensure the log for the selected root is loaded when the Log tab is shown.
    if state.ui.tab == Tab::Log && state.selected_root.is_some() {
        let id = state.selected_root.clone().unwrap();
        if !state.log_cache.contains_key(&id) {
            state.fetch_log(id);
        }
    }

    egui::CentralPanel::default().show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(state.ui.tab == Tab::Commit, "Commit")
                .clicked()
            {
                state.ui.tab = Tab::Commit;
                state.persist_ui();
            }
            if ui
                .selectable_label(state.ui.tab == Tab::Log, "Log")
                .clicked()
            {
                state.ui.tab = Tab::Log;
                state.persist_ui();
            }
            if ui
                .selectable_label(state.ui.tab == Tab::History, "History")
                .clicked()
            {
                state.ui.tab = Tab::History;
                state.persist_ui();
            }
        });
        ui.separator();
        match state.ui.tab {
            Tab::Commit => commit_window::show(ui, state),
            Tab::Log => log_window::show_log(ui, state),
            Tab::History => log_window::show_history(ui, state),
            Tab::Settings => settings_inline(ui, state),
        }
    });
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
