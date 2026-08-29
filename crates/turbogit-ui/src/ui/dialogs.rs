//! Modal dialogs: Merge (F1/F2), Rebase (F3/F4), Interactive rebase (F5),
//! New Branch (E3), Tag (O1–O4), Shelve (J1–J4), Stash (J5–J8). The Push
//! dialog lives in [`super::push_dialog`] (issue #20).

use egui::Ui;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{MergeOpts, RebaseAction, RebaseOpts};
use turbogit_services::{branch_service, history_editor, integrate_service, shelve_stash};

pub fn show(ui: &mut Ui, state: &mut AppState, dialog: Dialog) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    let title = match dialog {
        Dialog::NewBranch => "New Branch",
        Dialog::Merge => "Merge",
        Dialog::Rebase => "Rebase",
        Dialog::InteractiveRebase => "Interactive Rebase",
        Dialog::Tag => "Tag",
        Dialog::Shelve => "Shelve",
        Dialog::Stash => "Stash",
        // Push is rendered by `push_dialog` (issue #20); `ui::render` routes
        // it there before this function is ever called.
        _ => return,
    };
    egui::Window::new(title)
        .open(&mut open)
        .show(&ctx, |ui| match dialog {
            Dialog::NewBranch => new_branch(ui, state),
            Dialog::Merge => merge(ui, state),
            Dialog::Rebase => rebase(ui, state),
            Dialog::InteractiveRebase => interactive_rebase(ui, state),
            Dialog::Tag => tag(ui, state),
            Dialog::Shelve => shelve(ui, state),
            Dialog::Stash => stash(ui, state),
            _ => {}
        });
    if !open {
        state.ui.dialog = None;
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
}

fn new_branch(ui: &mut Ui, state: &mut AppState) {
    ui.label("Name:");
    ui.text_edit_singleline(&mut state.ui.dlg.new_branch_name);
    ui.label("Start point (blank = current HEAD):");
    ui.text_edit_singleline(&mut state.ui.dlg.new_branch_start);
    ui.checkbox(
        &mut state.ui.dlg.new_branch_checkout,
        "Checkout after create",
    );
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() {
            let root = state.selected_path();
            let name = state.ui.dlg.new_branch_name.clone();
            let start = if state.ui.dlg.new_branch_start.trim().is_empty() {
                None
            } else {
                Some(state.ui.dlg.new_branch_start.clone())
            };
            let co = state.ui.dlg.new_branch_checkout;
            state.run_git(
                format!("Create branch {name}"),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        branch_service::create(v, r, &name, start.as_deref(), co)
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn merge(ui: &mut Ui, state: &mut AppState) {
    ui.label("Merge into current branch from:");
    ui.text_edit_singleline(&mut state.ui.dlg.merge_target);
    ui.checkbox(&mut state.ui.dlg.merge_no_ff, "No fast-forward (--no-ff)");
    ui.checkbox(&mut state.ui.dlg.merge_squash, "Squash");
    ui.checkbox(&mut state.ui.dlg.merge_no_commit, "No commit");
    ui.checkbox(
        &mut state.ui.dlg.merge_no_verify,
        "Skip hooks (--no-verify)",
    );
    ui.horizontal(|ui| {
        if ui.button("Merge").clicked() {
            let root = state.selected_path();
            let target = state.ui.dlg.merge_target.clone();
            let opts = MergeOpts {
                no_ff: state.ui.dlg.merge_no_ff,
                squash: state.ui.dlg.merge_squash,
                no_commit: state.ui.dlg.merge_no_commit,
                no_verify: state.ui.dlg.merge_no_verify,
                ..Default::default()
            };
            let clean = state.settings.clean_tree_method;
            state.run_git(
                format!("Merge {target}"),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        integrate_service::smart_merge(v, r, &target, &opts, clean)
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn rebase(ui: &mut Ui, state: &mut AppState) {
    ui.label("Rebase current branch onto:");
    ui.text_edit_singleline(&mut state.ui.dlg.rebase_onto);
    ui.checkbox(&mut state.ui.dlg.rebase_merges, "--rebase-merges");
    ui.checkbox(&mut state.ui.dlg.rebase_keep_empty, "--keep-empty");
    ui.checkbox(&mut state.ui.dlg.rebase_update_refs, "--update-refs");
    ui.checkbox(&mut state.ui.dlg.rebase_autosquash, "--autosquash");
    ui.horizontal(|ui| {
        if ui.button("Rebase").clicked() {
            let root = state.selected_path();
            let onto = state.ui.dlg.rebase_onto.clone();
            let onto_arg = onto.clone();
            let opts = RebaseOpts {
                onto: if onto.trim().is_empty() {
                    None
                } else {
                    Some(onto)
                },
                rebase_merges: state.ui.dlg.rebase_merges,
                keep_empty: state.ui.dlg.rebase_keep_empty,
                update_refs: state.ui.dlg.rebase_update_refs,
                autosquash: state.ui.dlg.rebase_autosquash,
                ..Default::default()
            };
            state.run_git(
                "Rebase".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        integrate_service::rebase(v, r, &onto_arg, &opts)
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn interactive_rebase(ui: &mut Ui, state: &mut AppState) {
    // Build the plan on first open from the selected commit (rebase its
    // ancestors up to HEAD).
    if state.ui.dlg.rebase_plan.is_none()
        && let Some(cid) = &state.ui.selected_commit
        && let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
        && let Some(commit) = root
            .branches
            .first()
            .map(|_| ())
            .and(root.head.clone())
            .and_then(|_| state.caches.log(id))
            .and_then(|cs| cs.iter().find(|c| &c.id == cid))
    {
        let base = commit.parents.first().cloned();
        if let Some(base) = base {
            state.ui.dlg.rebase_base = Some(base.clone());
            if let Ok(plan) = history_editor::build_plan(state.executor.as_ref(), &id.0, &base) {
                state.ui.dlg.rebase_plan = Some(plan);
            }
        }
    }

    {
        let plan = match &mut state.ui.dlg.rebase_plan {
            Some(p) => p,
            None => {
                ui.label("Select a commit in the Log tab first, then open this dialog.");
                if ui.button("Close").clicked() {
                    close(state);
                }
                return;
            }
        };
        ui.label("Reorder / change actions, then Start. (Drop removes the commit.)");
        egui::ScrollArea::vertical().show(ui, |ui| {
            let n = plan.len();
            for i in 0..n {
                ui.horizontal(|ui| {
                    let actions = [
                        ("pick", RebaseAction::Pick),
                        ("reword", RebaseAction::Reword),
                        ("edit", RebaseAction::Edit),
                        ("squash", RebaseAction::Squash),
                        ("fixup", RebaseAction::Fixup),
                        ("drop", RebaseAction::Drop),
                    ];
                    for (lbl, act) in actions {
                        if ui.selectable_label(plan[i].action == act, lbl).clicked() {
                            plan[i].action = act;
                        }
                    }
                    ui.label(format!(
                        "{}  {}",
                        &plan[i].commit[..7.min(plan[i].commit.len())],
                        plan[i].subject
                    ));
                    if ui.button("↑").clicked() && i > 0 {
                        history_editor::reorder(plan, i, i - 1);
                    }
                    if ui.button("↓").clicked() && i + 1 < n {
                        history_editor::reorder(plan, i, i + 1);
                    }
                });
            }
        });
    }

    ui.horizontal(|ui| {
        if ui.button("Start Rebase").clicked() {
            let root = state.selected_path();
            let plan2 = state.ui.dlg.rebase_plan.clone().unwrap_or_default();
            state.run_git(
                "Interactive rebase".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        history_editor::execute(v, r, &plan2)
                    } else {
                        Ok(())
                    }
                },
            );
            state.ui.dlg.rebase_plan = None;
            close(state);
        }
        if ui.button("Cancel").clicked() {
            state.ui.dlg.rebase_plan = None;
            close(state);
        }
    });
}

fn tag(ui: &mut Ui, state: &mut AppState) {
    ui.label("Name:");
    ui.text_edit_singleline(&mut state.ui.dlg.tag_name);
    ui.label("Message (blank = lightweight):");
    ui.text_edit_singleline(&mut state.ui.dlg.tag_msg);
    ui.checkbox(&mut state.ui.dlg.tag_push, "Push tag after create");
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() {
            let root = state.selected_path();
            let name = state.ui.dlg.tag_name.clone();
            let msg = state.ui.dlg.tag_msg.clone();
            let push = state.ui.dlg.tag_push;
            state.run_git(
                format!("Create tag {name}"),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        v.tag_create(r, &name, if msg.is_empty() { None } else { Some(&msg) })?;
                        if push {
                            let remote = v
                                .remotes(r)?
                                .first()
                                .map(|x| x.name.clone())
                                .unwrap_or_else(|| "origin".into());
                            v.tag_push(r, &remote, Some(&name), false)?;
                        }
                        Ok(())
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn shelve(ui: &mut Ui, state: &mut AppState) {
    ui.label("Shelf name:");
    ui.text_edit_singleline(&mut state.ui.dlg.shelve_name);
    ui.horizontal(|ui| {
        if ui.button("Shelve selected").clicked() {
            let name = state.ui.dlg.shelve_name.clone();
            let mut changes = Vec::new();
            if let Some(id) = &state.selected_root
                && let Some(root) = state.multi.by_id(id)
            {
                for c in &root.status.changes {
                    if state.ui.selected.contains(&c.path) {
                        changes.push(c.clone());
                    }
                }
            }
            let shelf = shelve_stash::make_shelf(&name, &changes);
            state.ui.shelves.push(shelf);
            let _ = shelve_stash::save_shelves(&state.project_dir, &state.ui.shelves);
            let root = state.selected_path();
            state.run_git(
                "Shelve".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        // Stash the working changes so they are parked.
                        v.stash_push(r, &name, false)
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn stash(ui: &mut Ui, state: &mut AppState) {
    ui.label("Message:");
    ui.text_edit_singleline(&mut state.ui.dlg.stash_msg);
    ui.checkbox(&mut state.ui.dlg.stash_keep, "Keep index (--keep-index)");
    ui.horizontal(|ui| {
        if ui.button("Stash").clicked() {
            let root = state.selected_path();
            let msg = state.ui.dlg.stash_msg.clone();
            let keep = state.ui.dlg.stash_keep;
            state.run_git(
                "Stash".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        v.stash_push(r, &msg, keep)
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Pop latest").clicked() {
            let root = state.selected_path();
            state.run_git(
                "Stash pop".into(),
                Affected::from_optional_root(root.as_deref()),
                move |v| {
                    if let Some(r) = &root {
                        v.stash_pop(r, 0)
                    } else {
                        Ok(())
                    }
                },
            );
            close(state);
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}
