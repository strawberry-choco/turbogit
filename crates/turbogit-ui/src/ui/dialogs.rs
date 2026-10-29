use egui::{Align, FontFamily, FontId, Layout, RichText, Ui};
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, TagType, UnmergedCommits};
use turbogit_app::{integrate_service, shelve_stash, sync_service, tag_service};
use turbogit_domain::model::{BranchKind, DisplayName, MergeStrategy};

use crate::theme::{Palette, TYPE_BODY, TYPE_CONTROL, data_font};
use crate::ui::components::{KitButton, counted, kit_button};
use crate::ui::icons::{Icon, icon};
use crate::ui::widgets::{self, group_title};

pub fn show(ui: &mut Ui, state: &mut AppState, dialog: Dialog) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    let title = match dialog {
        Dialog::NewBranch => "New Branch".to_string(),
        Dialog::Merge => format!(
            "Merge into {}",
            current_branch_name(state).unwrap_or_else(|| "…".to_string())
        ),
        Dialog::Rebase => format!(
            "Rebase {}",
            current_branch_name(state).unwrap_or_else(|| "…".to_string())
        ),
        Dialog::Tag => {
            let name = state.ui.dlg.tag_name.trim();
            if name.is_empty() {
                "Create tag".to_string()
            } else {
                format!("Create tag {name}")
            }
        }
        Dialog::Shelve => "Shelve".to_string(),
        Dialog::ImportPatches => "Import Patches".to_string(),
        Dialog::Stash => "Stash".to_string(),
        Dialog::NewWorktree => "New worktree".to_string(),
        Dialog::CherryPickTarget => "Cherry-pick to branch".to_string(),
        Dialog::RenameBranch => "Rename branch".to_string(),
        Dialog::BranchNotFullyMerged => "Branch Was Not Fully Merged".to_string(),
        Dialog::CompareBranches => {
            format!(
                "Compare {} with {}",
                state.ui.dlg.compare_left, state.ui.dlg.compare_right
            )
        }
        _ => return,
    };
    egui::Window::new(title)
        .open(&mut open)
        .show(&ctx, |ui| match dialog {
            Dialog::NewBranch => new_branch(ui, state),
            Dialog::Merge => merge(ui, state),
            Dialog::Rebase => rebase(ui, state),
            Dialog::Tag => tag(ui, state),
            Dialog::Shelve => shelve(ui, state),
            Dialog::ImportPatches => import_patches(ui, state),
            Dialog::Stash => stash(ui, state),
            Dialog::NewWorktree => super::worktrees::new_worktree_dialog(ui, state),
            Dialog::CherryPickTarget => cherry_pick_target(ui, state),
            Dialog::RenameBranch => rename_branch(ui, state),
            Dialog::BranchNotFullyMerged => branch_not_fully_merged(ui, state),
            Dialog::CompareBranches => compare_branches(ui, state),
            _ => {}
        });
    if !open {
        state.ui.dialog = None;
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
}

fn cherry_pick_target(ui: &mut Ui, state: &mut AppState) {
    ui.label("Apply the selected commit onto which branch?");
    if state.ui.dlg.cherry_pick_commit.is_none() {
        widgets::empty_state(ui, "No commit selected.");
        widgets::dialog_footer(ui, |ui| {
            if ui.button("Cancel").clicked() {
                close(state);
            }
        });
        return;
    }

    let local: Vec<String> = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|r| {
            r.branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .map(|b| b.name.clone())
                .collect()
        })
        .unwrap_or_default();
    if local.is_empty() {
        widgets::empty_state(ui, "No local branches.");
    }
    let current = current_branch_name(state);
    for name in local {
        let (label, reason) = if current.as_deref() == Some(name.as_str()) {
            (
                format!("{name} (current)"),
                "this is the current branch".to_owned(),
            )
        } else if sync_service::is_protected(&state.settings, &name) {
            (
                format!("{name} (protected)"),
                format!("'{name}' is a protected branch"),
            )
        } else {
            (name.clone(), String::new())
        };
        let row = ui
            .add_enabled(reason.is_empty(), egui::Button::new(label))
            .on_disabled_hover_text(reason);
        if row.clicked() {
            if let Some(commit) = state.ui.dlg.cherry_pick_commit.take() {
                state.cherry_pick_to(commit, name.clone());
            }
            close(state);
        }
    }
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn new_branch(ui: &mut Ui, state: &mut AppState) {
    if state.ui.dlg.new_branch_base == NewBranchBase::Unset
        && let Some(cur) = current_branch_name(state)
    {
        state.ui.dlg.new_branch_base = NewBranchBase::Branch(cur);
    }
    ui.label("Name:");
    ui.text_edit_singleline(&mut state.ui.dlg.new_branch_name);
    ui.label("Start from:");
    ui.horizontal(|ui| {
        new_branch_base_value(ui, state);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::compact_button(ui, "Change…").clicked() {
                state.ui.dlg.new_branch_base_picker_open =
                    !state.ui.dlg.new_branch_base_picker_open;
            }
        });
    });
    if state.ui.dlg.new_branch_base_picker_open {
        let branches: Vec<String> = state
            .selected_root
            .as_ref()
            .and_then(|id| state.multi.by_id(id))
            .map(|r| {
                r.branches
                    .iter()
                    .filter(|b| b.kind == BranchKind::Local)
                    .map(|b| b.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        for b in branches {
            if ui
                .selectable_label(
                    state.ui.dlg.new_branch_base == NewBranchBase::Branch(b.clone()),
                    &b,
                )
                .clicked()
            {
                state.ui.dlg.new_branch_base = NewBranchBase::Branch(b);
                state.ui.dlg.new_branch_base_picker_open = false;
            }
        }
    }
    ui.checkbox(
        &mut state.ui.dlg.new_branch_checkout,
        "Switch to the new branch now",
    );
    widgets::dialog_footer(ui, |ui| {
        if widgets::compact_button(ui, "Cancel").clicked() {
            close(state);
        }
        if widgets::compact_button(ui, "Create").clicked() {
            let root = state.selected_root.clone();
            let name = state.ui.dlg.new_branch_name.clone();
            let start = state
                .ui
                .dlg
                .new_branch_base
                .start_point()
                .map(str::to_owned);
            let co = state.ui.dlg.new_branch_checkout;
            state.ui.branches_tree.scroll_to = Some(name.clone());
            // `scroll_to` is consumed the frame it resolves, so the reveal has to lift the fold too.
            if let Some(root) = &root {
                state.ui.branches_tree.collapsed_repos.remove(root);
            }
            if let Some(root) = root {
                state.dispatch(shell_writes::branch::create(
                    &root,
                    &name,
                    start.as_deref(),
                    co,
                ));
            }
            close(state);
        }
    });
}

fn new_branch_base_value(ui: &mut Ui, state: &AppState) {
    let base = &state.ui.dlg.new_branch_base;
    let NewBranchBase::Commit(id) = base else {
        if let NewBranchBase::Branch(name) = base {
            ui.label(name);
        }
        return;
    };
    icon(ui, Icon::GIT_COMMIT, 14.0, Palette::BRAND);
    ui.label(
        RichText::new(widgets::short_commit_ref(id))
            .font(data_font(TYPE_BODY))
            .color(Palette::INK),
    );
    if let Some(subject) = commit_subject(state, id) {
        ui.add(
            egui::Label::new(
                RichText::new(subject)
                    .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                    .color(Palette::INK_3),
            )
            .truncate(),
        );
    }
}

fn commit_subject(state: &AppState, cid: &str) -> Option<String> {
    let root = state.selected_root.as_ref()?;
    let commits = state
        .caches
        .log(root)
        .or_else(|| {
            state
                .ui
                .log_ref_scope
                .as_ref()
                .and_then(|(r, name)| state.caches.ref_log(r, name))
        })
        .or_else(|| {
            state
                .ui
                .log_path_scope
                .as_ref()
                .and_then(|path| state.caches.path_log(root, path))
        })?;
    commits
        .iter()
        .find(|c| c.id == cid)
        .map(|c| c.subject().to_owned())
}

fn current_branch_name(state: &AppState) -> Option<String> {
    let id = state.selected_root.as_ref()?;
    state.multi.by_id(id)?.current_branch.clone()
}

fn merge(ui: &mut Ui, state: &mut AppState) {
    let branches: Vec<String> = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|r| {
            r.branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .map(|b| b.name.clone())
                .collect()
        })
        .unwrap_or_default();

    group_title(ui, "SOURCE BRANCH");
    let target_empty = state.ui.dlg.merge_target.trim().is_empty();
    ui.horizontal(|ui| {
        icon(ui, Icon::GIT_BRANCH, 14.0, Palette::BRAND);
        if target_empty {
            ui.label(
                egui::RichText::new("Select source branch…")
                    .monospace()
                    .color(Palette::INK_3),
            );
        } else {
            ui.label(
                egui::RichText::new(state.ui.dlg.merge_target.clone())
                    .monospace()
                    .color(Palette::INK),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Change…").clicked() {
                state.ui.dlg.merge_source_picker_open = !state.ui.dlg.merge_source_picker_open;
            }
        });
    });
    if state.ui.dlg.merge_source_picker_open {
        for name in &branches {
            if ui
                .selectable_label(&state.ui.dlg.merge_target == name, name.clone())
                .clicked()
            {
                state.ui.dlg.merge_target = name.clone();
                state.ui.dlg.merge_source_picker_open = false;
                state.ui.dlg.merge_preview = None;
            }
        }
    }
    ui.add_space(4.0);

    group_title(ui, "STRATEGY");
    ui.horizontal(|ui| {
        for (label, strategy) in [
            ("No-commit", MergeStrategy::NoCommit),
            ("Commit", MergeStrategy::Commit),
            ("Squash", MergeStrategy::Squash),
            ("Fast-forward", MergeStrategy::FastForward),
        ] {
            if ui
                .selectable_label(state.ui.dlg.merge_strategy == strategy, label)
                .clicked()
            {
                state.ui.dlg.merge_strategy = strategy;
                state.ui.dlg.merge_preview = None;
            }
        }
    });
    ui.add_space(4.0);

    group_title(ui, "OPTIONS");
    ui.checkbox(&mut state.ui.dlg.merge_no_ff, "No fast-forward (--no-ff)");
    ui.checkbox(
        &mut state.ui.dlg.merge_verify_signatures,
        "Verify signatures on incoming",
    );
    ui.checkbox(
        &mut state.ui.dlg.merge_allow_unrelated,
        "Allow unrelated histories (--allow-unrelated-histories)",
    );
    ui.checkbox(
        &mut state.ui.dlg.merge_no_verify,
        "Skip hooks (--no-verify)",
    );
    ui.add_space(6.0);

    let target = state.ui.dlg.merge_target.trim().to_string();
    if !target.is_empty() {
        let key = (target.clone(), state.ui.dlg.merge_strategy);
        if state.ui.dlg.merge_preview_key.as_ref() != Some(&key) {
            let preview = state
                .selected_root
                .as_ref()
                .and_then(|root| state.merge_preview(root, &target, key.1));
            state.ui.dlg.merge_preview = preview;
            state.ui.dlg.merge_preview_key = Some(key);
        }
        if let Some(preview) = state.ui.dlg.merge_preview {
            preview_box(ui, preview, state.ui.dlg.merge_strategy);
        }
    }
    ui.add_space(6.0);

    let siblings = state.merge_cascade_siblings();
    if !siblings.is_empty() {
        cascade_banner(ui, state, siblings.len());
        ui.add_space(4.0);
    }

    widgets::dialog_footer(ui, |ui| {
        {
            let can_merge = !target.is_empty();
            let merge_btn = ui.add_enabled(can_merge, egui::Button::new("Merge"));
            if merge_btn.clicked() {
                let root = state.selected_root.clone();
                let target = state.ui.dlg.merge_target.clone();
                let opts = state.merge_dialog_opts();
                let clean = state.settings.clean_tree_method;
                let commits = state
                    .ui
                    .dlg
                    .merge_preview
                    .as_ref()
                    .map(|p| p.merge_commits)
                    .unwrap_or(0);
                if let Some(root) = root {
                    state.dispatch(Operation::Merge {
                        root,
                        target,
                        opts,
                        clean,
                        commits,
                    });
                }
                close(state);
            }
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn preview_box(ui: &mut Ui, preview: integrate_service::MergePreview, strategy: MergeStrategy) {
    widgets::note(ui, None, |ui| {
        if preview.files == 0 && preview.insertions == 0 && preview.deletions == 0 {
            ui.strong("Already up to date");
            return;
        }
        let commits_line = match strategy {
            MergeStrategy::FastForward => "Will fast-forward — no merge commit".to_string(),
            _ => format!(
                "Will create {}",
                counted(preview.merge_commits, "merge commit", "merge commits")
            ),
        };
        ui.strong(commits_line);
        ui.label(
            egui::RichText::new(format!(
                "{} changed · {} · {}",
                counted(preview.files, "file", "files"),
                counted(preview.insertions, "insertion", "insertions"),
                counted(preview.deletions, "deletion", "deletions")
            ))
            .monospace(),
        );
    });
}

fn cascade_banner(ui: &mut Ui, state: &mut AppState, siblings: usize) {
    widgets::note(ui, Some(Palette::STATE_WARNING), |ui| {
        ui.horizontal(|ui| {
            ui.colored_label(
                Palette::STATE_WARNING,
                format!(
                    "⚠ Cascade with {} after this merge",
                    counted(siblings, "other repo", "other repos")
                ),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("View plan →").clicked() {
                    state.open_merge_cascade_plan();
                }
            });
        });
    });
}

fn rebase(ui: &mut Ui, state: &mut AppState) {
    let current = current_branch_name(state);
    let branches: Vec<String> = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .map(|r| {
            r.branches
                .iter()
                .filter(|b| b.kind == BranchKind::Local)
                .filter(|b| Some(&b.name) != current.as_ref())
                .map(|b| b.name.clone())
                .collect()
        })
        .unwrap_or_default();

    let onto_enabled = state.ui.dlg.rebase_onto_enabled;

    group_title(ui, "ONTO BRANCH");
    if onto_enabled {
        ui.label(
            egui::RichText::new("The plain target is superseded by the two base fields")
                .color(Palette::INK_3),
        );
    }
    ui.add_enabled_ui(!onto_enabled, |ui| {
        ui.horizontal(|ui| {
            icon(ui, Icon::GIT_BRANCH, 14.0, Palette::BRAND);
            let onto_empty = state.ui.dlg.rebase_onto.trim().is_empty();
            if onto_empty {
                ui.label(
                    egui::RichText::new("Rebase onto…")
                        .monospace()
                        .color(Palette::INK_3),
                );
            } else {
                ui.label(
                    egui::RichText::new(state.ui.dlg.rebase_onto.clone())
                        .monospace()
                        .color(Palette::INK),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Change…").clicked() {
                    state.ui.dlg.rebase_onto_picker_open = !state.ui.dlg.rebase_onto_picker_open;
                }
            });
        });
        if state.ui.dlg.rebase_onto_picker_open {
            for name in &branches {
                if ui
                    .selectable_label(&state.ui.dlg.rebase_onto == name, name.clone())
                    .clicked()
                {
                    state.ui.dlg.rebase_onto = name.clone();
                    state.ui.dlg.rebase_onto_picker_open = false;
                    state.ui.dlg.rebase_preview = None;
                }
            }
        }
    });
    ui.add_space(4.0);

    group_title(ui, "MODE");
    ui.horizontal(|ui| {
        for (label, mode) in [
            (
                "Interactive",
                turbogit_domain::model::RebaseMode::Interactive,
            ),
            ("Standard", turbogit_domain::model::RebaseMode::Standard),
            ("Autosquash", turbogit_domain::model::RebaseMode::Autosquash),
        ] {
            let interactive_with_onto =
                onto_enabled && mode == turbogit_domain::model::RebaseMode::Interactive;
            let segment = ui.add_enabled(
                !interactive_with_onto,
                egui::Button::selectable(state.ui.dlg.rebase_mode == mode, label),
            );
            let segment = if interactive_with_onto {
                segment.on_disabled_hover_text(
                    "Interactive replays an edited plan; a plan has no slot for --onto",
                )
            } else {
                segment
            };
            if segment.clicked() {
                state.ui.dlg.rebase_mode = mode;
            }
        }
    });
    ui.add_space(4.0);

    let onto = state.ui.dlg.rebase_onto.trim().to_string();
    let (new_verdict, old_verdict) = onto_base_verdicts(state, onto_enabled);
    let effective_base = if onto_enabled {
        if old_verdict == BaseVerdict::Usable {
            state.ui.dlg.rebase_old_base.trim().to_string()
        } else {
            String::new()
        }
    } else {
        onto.clone()
    };
    let mut replay: Vec<turbogit_domain::model::RebasePlanEntry> = Vec::new();
    if !effective_base.is_empty() {
        if state.ui.dlg.rebase_preview_key.as_deref() != Some(effective_base.as_str()) {
            let preview = state
                .selected_root
                .as_ref()
                .and_then(|root| state.rebase_plan(root, &effective_base));
            state.ui.dlg.rebase_preview = preview;
            state.ui.dlg.rebase_preview_key = Some(effective_base.clone());
        }
        if let Some(plan) = state.ui.dlg.rebase_preview.clone() {
            group_title(ui, &format!("{} COMMITS TO REBASE", plan.len()));
            widgets::note(ui, None, |ui| {
                for entry in &plan {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  {}",
                            widgets::short_commit_ref(&entry.commit),
                            entry.subject
                        ))
                        .monospace(),
                    );
                }
            });
            replay = plan;
        }
    }
    ui.add_space(6.0);

    group_title(ui, "OPTIONS");
    ui.checkbox(
        &mut state.ui.dlg.rebase_autosquash,
        "Autosquash fixup commits",
    );
    ui.checkbox(
        &mut state.ui.dlg.rebase_update_refs,
        "Update branches (--update-refs)",
    );
    ui.checkbox(&mut state.ui.dlg.rebase_keep_empty, "Keep empty commits");
    let was_enabled = state.ui.dlg.rebase_onto_enabled;
    ui.checkbox(
        &mut state.ui.dlg.rebase_onto_enabled,
        "Custom base (--onto)",
    );
    if state.ui.dlg.rebase_onto_enabled
        && !was_enabled
        && state.ui.dlg.rebase_mode == turbogit_domain::model::RebaseMode::Interactive
    {
        state.ui.dlg.rebase_mode = turbogit_domain::model::RebaseMode::Standard;
    }
    ui.add_space(6.0);

    // --onto, the pair, destination first. The order is the whole point:
    // `git rebase --onto <new base> <old base>` moves the sequence above the
    // cutoff ONTO the new base, and swapping the two rebases the wrong commits.
    if state.ui.dlg.rebase_onto_enabled {
        let missing = "There is no such branch or hash";
        group_title(ui, "NEW BASE");
        onto_base_field(
            ui,
            "New base",
            &mut state.ui.dlg.rebase_new_base,
            new_verdict,
            "Select a new base",
            missing,
        );
        ui.add_space(4.0);
        group_title(ui, "OLD BASE");
        onto_base_field(
            ui,
            "Old base",
            &mut state.ui.dlg.rebase_old_base,
            old_verdict,
            "Select an old base",
            missing,
        );
        ui.add_space(6.0);
    }

    let siblings = state.rebase_affected_siblings();
    if !siblings.is_empty() && !replay.is_empty() {
        rebase_banner(ui, state, replay.len(), siblings.len() + 1);
        ui.add_space(4.0);
    }

    let protected = state.rebase_branch_is_protected();
    let in_flight = state
        .selected_root
        .as_ref()
        .and_then(|root| state.holding_operation(root.as_path()));
    widgets::dialog_footer(ui, |ui| {
        {
            let label = match state.ui.dlg.rebase_mode {
                turbogit_domain::model::RebaseMode::Interactive => "Start interactive rebase",
                turbogit_domain::model::RebaseMode::Standard => "Start rebase",
                turbogit_domain::model::RebaseMode::Autosquash => "Start autosquash rebase",
            };
            let bases_ready = new_verdict == BaseVerdict::Usable
                && old_verdict == BaseVerdict::Usable
                && !state.ui.dlg.rebase_new_base.trim().is_empty()
                && !state.ui.dlg.rebase_old_base.trim().is_empty();
            let ready = if state.ui.dlg.rebase_onto_enabled {
                bases_ready
            } else {
                !effective_base.is_empty()
            };
            let start = ui
                .add_enabled(
                    ready && in_flight.is_none() && !protected,
                    egui::Button::new(label),
                )
                .on_disabled_hover_text(if protected {
                    format!(
                        "'{}' is a protected branch — rebasing it is not allowed",
                        current.clone().unwrap_or_else(|| "…".to_string())
                    )
                } else if let Some(operation) = in_flight {
                    format!(
                        "A {} is already in progress in {}",
                        operation.label(),
                        state
                            .selected_root
                            .as_ref()
                            .map(|root| root.as_path().display_name_or(""))
                            .unwrap_or_else(|| "this repository".to_string())
                    )
                } else if !ready {
                    if state.ui.dlg.rebase_onto_enabled {
                        "Fill in both base fields".to_string()
                    } else {
                        "Pick an onto branch first".to_string()
                    }
                } else {
                    String::new()
                });
            if start.clicked() {
                let root = state.selected_root.clone();
                let mode = state.ui.dlg.rebase_mode;
                let opts = state.rebase_dialog_opts();
                let settings = state.settings.clone();
                let branch = current.clone().unwrap_or_default();
                let onto2 = onto.clone();
                let replay2 = replay.clone();
                let new_base = state.ui.dlg.rebase_new_base.trim().to_string();
                let old_base = state.ui.dlg.rebase_old_base.trim().to_string();
                if let Some(root) = root {
                    let op = if state.ui.dlg.rebase_onto_enabled {
                        Operation::rebase_with_onto(
                            &root, &branch, &new_base, &old_base, &opts, &settings,
                        )
                    } else {
                        match mode {
                            turbogit_domain::model::RebaseMode::Interactive => {
                                Operation::rebase_interactive(
                                    &root, &branch, replay2, &settings, false,
                                )
                            }
                            turbogit_domain::model::RebaseMode::Standard => {
                                Operation::rebase_onto(&root, &branch, &onto2, &opts, &settings)
                            }
                            turbogit_domain::model::RebaseMode::Autosquash => {
                                Operation::rebase_autosquash(
                                    &root, &branch, &onto2, &opts, &settings,
                                )
                            }
                        }
                    };
                    state.dispatch(op);
                }
                close(state);
            }
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BaseVerdict {
    Empty,
    Missing,
    Usable,
}

/// The two base verdicts, re-read from the repository only when a field's value
/// changed since the last look.
///
/// `git rev-parse` is a process, and this runs in the paint pass: uncached, a
/// dialog sitting open would ask it twice a frame. The cache is keyed by the
/// value itself, so editing a field re-asks and anything else does not. With
/// the option off both fields are inert and answer `Usable`, which keeps the
/// start gate a single expression.
fn onto_base_verdicts(state: &mut AppState, onto_enabled: bool) -> (BaseVerdict, BaseVerdict) {
    if !onto_enabled {
        return (BaseVerdict::Usable, BaseVerdict::Usable);
    }
    let Some(root) = state.selected_root.clone() else {
        return (BaseVerdict::Empty, BaseVerdict::Empty);
    };
    let new_value = state.ui.dlg.rebase_new_base.trim().to_string();
    let old_value = state.ui.dlg.rebase_old_base.trim().to_string();
    let new_cached = state
        .ui
        .dlg
        .rebase_new_base_check
        .as_ref()
        .filter(|(key, _)| *key == new_value)
        .map(|(_, ok)| *ok);
    let old_cached = state
        .ui
        .dlg
        .rebase_old_base_check
        .as_ref()
        .filter(|(key, _)| *key == old_value)
        .map(|(_, ok)| *ok);
    let new_ok = match new_cached {
        Some(ok) => ok,
        None => !new_value.is_empty() && state.revision_exists(&root, &new_value),
    };
    let old_ok = match old_cached {
        Some(ok) => ok,
        None => !old_value.is_empty() && state.revision_exists(&root, &old_value),
    };
    state.ui.dlg.rebase_new_base_check = Some((new_value.clone(), new_ok));
    state.ui.dlg.rebase_old_base_check = Some((old_value.clone(), old_ok));
    (
        base_verdict_of(&new_value, new_ok),
        base_verdict_of(&old_value, old_ok),
    )
}

fn base_verdict_of(value: &str, resolves: bool) -> BaseVerdict {
    if value.is_empty() {
        BaseVerdict::Empty
    } else if resolves {
        BaseVerdict::Usable
    } else {
        BaseVerdict::Missing
    }
}

fn onto_base_field(
    ui: &mut Ui,
    name: &str,
    value: &mut String,
    verdict: BaseVerdict,
    empty_error: &str,
    missing_error: &str,
) {
    // Bounded width: an INFINITY desired width inside a horizontal claims the
    // whole window surface and swallows clicks below (the tag field's lesson).
    let resp = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text("branch or hash")
            .desired_width(320.0),
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name.to_owned())
    });
    match verdict {
        BaseVerdict::Usable => {}
        BaseVerdict::Empty => {
            widgets::inline_error(ui, empty_error);
        }
        BaseVerdict::Missing => {
            widgets::inline_error(ui, missing_error);
        }
    }
}

fn rebase_banner(ui: &mut Ui, state: &mut AppState, commits: usize, repos: usize) {
    widgets::note(ui, Some(Palette::STATE_WARNING), |ui| {
        ui.horizontal(|ui| {
            ui.colored_label(
                Palette::STATE_WARNING,
                format!(
                    "⚠ Rewrites {commits} commits across {}",
                    counted(repos, "repo", "repos")
                ),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("View affected →").clicked() {
                    state.open_rebase_affected_list();
                }
            });
        });
    });
}

fn tag(ui: &mut Ui, state: &mut AppState) {
    if state.ui.dlg.tag_existing.is_none() {
        state.ui.dlg.tag_existing = state
            .selected_root
            .as_ref()
            .and_then(|root| state.tag_names(root));
    }

    group_title(ui, "TAG NAME");
    let name = state.ui.dlg.tag_name.clone();
    let validation =
        tag_service::validate_name(&name, state.ui.dlg.tag_existing.as_deref().unwrap_or(&[]));
    ui.horizontal(|ui| {
        // Bounded width: an INFINITY desired width inside a horizontal
        // claims the whole window surface and swallows clicks below.
        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.tag_name)
                .hint_text("v1.2.3")
                .desired_width(320.0),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Tag name")
        });
        match &validation {
            Ok(()) => ui.colored_label(Palette::STATE_SUCCESS, "✓ valid"),
            Err(reason) => widgets::inline_error(ui, reason),
        };
    });
    ui.add_space(4.0);

    group_title(ui, "TARGET");
    ui.horizontal(|ui| {
        icon(ui, Icon::GIT_COMMIT, 14.0, Palette::BRAND);
        if state.ui.dlg.tag_target.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "HEAD on {}",
                    current_branch_name(state).unwrap_or_else(|| "…".to_string())
                ))
                .monospace()
                .color(Palette::INK),
            );
        } else {
            ui.label(
                egui::RichText::new(state.ui.dlg.tag_target.clone())
                    .monospace()
                    .color(Palette::INK),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let r = widgets::compact_button(ui, "Change…");
            if std::env::var("TG_TAG_DEBUG").is_ok() {
                eprintln!(
                    "CHG rect={:?} clicked={} pointer={:?}",
                    r.rect,
                    r.clicked(),
                    ui.input(|i| i.pointer.latest_pos())
                );
            }
            if r.clicked() {
                state.ui.dlg.tag_target_picker_open = !state.ui.dlg.tag_target_picker_open;
            }
        });
    });
    if state.ui.dlg.tag_target_picker_open {
        if state.ui.dlg.tag_candidates.is_none() {
            state.ui.dlg.tag_candidates = state
                .selected_root
                .as_ref()
                .and_then(|root| state.recent_commits(root, 50));
        }
        if ui
            .selectable_label(state.ui.dlg.tag_target.is_empty(), "HEAD")
            .clicked()
        {
            state.ui.dlg.tag_target.clear();
            state.ui.dlg.tag_target_picker_open = false;
        }
        for c in state.ui.dlg.tag_candidates.iter().flatten() {
            let row = commit_row_label(c);
            if ui.selectable_label(false, row.clone()).clicked() {
                state.ui.dlg.tag_target = row;
                state.ui.dlg.tag_target_picker_open = false;
            }
        }
    }
    ui.add_space(4.0);

    group_title(ui, "TYPE");
    ui.horizontal(|ui| {
        for (label, ty) in [
            ("Lightweight", TagType::Lightweight),
            ("Annotated", TagType::Annotated),
        ] {
            if ui
                .selectable_label(state.ui.dlg.tag_type == ty, label)
                .clicked()
            {
                state.ui.dlg.tag_type = ty;
            }
        }
    });
    ui.add_space(4.0);

    let annotated = state.ui.dlg.tag_type == TagType::Annotated;
    if annotated {
        group_title(ui, "MESSAGE");
        let msg = ui.add(
            egui::TextEdit::multiline(&mut state.ui.dlg.tag_msg)
                .hint_text("Markdown supported")
                .desired_width(380.0),
        );
        msg.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Tag message")
        });
        ui.add_space(4.0);

        group_title(ui, "TAGGER");
        let tagger = ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.tag_tagger)
                .hint_text("Name <email> — blank = git config identity")
                .desired_width(380.0),
        );
        tagger
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Tagger"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.ui.dlg.tag_sign, "Sign with GPG key");
            if state.ui.dlg.tag_sign && !state.ui.dlg.tag_signing_key_fetched {
                state.ui.dlg.tag_signing_key = state
                    .selected_root
                    .as_ref()
                    .and_then(|root| state.signing_key(root));
                state.ui.dlg.tag_signing_key_fetched = true;
            }
            if state.ui.dlg.tag_sign
                && let Some(key) = &state.ui.dlg.tag_signing_key
            {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(shorten_key(key))
                            .monospace()
                            .color(Palette::BRAND),
                    );
                });
            }
        });
        ui.add_space(4.0);
    }

    group_title(ui, "OPTIONS");
    ui.checkbox(&mut state.ui.dlg.tag_push, "Push tag to origin immediately");
    ui.add_space(6.0);

    widgets::dialog_footer(ui, |ui| {
        let valid = validation.is_ok();
        let create = ui
            .scope(|ui| {
                if !valid {
                    ui.disable();
                }
                widgets::compact_button(ui, "Create tag")
            })
            .inner;
        if create.clicked() {
            let root = state.selected_root.clone();
            let spec = turbogit_domain::model::TagSpec {
                name: state.ui.dlg.tag_name.trim().to_string(),
                target: (!state.ui.dlg.tag_target.is_empty())
                    .then(|| target_commitish(&state.ui.dlg.tag_target)),
                message: annotated
                    .then(|| state.ui.dlg.tag_msg.clone())
                    .filter(|m| !m.trim().is_empty()),
                tagger: annotated
                    .then(|| state.ui.dlg.tag_tagger.trim().to_string())
                    .filter(|t| !t.is_empty()),
                sign: annotated && state.ui.dlg.tag_sign,
            };
            let push = state.ui.dlg.tag_push;
            if let Some(root) = root {
                state.dispatch(shell_writes::tag::create(&root, &spec, push));
            }
            close(state);
        }
        if widgets::compact_button(ui, "Cancel").clicked() {
            close(state);
        }
    });
}

fn commit_row_label(c: &turbogit_domain::model::Commit) -> String {
    format!("{}  {}", widgets::short_commit_ref(&c.id), c.subject())
}

fn target_commitish(row: &str) -> String {
    row.split_whitespace().next().unwrap_or(row).to_string()
}

fn shorten_key(key: &str) -> String {
    let head: String = key.chars().take(17).collect();
    if key.chars().count() > 17 {
        format!("{head}…")
    } else {
        head
    }
}

fn shelve(ui: &mut Ui, state: &mut AppState) {
    ui.label("Shelf name:");
    ui.text_edit_singleline(&mut state.ui.dlg.shelve_name);
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        if ui.button("Shelve selected").clicked() {
            let name = state.ui.dlg.shelve_name.clone();
            let mut changes = Vec::new();
            if let Some(id) = &state.selected_root
                && let Some(root) = state.multi.by_id(id)
            {
                for c in &root.status.changes {
                    if state.ui.selected.contains(&root.canonical_key(c)) {
                        changes.push(c.clone());
                    }
                }
            }
            let Some(root) = state.selected_root.clone() else {
                close(state);
                return;
            };
            let shelf = shelve_stash::make_shelf(&name, &changes, root.clone());
            if state.add_shelf(shelf).is_err() {
                close(state);
                return;
            }
            state.dispatch(Operation::shelve(root, Vec::new(), name));
            close(state);
        }
    });
}

fn import_patches(ui: &mut Ui, state: &mut AppState) {
    ui.label("Patch file:");
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.ui.dlg.import_patch_path)
                .hint_text("/path/to/change.patch")
                .desired_width(320.0),
        );
        if ui.button("Choose file…").clicked() {
            state.choose_import_patch_file();
        }
    });
    ui.label("Shelf name:");
    ui.text_edit_singleline(&mut state.ui.dlg.import_patch_name);
    if let Some(message) = state.ui.dlg.import_patch_error.clone() {
        widgets::inline_error(ui, message);
    }
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        if ui.button("Import to shelf").clicked() {
            let path = std::path::PathBuf::from(state.ui.dlg.import_patch_path.trim());
            let name = state.ui.dlg.import_patch_name.trim().to_string();
            match state.import_patch_file_to_shelf(&path, &name) {
                Ok(_) => {
                    state.ui.dlg.import_patch_path.clear();
                    state.ui.dlg.import_patch_name.clear();
                    state.ui.dlg.import_patch_error = None;
                    close(state);
                }
                Err(e) => state.ui.dlg.import_patch_error = Some(e.to_string()),
            }
        }
    });
}

fn stash(ui: &mut Ui, state: &mut AppState) {
    ui.label("Message:");
    ui.text_edit_singleline(&mut state.ui.dlg.stash_msg);
    ui.checkbox(&mut state.ui.dlg.stash_keep, "Keep index (--keep-index)");
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        if ui.button("Pop latest").clicked() {
            if let Some(root) = state.selected_root.clone() {
                state.dispatch(shell_writes::stash::pop_latest(&root));
            }
            close(state);
        }
        if ui.button("Stash").clicked() {
            let msg = state.ui.dlg.stash_msg.clone();
            let keep = state.ui.dlg.stash_keep;
            if let Some(root) = state.selected_root.clone() {
                state.dispatch(shell_writes::stash::push(&root, &msg, keep));
            }
            close(state);
        }
    });
}

fn rename_branch(ui: &mut Ui, state: &mut AppState) {
    let old = state.ui.dlg.rename_branch_name.clone();
    let root = state.ui.dlg.rename_branch_root.clone();
    let upstream = state.ui.dlg.rename_branch_upstream.clone();
    ui.label(format!("Rename '{old}' to:"));
    let edit = ui.text_edit_singleline(&mut state.ui.dlg.rename_branch_new);
    edit.request_focus();
    if upstream.is_some() {
        ui.checkbox(
            &mut state.ui.dlg.rename_branch_unset_upstream,
            "Unset upstream branch",
        );
    }
    if let Some(up) = &upstream {
        let clearing = state.ui.dlg.rename_branch_unset_upstream;
        widgets::note(ui, None, |ui| {
            ui.label(format!(
                "Only the local branch is renamed — your next push creates and tracks a new \
                 remote branch, and {} is left behind.",
                up.git_ref()
            ));
            if clearing {
                ui.label(
                    "The upstream is cleared now, so the renamed branch tracks nothing until \
                     you push.",
                );
            }
        });
    }
    let new = state.ui.dlg.rename_branch_new.trim().to_string();
    let named = !new.is_empty() && new != old;
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        let rename = kit_button(
            ui,
            if named {
                KitButton::Primary
            } else {
                KitButton::Quiet
            },
            "Rename",
        );
        if named && rename.clicked() {
            let unset_upstream = state.ui.dlg.rename_branch_unset_upstream;
            if let Some(id) = &root {
                state.rename_branch(id, &old, &new, unset_upstream);
            }
            close(state);
        }
    });
}

fn compare_branches(ui: &mut Ui, state: &mut AppState) {
    let left = state.ui.dlg.compare_left.clone();
    let right = state.ui.dlg.compare_right.clone();
    let commits = state.ui.dlg.compare_commits.clone();
    let root = state.ui.dlg.compare_root.clone();
    let mut swapped = false;
    ui.label(format!(
        "{} commit(s) in '{left}' missing from '{right}'",
        commits.len()
    ));
    let subjects: std::collections::HashMap<String, String> = root
        .as_ref()
        .and_then(|id| state.caches.log(id))
        .map(|log| {
            log.iter()
                .map(|c| (c.id.clone(), c.message.clone()))
                .collect()
        })
        .unwrap_or_default();
    egui::ScrollArea::vertical()
        .id_salt("compare_branch_commits")
        .max_height(240.0)
        .show(ui, |ui| {
            for c in &commits {
                let short = widgets::short_commit_ref(c);
                let subj = subjects.get(c).cloned().unwrap_or_default();
                let line = if subj.is_empty() {
                    short
                } else {
                    format!("{short}  {subj}")
                };
                ui.label(line);
            }
            if commits.is_empty() {
                widgets::empty_state(ui, "No commits — branches are in sync.");
            }
        });
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Close").clicked() {
            close(state);
        }
        if ui.button("Swap Branches").clicked() {
            swapped = true;
        }
    });
    if swapped {
        state.ui.dlg.compare_left = right.clone();
        state.ui.dlg.compare_right = left.clone();
        if let Some(id) = root {
            state.ui.dlg.compare_commits = state.outgoing(&id, &right, &left);
        }
    }
}

pub(crate) enum UnmergedAction {
    Nothing,
    Restore,
    Close,
}

pub(crate) fn unmerged_disclosure(
    ui: &mut Ui,
    state: &AppState,
    unmerged: &UnmergedCommits,
) -> UnmergedAction {
    ui.label(unmerged.header());
    egui::ScrollArea::vertical()
        .id_salt("unmerged_commits")
        .max_height(240.0)
        .show(ui, |ui| {
            for c in &unmerged.commits {
                let short = widgets::short_commit_ref(&c.id);
                let line = match &c.subject {
                    Some(subject) => format!("{short}  {subject}"),
                    None => short,
                };
                ui.label(line);
            }
        });
    let mut action = UnmergedAction::Nothing;
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Close").clicked() {
            action = UnmergedAction::Close;
        }
        if state.unmerged_restore_available(unmerged)
            && kit_button(ui, KitButton::Primary, "Restore").clicked()
        {
            action = UnmergedAction::Restore;
        }
    });
    action
}

fn branch_not_fully_merged(ui: &mut Ui, state: &mut AppState) {
    let Some(unmerged) = state.ui.dlg.unmerged_view.clone() else {
        widgets::empty_state(ui, "No unmerged commits to show.");
        close(state);
        return;
    };
    match unmerged_disclosure(ui, state, &unmerged) {
        UnmergedAction::Nothing => {}
        UnmergedAction::Close => {
            state.ui.dlg.unmerged_view = None;
            close(state);
        }
        UnmergedAction::Restore => {
            state.restore_unmerged_branch(&unmerged);
            state.ui.dlg.unmerged_view = None;
            close(state);
        }
    }
}
