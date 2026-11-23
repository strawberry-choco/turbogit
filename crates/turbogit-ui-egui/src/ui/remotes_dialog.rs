use egui::Ui;
use turbogit_app::shell_writes::remote as writes;
use turbogit_app::state::{AppState, RemoteRowAction};
use turbogit_domain::model::{BranchKind, Root};

use crate::ui::widgets;
use crate::ui::widgets::group_title;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Manage remotes")
        .open(&mut open)
        // A near-minimal seed makes the first frame measure the content; egui's
        // default 420 outer height would otherwise clip the footer below the
        // selector band and the per-root remote lists.
        .resizable(false)
        .default_height(32.0)
        .show(&ctx, |ui| {
            super::dialogs::repo_scope_band(ui, state);
            manage_remotes(ui, state);
        });
    if !open {
        state.ui.dialog = None;
    }
}

fn manage_remotes(ui: &mut Ui, state: &mut AppState) {
    if state.multi.roots.is_empty() {
        widgets::empty_state(ui, "No repository registered.");
        return;
    }
    group_title(ui, "REMOTES");
    let roots: Vec<Root> = state.multi.roots.clone();
    for root in &roots {
        group_title(ui, &root.id.name());
        if root.remotes.is_empty() {
            widgets::empty_state(ui, "No remotes configured.");
        }
        for remote in &root.remotes {
            remote_row(ui, state, root, remote);
        }
        ui.add_space(4.0);
    }
    ui.add_space(6.0);

    group_title(ui, "ADD REMOTE");
    ui.label("Added to every repository in scope.");
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
    let add = ui
        .scope(|ui| {
            if !can_add || state.ui.dlg.repo_scope.is_empty() {
                ui.disable();
            }
            widgets::compact_button(ui, "Add remote")
        })
        .inner;
    if add.clicked() {
        let add_push = state.ui.dlg.remotes_add_push.trim().to_string();
        for id in state.ui.dlg.repo_scope.clone() {
            if let Some(root) = state.multi.by_id(&id) {
                state.dispatch(writes::add(
                    &id,
                    root.path.as_path(),
                    &add_name,
                    &add_fetch,
                    &add_push,
                ));
            }
        }
    }
    ui.add_space(6.0);

    widgets::dialog_footer(ui, |ui| {
        if widgets::compact_button(ui, "Close").clicked() {
            state.ui.dialog = None;
        }
    });
}

fn remote_row(
    ui: &mut Ui,
    state: &mut AppState,
    root: &Root,
    remote: &turbogit_domain::model::Remote,
) {
    let id = root.id.clone();
    let root_path = root.path.clone();
    let name = remote.name.clone();
    let action = state.ui.dlg.remotes_row_action.as_ref().cloned();
    let editing_this = action
        .as_ref()
        .is_some_and(|(row_root, n, _)| row_root == &id && *n == name);
    ui.horizontal(|ui| {
        ui.strong(&name);
        match (editing_this, action) {
            (true, Some((_, _, RemoteRowAction::Rename))) => {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.ui.dlg.remotes_rename_new)
                        .desired_width(180.0),
                );
                edit.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "New remote name")
                });
                if widgets::compact_button(ui, "Rename").clicked() {
                    let new = state.ui.dlg.remotes_rename_new.trim().to_string();
                    if !new.is_empty() && new != name {
                        state.dispatch(writes::rename(&id, root_path.as_path(), &name, &new));
                    }
                    state.ui.dlg.remotes_row_action = None;
                }
                if widgets::compact_button(ui, "Cancel").clicked() {
                    state.ui.dlg.remotes_row_action = None;
                }
            }
            (true, Some((_, _, RemoteRowAction::EditUrl))) => {
                ui.label("Fetch:");
                let f = ui.add(
                    egui::TextEdit::singleline(&mut state.ui.dlg.remotes_edit_fetch)
                        .desired_width(200.0),
                );
                f.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Edit fetch URL")
                });
                ui.label("Push:");
                let p = ui.add(
                    egui::TextEdit::singleline(&mut state.ui.dlg.remotes_edit_push)
                        .desired_width(200.0),
                );
                p.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Edit push URL")
                });
                if widgets::compact_button(ui, "Save").clicked() {
                    let fetch = state.ui.dlg.remotes_edit_fetch.clone();
                    let push = state.ui.dlg.remotes_edit_push.clone();
                    state.dispatch(writes::update(
                        &id,
                        root_path.as_path(),
                        &name,
                        &fetch,
                        &push,
                    ));
                    state.ui.dlg.remotes_row_action = None;
                }
                if widgets::compact_button(ui, "Cancel").clicked() {
                    state.ui.dlg.remotes_row_action = None;
                }
            }
            (true, Some((_, _, RemoteRowAction::SetUpstream))) => {
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
                        state.dispatch(writes::set_upstream(
                            &id,
                            root_path.as_path(),
                            &branch,
                            &upstream,
                        ));
                        state.ui.dlg.remotes_row_action = None;
                    }
                }
                if widgets::compact_button(ui, "Cancel").clicked() {
                    state.ui.dlg.remotes_row_action = None;
                }
            }
            _ => {
                if widgets::compact_button(ui, "Set upstream…").clicked() {
                    state.ui.dlg.remotes_row_action =
                        Some((id.clone(), name.clone(), RemoteRowAction::SetUpstream));
                }
                if widgets::compact_button(ui, "Rename").clicked() {
                    state.ui.dlg.remotes_rename_new = name.clone();
                    state.ui.dlg.remotes_row_action =
                        Some((id.clone(), name.clone(), RemoteRowAction::Rename));
                }
                if widgets::compact_button(ui, "Edit URL").clicked() {
                    state.ui.dlg.remotes_edit_fetch = remote.fetch_url.clone().unwrap_or_default();
                    state.ui.dlg.remotes_edit_push = remote.push_url.clone().unwrap_or_default();
                    state.ui.dlg.remotes_row_action =
                        Some((id.clone(), name.clone(), RemoteRowAction::EditUrl));
                }
                if widgets::compact_button(ui, "Remove").clicked() {
                    state.dispatch(writes::remove(&id, root_path.as_path(), &name));
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
