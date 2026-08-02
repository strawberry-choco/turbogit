//! Commit tool window (C1–C15): changelists, staging, partial commit, amend,
//! Commit & Push, recent messages, discard. Realized as the Commit tab.

use crate::core::changes;
use crate::model::Change;
use crate::state::{AppState, Dialog, PendingConfirm};
use egui::{Color32, Ui};

/// Collect the `Change` objects whose path is in the current selection.
fn selected_changes(state: &AppState) -> Vec<Change> {
    let mut out = Vec::new();
    if let Some(id) = &state.selected_root {
        if let Some(root) = state.multi.by_id(id) {
            for c in &root.status.changes {
                if state.ui.selected.contains(&c.path) {
                    out.push(c.clone());
                }
            }
        }
    }
    out
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Commit");

    // Recent commit messages.
    if !state.ui.recent_messages.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label("Recent:");
            for m in state.ui.recent_messages.iter().take(6) {
                let short = if m.len() > 22 { &m[..22] } else { m };
                if ui.button(short).clicked() {
                    state.ui.commit_message = m.clone();
                }
            }
        });
        ui.separator();
    }

    ui.label("Commit message:");
    ui.text_edit_multiline(&mut state.ui.commit_message);

    // Subject-length guidance (Epic C5).
    let subject = state.ui.commit_message.lines().next().unwrap_or("");
    let slen = subject.chars().count();
    ui.horizontal(|ui| {
        ui.label(format!("Subject: {slen}/50"));
        if slen > 50 {
            ui.colored_label(Color32::from_rgb(230, 120, 110), "(keep ≤ 50)");
        }
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.ui.amend, "Amend");
        if ui.button("Template").clicked() {
            let tpl = state.vcs.settings.commit_template.clone();
            if tpl.is_empty() {
                state.ui.toast = Some("No commit template configured.".into());
            } else if let Ok(content) = std::fs::read_to_string(&tpl) {
                state.ui.commit_message = content;
            } else {
                state.ui.toast = Some(format!("Could not read template: {tpl}"));
            }
        }
        if ui.button("Clear").clicked() {
            state.ui.commit_message.clear();
            state.ui.selected.clear();
        }
    });

    ui.separator();
    ui.label("Changes (check to include in commit):");

    let id = state.selected_root.clone();
    if let Some(root) = id.as_ref().and_then(|id| state.multi.by_id(id)) {
        let changes = root.status.changes.clone();
        let unstaged: Vec<Change> = changes.iter().filter(|c| !c.staged).cloned().collect();
        let staged: Vec<Change> = changes.iter().filter(|c| c.staged).cloned().collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.collapsing(format!("Unstaged ({})", unstaged.len()), |ui| {
                if unstaged.is_empty() {
                    ui.colored_label(egui::Color32::GRAY, "Nothing to stage");
                }
                for c in &unstaged {
                    render_change_row(ui, state, c);
                }
            });
            ui.collapsing(format!("Staged ({})", staged.len()), |ui| {
                if staged.is_empty() {
                    ui.colored_label(egui::Color32::GRAY, "No staged changes");
                }
                for c in &staged {
                    render_change_row(ui, state, c);
                }
            });
        });
    } else {
        ui.colored_label(egui::Color32::GRAY, "Select a repository to see changes.");
    }

    ui.horizontal(|ui| {
        if ui.button("Stage selected").on_hover_text("Move selected files into the staging area").clicked() {
            let ch = selected_changes(state);
            let root = state.selected_path();
            state.run_git("Stage".into(), move |v| {
                if let Some(r) = &root {
                    changes::stage_selected(v, r, &ch)
                } else {
                    Ok(())
                }
            });
        }
        if ui.button("Unstage selected").on_hover_text("Remove selected files from the staging area").clicked() {
            let ch = selected_changes(state);
            let root = state.selected_path();
            state.run_git("Unstage".into(), move |v| {
                if let Some(r) = &root {
                    changes::unstage_selected(v, r, &ch)
                } else {
                    Ok(())
                }
            });
        }
        if ui.button("Discard").on_hover_text("Discard working-tree changes (irreversible)").clicked() {
            let ch = selected_changes(state);
            if ch.is_empty() {
                state.ui.toast = Some("Select files to discard.".into());
            } else {
                // Gate behind confirmation (Epic C8) — destructive, irreversible.
                state.ui.confirm = Some(PendingConfirm::Discard { changes: ch });
            }
        }
    });

    ui.separator();
    ui.horizontal(|ui| {
        let can_commit = !state.ui.commit_message.trim().is_empty();
        if ui
            .add_enabled(can_commit, egui::Button::new("Commit"))
            .on_hover_text("Commit selected changes")
            .clicked()
        {
            do_commit(state, false);
        }
        if ui
            .add_enabled(can_commit, egui::Button::new("Commit & Push"))
            .on_hover_text("Commit then push to remote")
            .clicked()
        {
            do_commit(state, true);
        }
        if ui.button("Shelve…").on_hover_text("Shelve selected changes").clicked() {
            state.ui.dialog = Some(Dialog::Shelve);
        }
        if ui.button("Stash…").on_hover_text("Stash all changes").clicked() {
            state.ui.dialog = Some(Dialog::Stash);
        }
    });

    if let Some(err) = &state.last_error {
        ui.separator();
        ui.colored_label(egui::Color32::RED, format!("⚠ {err}"));
    }

    // Inline per-file diff preview (Epic C3).
    if let Some(path) = state.ui.preview_change.clone() {
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Preview diff");
            if ui.small_button("Close").clicked() {
                state.ui.preview_change = None;
            }
        });
        crate::ui::diff::render_diff(ui, state, &None, &None, &Some(path));
    }

    // Merge-conflict resolution (G1–G9) appears inline in the Commit tab.
    crate::ui::conflicts::render(ui, state);
}

/// One change row: checkbox (include-in-selection) + inline diff button (Epic C3).
fn render_change_row(ui: &mut Ui, state: &mut AppState, c: &Change) {
    let mut sel = state.ui.selected.contains(&c.path);
    let label = format!(
        "[{stat}] {path}",
        stat = c.status.short(),
        path = c.path.display()
    );
    ui.horizontal(|ui| {
        if ui.checkbox(&mut sel, label).clicked() {
            if sel {
                state.ui.selected.insert(c.path.clone());
            } else {
                state.ui.selected.remove(&c.path);
            }
        }
        if ui
            .small_button("Diff")
            .on_hover_text("Preview file diff")
            .clicked()
        {
            state.ui.preview_change = Some(c.path.clone());
        }
    });
}

fn do_commit(state: &mut AppState, and_push: bool) {
    let root = state.selected_path();
    let changes = selected_changes(state);
    let msg = state.ui.commit_message.clone();
    let amend = state.ui.amend;
    let recent = msg.clone();
    state.run_git("Commit".into(), move |v| {
        if let Some(r) = &root {
            let _ = changes::commit_selected(v, r, &msg, &changes, amend)?;
            Ok(())
        } else {
            Ok(())
        }
    });
    // Record recent message + reset fields.
    if !state.ui.recent_messages.contains(&recent) {
        state.ui.recent_messages.insert(0, recent);
        state.ui.recent_messages.truncate(12);
    }
    state.ui.commit_message.clear();
    state.ui.selected.clear();
    state.persist_ui();
    if and_push {
        state.ui.dialog = Some(Dialog::Push);
    }
}
