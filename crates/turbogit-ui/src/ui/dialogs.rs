//! Modal dialogs: Merge (F1/F2), Rebase (F3/F4), Interactive rebase (F5),
//! New Branch (E3), Tag (O1–O4), Shelve (J1–J4), Stash (J5–J8). The Push
//! dialog lives in [`super::push_dialog`] (issue #20).

use egui::{Align, Layout, Ui};
use std::path::Path;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Dialog, TagType};
use turbogit_domain::model::{BranchKind, MergeStrategy};
use turbogit_services::{
    branch_service, history_editor, integrate_service, shelve_stash, sync_service, tag_service,
};

use crate::theme::Palette;
use crate::ui::icons::{Icon, icon};
use crate::ui::widgets::group_title;

pub fn show(ui: &mut Ui, state: &mut AppState, dialog: Dialog) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    let title = match dialog {
        Dialog::NewBranch => "New Branch".to_string(),
        // Screen 14: the title names the branch being merged into.
        Dialog::Merge => format!(
            "Merge into {}",
            current_branch_name(state).unwrap_or_else(|| "…".to_string())
        ),
        // Screen 15: the title names the branch being rewritten.
        Dialog::Rebase => format!(
            "Rebase {}",
            current_branch_name(state).unwrap_or_else(|| "…".to_string())
        ),
        // Screen 16: the title names the tag being created.
        Dialog::Tag => {
            let name = state.ui.dlg.tag_name.trim();
            if name.is_empty() {
                "Create tag".to_string()
            } else {
                format!("Create tag {name}")
            }
        }
        Dialog::Shelve => "Shelve".to_string(),
        Dialog::Stash => "Stash".to_string(),
        Dialog::NewWorktree => "New worktree".to_string(),
        Dialog::CherryPickTarget => "Cherry-pick to branch".to_string(),
        // Issue 32 branches popup row actions.
        Dialog::RenameBranch => "Rename branch".to_string(),
        Dialog::CompareBranches => {
            format!(
                "Compare {} with {}",
                state.ui.dlg.compare_left, state.ui.dlg.compare_right
            )
        }
        // Push is rendered by `push_dialog` (issue #20); `ui::render` routes
        // it there before this function is ever called. The interactive
        // rebase editor (issue 30) is likewise routed to its own module.
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
            Dialog::Stash => stash(ui, state),
            Dialog::NewWorktree => super::worktrees::new_worktree_dialog(ui, state),
            Dialog::CherryPickTarget => cherry_pick_target(ui, state),
            Dialog::RenameBranch => rename_branch(ui, state),
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

/// Cherry-pick target-branch picker (issue 15): one row per local branch of
/// the focused root. Protected branches are disabled with an explanation;
/// picking a branch dispatches the guarded cherry-pick through
/// [`AppState::cherry_pick_to`] and closes the dialog.
fn cherry_pick_target(ui: &mut Ui, state: &mut AppState) {
    ui.label("Apply the selected commit onto which branch?");
    if state.ui.dlg.cherry_pick_commit.is_none() {
        ui.label("No commit selected.");
        if ui.button("Cancel").clicked() {
            close(state);
        }
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
        ui.label("No local branches.");
    }
    for name in local {
        let protected = sync_service::is_protected(&state.settings, &name);
        let label = if protected {
            format!("{name} (protected)")
        } else {
            name.clone()
        };
        let row = ui
            .add_enabled(!protected, egui::Button::new(label))
            .on_disabled_hover_text(format!("'{name}' is a protected branch"));
        if row.clicked() {
            if let Some(commit) = state.ui.dlg.cherry_pick_commit.take() {
                state.cherry_pick_to(commit, name.clone());
            }
            close(state);
        }
    }
    if ui.button("Cancel").clicked() {
        close(state);
    }
}

fn new_branch(ui: &mut Ui, state: &mut AppState) {
    // The base defaults to the branch the person is on — creating from an
    // unexpected base is a classic silent mistake (issue 08, design §6.2).
    if state.ui.dlg.new_branch_base.is_empty()
        && let Some(cur) = current_branch_name(state)
    {
        state.ui.dlg.new_branch_base = cur;
    }
    ui.label("Name:");
    ui.text_edit_singleline(&mut state.ui.dlg.new_branch_name);
    ui.label("Start from:");
    ui.horizontal(|ui| {
        ui.label(&state.ui.dlg.new_branch_base);
        if ui.button("Change…").clicked() {
            state.ui.dlg.new_branch_base_picker_open = !state.ui.dlg.new_branch_base_picker_open;
        }
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
                .selectable_label(state.ui.dlg.new_branch_base == b, &b)
                .clicked()
            {
                state.ui.dlg.new_branch_base = b;
                state.ui.dlg.new_branch_base_picker_open = false;
            }
        }
    }
    // One decision made explicit: switch to the new branch right away? The
    // default is yes — almost always the intent (issue 08).
    ui.checkbox(
        &mut state.ui.dlg.new_branch_checkout,
        "Switch to the new branch now",
    );
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() {
            let root = state.selected_path();
            let name = state.ui.dlg.new_branch_name.clone();
            let base = state.ui.dlg.new_branch_base.clone();
            let start = if base.trim().is_empty() {
                None
            } else {
                Some(base)
            };
            let co = state.ui.dlg.new_branch_checkout;
            // The Branches tab scrolls the fresh branch into view (issue 08):
            // creating something and then hunting for it feels broken.
            state.ui.branches_scroll_to = Some(name.clone());
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

/// The focused root's current branch name, for the dialog title.
fn current_branch_name(state: &AppState) -> Option<String> {
    let id = state.selected_root.as_ref()?;
    state.multi.by_id(id)?.current_branch.clone()
}

/// The Merge dialog (issue 28, screen 14): a source-branch picker with a
/// "Change…" list, the STRATEGY segmented control driving the merge flags,
/// the option rows, a pre-merge preview box ("Will create …"), and — when
/// sibling repos share the branch being merged into — the cascade banner
/// handing off to the cascade preflight ([`AppState::open_merge_cascade_plan`]).
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

    // SOURCE BRANCH
    group_title(ui, "SOURCE BRANCH");
    let target_empty = state.ui.dlg.merge_target.trim().is_empty();
    ui.horizontal(|ui| {
        // The picker row: branch icon + picked name (or placeholder).
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

    // STRATEGY
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

    // OPTIONS
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

    // PREVIEW — computed from the live repository before anything runs;
    // cached per (source branch, strategy) so it only recomputes on change.
    let target = state.ui.dlg.merge_target.trim().to_string();
    if !target.is_empty() {
        let key = (target.clone(), state.ui.dlg.merge_strategy);
        if state.ui.dlg.merge_preview_key.as_ref() != Some(&key) {
            let preview = state.selected_path().and_then(|root| {
                integrate_service::merge_preview(
                    state.executor.as_ref(),
                    Path::new(&root),
                    &target,
                    key.1,
                )
                .ok()
            });
            state.ui.dlg.merge_preview = preview;
            state.ui.dlg.merge_preview_key = Some(key);
        }
        if let Some(preview) = state.ui.dlg.merge_preview {
            preview_box(ui, preview, state.ui.dlg.merge_strategy);
        }
    }
    ui.add_space(6.0);

    // CASCADE BANNER — sibling repos sharing the branch being merged into
    // will want the same merge (issue 28). The banner hands off to the
    // cascade preflight for them.
    let siblings = state.merge_cascade_siblings();
    if !siblings.is_empty() {
        cascade_banner(ui, state, siblings.len());
        ui.add_space(4.0);
    }

    // FOOTER
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can_merge = !target.is_empty();
            let merge_btn = ui.add_enabled(can_merge, egui::Button::new("Merge"));
            if merge_btn.clicked() {
                let root = state.selected_path();
                let target = state.ui.dlg.merge_target.clone();
                let opts = state.merge_dialog_opts();
                let clean = state.settings.clean_tree_method;
                // Issue 09: the label reports how many commits come in (the
                // preview already knows) so success says it plainly.
                let commits = state
                    .ui
                    .dlg
                    .merge_preview
                    .as_ref()
                    .map(|p| p.merge_commits)
                    .unwrap_or(0);
                let label = if commits > 0 {
                    format!("Merge {target} ({commits} commits)")
                } else {
                    format!("Merge {target}")
                };
                state.run_git(
                    label,
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
        });
    });
}

/// The preview box (screen 14): "Will create N merge commit(s)" plus the
/// file/insertion/deletion totals the merge brings in. Strategy-dependent
/// wording: a fast-forward creates no merge commit; an already-merged
/// target reads as up to date.
fn preview_box(ui: &mut Ui, preview: integrate_service::MergePreview, strategy: MergeStrategy) {
    egui::Frame::new()
        .fill(Palette::SURFACE_2)
        .inner_margin(8.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            if preview.files == 0 && preview.insertions == 0 && preview.deletions == 0 {
                ui.strong("Already up to date");
                return;
            }
            let commits_line = match strategy {
                MergeStrategy::FastForward => "Will fast-forward — no merge commit".to_string(),
                _ => format!(
                    "Will create {} merge {}",
                    preview.merge_commits,
                    if preview.merge_commits == 1 {
                        "commit"
                    } else {
                        "commits"
                    }
                ),
            };
            ui.strong(commits_line);
            ui.label(
                egui::RichText::new(format!(
                    "{} {} · {} {} · {} {}",
                    preview.files,
                    if preview.files == 1 {
                        "file changed"
                    } else {
                        "files changed"
                    },
                    preview.insertions,
                    if preview.insertions == 1 {
                        "insertion"
                    } else {
                        "insertions"
                    },
                    preview.deletions,
                    if preview.deletions == 1 {
                        "deletion"
                    } else {
                        "deletions"
                    }
                ))
                .monospace(),
            );
        });
}

/// The cascade banner (screen 14): a warning-toned strip naming how many
/// sibling repos share the branch, with the "View plan →" hand-off.
fn cascade_banner(ui: &mut Ui, state: &mut AppState, siblings: usize) {
    let noun = if siblings == 1 { "repo" } else { "repos" };
    egui::Frame::new()
        .fill(Palette::SURFACE_2)
        .stroke(egui::Stroke::new(1.0, Palette::STATE_WARNING))
        .inner_margin(8.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Palette::STATE_WARNING,
                    format!("⚠ Cascade with {siblings} other {noun} after this merge"),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("View plan →").clicked() {
                        state.open_merge_cascade_plan();
                    }
                });
            });
        });
}

/// The Rebase dialog (issue 29, screen 15): an ONTO BRANCH picker with a
/// "Change…" list, the MODE segmented control (Interactive / Standard /
/// Autosquash) driving the rebase invocation, the commits-to-rebase list
/// computed from the live repository before anything runs, the option rows
/// mapped onto the chosen mode, and — when the rewritten branch is checked
/// out or tracking-shared in sibling repos — the cross-repo warning banner
/// handing off to the affected-repo list
/// ([`AppState::open_rebase_affected_list`]). A protected current branch
/// disables the start button (history rewrites are blocked on protected
/// branches).
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
                // Rebasing the branch onto itself is a no-op; the picker
                // lists every other local branch.
                .filter(|b| Some(&b.name) != current.as_ref())
                .map(|b| b.name.clone())
                .collect()
        })
        .unwrap_or_default();

    // ONTO BRANCH
    group_title(ui, "ONTO BRANCH");
    let onto_empty = state.ui.dlg.rebase_onto.trim().is_empty();
    ui.horizontal(|ui| {
        icon(ui, Icon::GIT_BRANCH, 14.0, Palette::BRAND);
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
    ui.add_space(4.0);

    // MODE
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
            if ui
                .selectable_label(state.ui.dlg.rebase_mode == mode, label)
                .clicked()
            {
                state.ui.dlg.rebase_mode = mode;
            }
        }
    });
    ui.add_space(4.0);

    // COMMITS TO REBASE — computed from the live repository before anything
    // runs; cached per onto branch so it only recomputes on change.
    let onto = state.ui.dlg.rebase_onto.trim().to_string();
    let mut replay: Vec<turbogit_domain::model::RebasePlanEntry> = Vec::new();
    if !onto.is_empty() {
        if state.ui.dlg.rebase_preview_key.as_deref() != Some(onto.as_str()) {
            let preview = state.selected_path().and_then(|root| {
                history_editor::build_plan(state.executor.as_ref(), Path::new(&root), &onto).ok()
            });
            state.ui.dlg.rebase_preview = preview;
            state.ui.dlg.rebase_preview_key = Some(onto.clone());
        }
        if let Some(plan) = state.ui.dlg.rebase_preview.clone() {
            group_title(ui, &format!("{} COMMITS TO REBASE", plan.len()));
            egui::Frame::new()
                .fill(Palette::SURFACE_2)
                .inner_margin(8.0)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    for entry in &plan {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}  {}",
                                &entry.commit[..7.min(entry.commit.len())],
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

    // OPTIONS
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
    ui.add_space(6.0);

    // CROSS-REPO BANNER — sibling repos with the branch checked out or
    // tracking-shared will diverge from the rewritten history (issue 29).
    let siblings = state.rebase_affected_siblings();
    if !siblings.is_empty() && !replay.is_empty() {
        rebase_banner(ui, state, replay.len(), siblings.len() + 1);
        ui.add_space(4.0);
    }

    // FOOTER
    let protected = state.rebase_branch_is_protected();
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (label, op) = match state.ui.dlg.rebase_mode {
                turbogit_domain::model::RebaseMode::Interactive => {
                    ("Start interactive rebase", "Interactive rebase".to_string())
                }
                turbogit_domain::model::RebaseMode::Standard => {
                    ("Start rebase", format!("Rebase onto {onto}"))
                }
                turbogit_domain::model::RebaseMode::Autosquash => (
                    "Start autosquash rebase",
                    format!("Autosquash rebase onto {onto}"),
                ),
            };
            let start = ui
                .add_enabled(!onto.is_empty() && !protected, egui::Button::new(label))
                .on_disabled_hover_text(if protected {
                    format!(
                        "'{}' is a protected branch — rebasing it is not allowed",
                        current.clone().unwrap_or_else(|| "…".to_string())
                    )
                } else {
                    "Pick an onto branch first".to_string()
                });
            if start.clicked() {
                let root = state.selected_path();
                let mode = state.ui.dlg.rebase_mode;
                let opts = state.rebase_dialog_opts();
                let settings = state.settings.clone();
                let branch = current.clone().unwrap_or_default();
                let onto2 = onto.clone();
                let replay2 = replay.clone();
                state.run_git(
                    op,
                    Affected::from_optional_root(root.as_deref()),
                    move |v| {
                        if let Some(r) = &root {
                            // Interactive mode replays the listed commits
                            // through the plan path; Standard/Autosquash run
                            // a plain rebase with the mode-mapped flags.
                            // Both dispatchers refuse a protected branch.
                            match mode {
                                turbogit_domain::model::RebaseMode::Interactive => {
                                    integrate_service::rebase_plan(
                                        v, r, &replay2, &settings, &branch,
                                    )
                                }
                                _ => integrate_service::rebase_current(
                                    v, r, &onto2, &opts, &settings, &branch,
                                ),
                            }
                        } else {
                            Ok(())
                        }
                    },
                );
                close(state);
            }
        });
    });
}

/// The cross-repo banner (screen 15): a warning-toned strip naming how many
/// commits the rewrite replays across how many repos, with the
/// "View affected →" hand-off to the affected-repo list.
fn rebase_banner(ui: &mut Ui, state: &mut AppState, commits: usize, repos: usize) {
    let noun = if repos == 1 { "repo" } else { "repos" };
    egui::Frame::new()
        .fill(Palette::SURFACE_2)
        .stroke(egui::Stroke::new(1.0, Palette::STATE_WARNING))
        .inner_margin(8.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Palette::STATE_WARNING,
                    format!("⚠ Rewrites {commits} commits across {repos} {noun}"),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("View affected →").clicked() {
                        state.open_rebase_affected_list();
                    }
                });
            });
        });
}

/// The Tag dialog (issue 31, screen 16): a live-validated tag name ("✓
/// valid" / the reason), a target picker over recent commits (defaulting to
/// HEAD), the Lightweight / Annotated TYPE segmented control, and — for
/// annotated tags — a Markdown message, a tagger identity, and the "Sign
/// with GPG key" option with the configured key's fingerprint chip.
/// "Push tag to origin immediately" pushes the new tag right after creating
/// it and reports the outcome through the standard op toast.
fn tag(ui: &mut Ui, state: &mut AppState) {
    // Validation reads the live repository's tags; fetched once per dialog
    // lifetime and cached in the dialog state.
    if state.ui.dlg.tag_existing.is_none() {
        state.ui.dlg.tag_existing = state
            .selected_path()
            .and_then(|root| tag_service::list(state.executor.as_ref(), Path::new(&root)).ok());
    }

    // TAG NAME
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
        let (text, color) = match &validation {
            Ok(()) => ("✓ valid".to_string(), Palette::STATE_SUCCESS),
            Err(reason) => (reason.clone(), Palette::STATE_ERROR),
        };
        ui.colored_label(color, text);
    });
    ui.add_space(4.0);

    // TARGET — any commit can be the tag point; the picker lists the
    // repository's recent commits and defaults to HEAD.
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
            let r = ui.button("Change…");
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
            state.ui.dlg.tag_candidates = state.selected_path().and_then(|root| {
                state
                    .executor
                    .log(
                        Path::new(&root),
                        &turbogit_domain::model::LogOpts {
                            max_count: Some(50),
                            ..Default::default()
                        },
                    )
                    .ok()
            });
        }
        // The "HEAD" reset row, then one row per recent commit.
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

    // TYPE
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

    // ANNOTATED-ONLY FIELDS — a lightweight tag has nothing to annotate or
    // sign, so the whole block collapses.
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
                state.ui.dlg.tag_signing_key = state.selected_path().and_then(|root| {
                    tag_service::signing_key(state.executor.as_ref(), Path::new(&root))
                });
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

    // OPTIONS
    group_title(ui, "OPTIONS");
    ui.checkbox(&mut state.ui.dlg.tag_push, "Push tag to origin immediately");
    ui.add_space(6.0);

    // FOOTER
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let valid = validation.is_ok();
            let create = ui.add_enabled(valid, egui::Button::new("Create tag"));
            if create.clicked() {
                let root = state.selected_path();
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
                let name = spec.name.clone();
                state.run_git(
                    format!("Create tag {name}"),
                    Affected::from_optional_root(root.as_deref()),
                    move |v| {
                        if let Some(r) = &root {
                            tag_service::create(v, r, &spec)?;
                            if push {
                                // Reports the outcome: the error carries the
                                // created-tag context when the push fails.
                                tag_service::push_new(v, r, &name)?;
                            }
                            Ok(())
                        } else {
                            Ok(())
                        }
                    },
                );
                close(state);
            }
        });
    });
}

/// The target picker row (and TARGET display) for a commit:
/// "<short-sha>  <subject>".
fn commit_row_label(c: &turbogit_domain::model::Commit) -> String {
    format!(
        "{}  {}",
        &c.id[..7.min(c.id.len())],
        c.message.lines().next().unwrap_or_default()
    )
}

/// The commit-ish dispatched for a picked target row: the short sha prefix.
fn target_commitish(row: &str) -> String {
    row.split_whitespace().next().unwrap_or(row).to_string()
}

/// The signing-key chip: at most the first groups of the fingerprint/key id.
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

// ------------------------------------------------------- issue 32 dialogs --

/// Rename a branch (issue 32 branches popup row action): one input preset
/// with the old name, Rename dispatches through the engine seam and closes;
/// a blank new name stays inert.
fn rename_branch(ui: &mut Ui, state: &mut AppState) {
    let old = state.ui.dlg.rename_branch_name.clone();
    let root = state.ui.dlg.rename_branch_root.clone();
    ui.label(format!("Rename '{old}' to:"));
    let edit = ui.text_edit_singleline(&mut state.ui.dlg.rename_branch_new);
    edit.request_focus();
    ui.horizontal(|ui| {
        if ui.button("Rename").clicked() {
            let new = state.ui.dlg.rename_branch_new.trim().to_string();
            if !new.is_empty() && new != old {
                if let Some(id) = &root {
                    state.rename_branch(id, &old, &new);
                }
                close(state);
            }
        }
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

/// Compare two branches (issue 32, spec E9): the commit list is snapshotted
/// once when the dialog opens — commits on the left branch missing from the
/// right one. Swap Branches recomputes the list in the other direction.
/// Subjects come from the root's cached log when present.
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
    // Subject map from the root's cached log (may be empty when the log
    // hasn't loaded — the dialog still shows the short hashes).
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
        .max_height(240.0)
        .show(ui, |ui| {
            for c in &commits {
                let short = &c[..7.min(c.len())];
                let subj = subjects.get(c).cloned().unwrap_or_default();
                let line = if subj.is_empty() {
                    short.to_string()
                } else {
                    format!("{short}  {subj}")
                };
                ui.label(line);
            }
            if commits.is_empty() {
                ui.colored_label(Palette::INK_3, "No commits — branches are in sync.");
            }
        });
    ui.horizontal(|ui| {
        if ui.button("Swap Branches").clicked() {
            swapped = true;
        }
        if ui.button("Close").clicked() {
            close(state);
        }
    });
    if swapped {
        state.ui.dlg.compare_left = right.clone();
        state.ui.dlg.compare_right = left.clone();
        if let Some(id) = root {
            let path = id.0.clone();
            state.ui.dlg.compare_commits = state
                .executor
                .outgoing_commits(&path, &right, &left)
                .unwrap_or_default();
        }
    }
}
