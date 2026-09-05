//! Issue 33 — Manage remotes dialog (screen 13 "Manage remotes…").
//!
//! Lists the focused repo's remotes with their fetch/push URLs, and offers
//! add / rename / edit-URL / remove / set-upstream-per-branch. In multi-root
//! projects the pending change can be applied across a checked repo selection
//! with per-repo outcomes (routed through the bulk-completed pipeline).

use egui::{Align, Layout, Ui};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, RemoteRowAction};
use turbogit_domain::model::{BranchKind, RootId};
use turbogit_services::remote_service::RemoteChange;

use crate::theme::Palette;
use crate::ui::widgets::group_title;

/// The dialog window: title names the focused repo; the body edits it.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    let title = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|r| format!("Manage remotes — {}", r.id.name()))
        .unwrap_or_else(|| "Manage remotes".to_string());
    egui::Window::new(title)
        .open(&mut open)
        .show(&ctx, |ui| manage_remotes(ui, state));
    if !open {
        state.ui.dialog = None;
    }
}

/// The focused root's remotes plus the manager's controls.
fn manage_remotes(ui: &mut Ui, state: &mut AppState) {
    let Some(id) = state.selected_root.clone() else {
        ui.label("No repository selected.");
        return;
    };
    let Some(root) = state.multi.by_id(&id).cloned() else {
        ui.label("Selected repository is not registered.");
        return;
    };
    let root_path = root.path.clone();
    let remotes = root.remotes.clone();

    // REMOTES — one row per remote: name, fetch/push URLs, row actions.
    group_title(ui, "REMOTES");
    if remotes.is_empty() {
        ui.colored_label(Palette::INK_3, "No remotes configured.");
    }
    for remote in &remotes {
        let name = remote.name.clone();
        let action = state.ui.dlg.remotes_row_action.as_ref().cloned();
        let editing_this = action.as_ref().map(|(n, _)| *n == name).unwrap_or(false);
        ui.horizontal(|ui| {
            ui.strong(&name);
            match (editing_this, action) {
                (true, Some((_, RemoteRowAction::Rename))) => {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut state.ui.dlg.remotes_rename_new)
                            .desired_width(180.0),
                    );
                    edit.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::TextEdit,
                            true,
                            "New remote name",
                        )
                    });
                    if ui.button("Rename").clicked() {
                        let new = state.ui.dlg.remotes_rename_new.trim().to_string();
                        if !new.is_empty() && new != name {
                            let p = root_path.clone();
                            state.run_git(
                                format!("Rename remote {name}"),
                                Affected::Root(id.clone()),
                                move |v| {
                                    turbogit_services::remote_service::rename(v, &p, &name, &new)
                                },
                            );
                        }
                        state.ui.dlg.remotes_row_action = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.dlg.remotes_row_action = None;
                    }
                }
                (true, Some((_, RemoteRowAction::EditUrl))) => {
                    ui.label("Fetch:");
                    let f = ui.add(
                        egui::TextEdit::singleline(&mut state.ui.dlg.remotes_edit_fetch)
                            .desired_width(200.0),
                    );
                    f.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::TextEdit,
                            true,
                            "Edit fetch URL",
                        )
                    });
                    ui.label("Push:");
                    let p = ui.add(
                        egui::TextEdit::singleline(&mut state.ui.dlg.remotes_edit_push)
                            .desired_width(200.0),
                    );
                    p.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Edit push URL")
                    });
                    if ui.button("Save").clicked() {
                        let fetch = state.ui.dlg.remotes_edit_fetch.clone();
                        let push = state.ui.dlg.remotes_edit_push.clone();
                        let p = root_path.clone();
                        state.run_git(
                            format!("Update remote {name}"),
                            Affected::Root(id.clone()),
                            move |v| {
                                let fetch = if fetch.is_empty() { None } else { Some(fetch) };
                                let push = if push.is_empty() { None } else { Some(push) };
                                turbogit_services::remote_service::set_url(
                                    v,
                                    &p,
                                    &name,
                                    fetch.as_deref(),
                                    push.as_deref(),
                                )
                            },
                        );
                        state.ui.dlg.remotes_row_action = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.dlg.remotes_row_action = None;
                    }
                }
                (true, Some((_, RemoteRowAction::SetUpstream))) => {
                    ui.label("Set upstream for:");
                    let local: Vec<String> = root
                        .branches
                        .iter()
                        .filter(|b| b.kind == BranchKind::Local)
                        .map(|b| b.name.clone())
                        .collect();
                    for branch in local {
                        let upstream = format!("{name}/{branch}");
                        if ui
                            .selectable_label(false, format!("{branch} → {upstream}"))
                            .clicked()
                        {
                            let b = branch.clone();
                            let p = root_path.clone();
                            state.run_git(
                                format!("Set upstream {b} → {upstream}"),
                                Affected::Root(id.clone()),
                                move |v| {
                                    turbogit_services::remote_service::set_upstream(
                                        v, &p, &b, &upstream,
                                    )
                                },
                            );
                            state.ui.dlg.remotes_row_action = None;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.dlg.remotes_row_action = None;
                    }
                }
                _ => {
                    if ui.button("Set upstream…").clicked() {
                        state.ui.dlg.remotes_row_action =
                            Some((name.clone(), RemoteRowAction::SetUpstream));
                    }
                    if ui.button("Rename").clicked() {
                        state.ui.dlg.remotes_rename_new = name.clone();
                        state.ui.dlg.remotes_row_action =
                            Some((name.clone(), RemoteRowAction::Rename));
                    }
                    if ui.button("Edit URL").clicked() {
                        state.ui.dlg.remotes_edit_fetch =
                            remote.fetch_url.clone().unwrap_or_default();
                        state.ui.dlg.remotes_edit_push =
                            remote.push_url.clone().unwrap_or_default();
                        state.ui.dlg.remotes_row_action =
                            Some((name.clone(), RemoteRowAction::EditUrl));
                    }
                    if ui.button("Remove").clicked() {
                        let p = root_path.clone();
                        state.run_git(
                            format!("Remove remote {name}"),
                            Affected::Root(id.clone()),
                            move |v| turbogit_services::remote_service::remove(v, &p, &name),
                        );
                    }
                }
            }
        });
        ui.label(format!(
            "fetch: {}",
            remote.fetch_url.as_deref().unwrap_or("—")
        ));
        if let Some(push) = &remote.push_url {
            ui.label(format!("push: {push}"));
        }
        ui.add_space(2.0);
    }
    ui.add_space(6.0);

    // ADD REMOTE — the pending change; in multi-root projects the same form
    // drives the apply-to-selection below.
    group_title(ui, "ADD REMOTE");
    ui.horizontal(|ui| {
        ui.label("Name:");
        let n = ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.remotes_add_name)
                .hint_text("origin")
                .desired_width(140.0),
        );
        n.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Remote name")
        });
    });
    ui.horizontal(|ui| {
        ui.label("Fetch URL:");
        let f = ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.remotes_add_fetch)
                .hint_text("https://…")
                .desired_width(300.0),
        );
        f.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Remote fetch URL")
        });
    });
    ui.horizontal(|ui| {
        ui.label("Push URL (blank = fetch):");
        let p = ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.remotes_add_push)
                .hint_text("https://…")
                .desired_width(300.0),
        );
        p.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Remote push URL")
        });
    });
    let add_name = state.ui.dlg.remotes_add_name.trim().to_string();
    let add_fetch = state.ui.dlg.remotes_add_fetch.trim().to_string();
    let can_add = !add_name.is_empty() && !add_fetch.is_empty();
    if ui
        .add_enabled(can_add, egui::Button::new("Add remote"))
        .clicked()
    {
        let add_push = state.ui.dlg.remotes_add_push.trim().to_string();
        let p = root_path.clone();
        let fetch = add_fetch.clone();
        let name = add_name.clone();
        state.run_git(
            format!("Add remote {add_name}"),
            Affected::Root(id.clone()),
            move |v| {
                let url = if add_push.is_empty() {
                    fetch.clone()
                } else {
                    add_push.clone()
                };
                turbogit_services::remote_service::add(v, &p, &name, &url)
            },
        );
    }
    ui.add_space(6.0);

    // APPLY TO SELECTION — multi-root only: the pending change (add or
    // update) applied to every checked repo, reporting per-repo outcomes.
    if state.multi.roots.len() > 1 {
        group_title(ui, "APPLY TO SELECTION");
        ui.horizontal(|ui| {
            ui.selectable_label(!state.ui.dlg.remotes_apply_update, "Add")
                .clicked()
                .then(|| state.ui.dlg.remotes_apply_update = false);
            ui.selectable_label(state.ui.dlg.remotes_apply_update, "Update URL")
                .clicked()
                .then(|| state.ui.dlg.remotes_apply_update = true);
        });
        ui.horizontal(|ui| {
            ui.label("Remote:");
            ui.label(egui::RichText::new(&add_name).monospace());
        });
        let update = state.ui.dlg.remotes_apply_update;
        for r in &state.multi.roots {
            let mut checked = state.ui.dlg.remotes_apply_scope.contains(&r.id);
            if ui.checkbox(&mut checked, r.id.name()).changed() {
                if checked {
                    state.ui.dlg.remotes_apply_scope.insert(r.id.clone());
                } else {
                    state.ui.dlg.remotes_apply_scope.remove(&r.id);
                }
            }
        }
        let apply = ui.add_enabled(
            !state.ui.dlg.remotes_apply_scope.is_empty() && can_add,
            egui::Button::new(if update { "Apply update" } else { "Apply add" }),
        );
        if apply.clicked() {
            let scope: Vec<RootId> = state.ui.dlg.remotes_apply_scope.iter().cloned().collect();
            let change = if update {
                RemoteChange::SetUrl {
                    name: add_name.clone(),
                    fetch_url: Some(add_fetch.clone()),
                    push_url: Some(state.ui.dlg.remotes_add_push.trim().to_string()),
                }
            } else {
                RemoteChange::Add {
                    name: add_name.clone(),
                    url: add_fetch.clone(),
                }
            };
            state.apply_remote_change(scope, change);
        }
        if let Some(results) = &state.ui.dlg.remotes_apply_results {
            ui.add_space(4.0);
            for (name, result) in results {
                match result {
                    Ok(()) => {
                        ui.colored_label(Palette::STATE_SUCCESS, format!("✓ {name}"));
                    }
                    Err(msg) => {
                        ui.horizontal(|ui| {
                            ui.colored_label(Palette::STATE_ERROR, format!("✗ {name}"));
                            ui.colored_label(Palette::STATE_ERROR, msg);
                        });
                    }
                }
            }
        }
        ui.add_space(6.0);
    }

    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.ui.dialog = None;
            }
        });
    });
}
