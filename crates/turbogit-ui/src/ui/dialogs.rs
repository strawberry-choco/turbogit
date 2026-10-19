//! Modal dialogs: Merge (F1/F2), Rebase (F3/F4), Interactive rebase (F5),
//! New Branch (E3), Tag (O1–O4), Shelve (J1–J4), Stash (J5–J8). The Push
//! dialog lives in [`super::push_dialog`] (issue #20).

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
        Dialog::ImportPatches => "Import Patches".to_string(),
        Dialog::Stash => "Stash".to_string(),
        Dialog::NewWorktree => "New worktree".to_string(),
        Dialog::CherryPickTarget => "Cherry-pick to branch".to_string(),
        // Issue 32 branches popup row actions.
        Dialog::RenameBranch => "Rename branch".to_string(),
        // The reference's own words for this dialog, because the
        // fact it states is about the branch's *merge* state, not about a
        // deletion the developer already watched happen.
        Dialog::BranchNotFullyMerged => "Branch Was Not Fully Merged".to_string(),
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

/// Cherry-pick target-branch picker (issue 15): one row per local branch of
/// the focused root. A row the work may not land on is disabled and explains
/// itself; picking a branch dispatches the guarded cherry-pick through
/// [`AppState::cherry_pick_to`] and closes the dialog.
fn cherry_pick_target(ui: &mut Ui, state: &mut AppState) {
    ui.label("Apply the selected commit onto which branch?");
    if state.ui.dlg.cherry_pick_commit.is_none() {
        widgets::empty_state(ui, "No commit selected.");
        // Even the empty state gets the shared footer: the action slot is a
        // place in every one of this module's dialogs, and a Cancel that hangs
        // off the body is exactly the shape the footer rule exists to stop.
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
        // One row, one reason. The branch the work is standing on is refused
        // ahead of a protected one: it is not a destination at all — the commit
        // is already on it — and no setting change makes it one, so a refusal
        // the developer cannot act on would be a dead end. Protection speaks
        // only for a branch the picker would otherwise have offered. The words
        // are the branch menu's, so the two menus refuse a branch the same way.
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
    // The shared footer, so the branch list above and the actions below are two
    // places. Right-to-left keeps the painted order: `Cancel` rightmost.
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
    });
}

fn new_branch(ui: &mut Ui, state: &mut AppState) {
    // The base defaults to the branch the person is on — creating from an
    // unexpected base is a classic silent mistake (issue 08, design §6.2). An
    // open site that means a specific base (the log's New branch item, the
    // branch menu's New branch from) has already said so and is not overwritten.
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
            // Only a branch base can be selected here: a commit is not one of
            // these rows, so nothing is marked while it is the base — which is
            // true rather than broken, and the row above states what it is.
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
    // One decision made explicit: switch to the new branch right away? The
    // default is yes — almost always the intent (issue 08).
    ui.checkbox(
        &mut state.ui.dlg.new_branch_checkout,
        "Switch to the new branch now",
    );
    widgets::dialog_footer(ui, |ui| {
        // Right-to-left footer: the first action is the rightmost one. Cancel
        // is called first so the visible order remains Create · Cancel.
        if widgets::compact_button(ui, "Cancel").clicked() {
            close(state);
        }
        if widgets::compact_button(ui, "Create").clicked() {
            let root = state.selected_root.clone();
            let name = state.ui.dlg.new_branch_name.clone();
            // Whatever the base is — a branch or the commit the log named — git
            // takes it as the start point, which is why there is nothing to
            // translate here.
            let start = state
                .ui
                .dlg
                .new_branch_base
                .start_point()
                .map(str::to_owned);
            let co = state.ui.dlg.new_branch_checkout;
            // The Branches tab scrolls the fresh branch into view (issue 08):
            // creating something and then hunting for it feels broken.
            state.ui.branches_tree.scroll_to = Some(name.clone());
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

/// The "Start from:" value, in the one place that knows a base is either a
/// branch or a commit.
///
/// A branch reads as a branch name, exactly as it always has. A commit reads as
/// what it is: the commit glyph, the short reference in the data face — the same
/// face the log paints hashes in, so a reference always looks like a reference
/// — and its subject as a quiet annotation. A 40-character hash would be neither
/// legible nor a value anyone recognises, and the subject is what tells two
/// commits apart at a glance.
///
/// The subject is dropped rather than squeezed when the row is narrow, and the
/// reference never is: a base that cannot be read is worse than a base without a
/// caption.
fn new_branch_base_value(ui: &mut Ui, state: &AppState) {
    let base = &state.ui.dlg.new_branch_base;
    let NewBranchBase::Commit(id) = base else {
        if let NewBranchBase::Branch(name) = base {
            ui.label(name);
        }
        return;
    };
    // The same commit glyph the tag dialog's TARGET row uses, so a row that
    // names a commit says so in one kit's grammar wherever it appears.
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

/// The subject of `cid`, read from the log caches the log pane has already
/// filled — a presentation detail, so it costs no git call, and it is absent
/// (the reference still shows) if the commit is not in the listing on screen.
fn commit_subject(state: &AppState, cid: &str) -> Option<String> {
    let root = state.selected_root.as_ref()?;
    // All three listings, because the log can be scoped to a ref or a path and
    // the right-clicked row came from whichever one is on screen.
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

    // CASCADE BANNER — sibling repos sharing the branch being merged into
    // will want the same merge (issue 28). The banner hands off to the
    // cascade preflight for them.
    let siblings = state.merge_cascade_siblings();
    if !siblings.is_empty() {
        cascade_banner(ui, state, siblings.len());
        ui.add_space(4.0);
    }

    // FOOTER — the shared one: a `RULE_FOOTER` hairline separating the body
    // from the action slot, then the actions right-aligned. The merge dialog
    // used to lay its buttons out in a bare `ui.horizontal` with no rule, so
    // its action slot ran into the cascade banner above it and the dialog read
    // as one undifferentiated column while every other modal had a footer.
    // Right-to-left: `Merge` is named first so it lands rightmost and `Cancel`
    // sits to its left, which is the order this dialog has always painted.
    widgets::dialog_footer(ui, |ui| {
        {
            let can_merge = !target.is_empty();
            let merge_btn = ui.add_enabled(can_merge, egui::Button::new("Merge"));
            if merge_btn.clicked() {
                let root = state.selected_root.clone();
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

/// The preview box (screen 14): "Will create N merge commit(s)" plus the
/// file/insertion/deletion totals the merge brings in. Strategy-dependent
/// wording: a fast-forward creates no merge commit; an already-merged
/// target reads as up to date.
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

/// The cascade banner (screen 14): a warning-toned strip naming how many
/// sibling repos share the branch, with the "View plan →" hand-off.
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
///
/// `--onto` is the option that turns this dialog into the stack-surgery tool:
/// it reveals **two ordered fields** (new base first, old base second) that
/// together say "take everything above this cutoff and put it on that base".
/// One "base" field cannot express the move, and the two are not
/// interchangeable, so the order is the design, and each field is validated in
/// its own right. While the option is on, the plain ONTO BRANCH target is
/// superseded — the old base *is* the upstream git reads — and the dialog says
/// so above the greyed field rather than letting a filled target read as live.
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

    let onto_enabled = state.ui.dlg.rebase_onto_enabled;

    // ONTO BRANCH — greyed while the two base fields are in charge.
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

    // MODE — Interactive replays a plan the developer edits, and a plan has no
    // slot for `--onto`, so the segment is withheld rather than silently
    // overridden while the option is on.
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

    // COMMITS TO REBASE — computed from the live repository before anything
    // runs; cached per base so it recomputes only on change. With `--onto` the
    // sequence that moves is the one ABOVE THE CUTOFF, so the cached base is
    // the old base, not the plain target: showing the target's list here would
    // describe a different rebase from the one about to run.
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
    let was_enabled = state.ui.dlg.rebase_onto_enabled;
    ui.checkbox(
        &mut state.ui.dlg.rebase_onto_enabled,
        "Custom base (--onto)",
    );
    if state.ui.dlg.rebase_onto_enabled
        && !was_enabled
        && state.ui.dlg.rebase_mode == turbogit_domain::model::RebaseMode::Interactive
    {
        // Turning the option on while Interactive is selected would leave the
        // dialog describing a rebase it cannot run, so the mode moves to the
        // plain invocation the two bases drive.
        state.ui.dlg.rebase_mode = turbogit_domain::model::RebaseMode::Standard;
    }
    ui.add_space(6.0);

    // --onto — the pair, destination first. The order is the whole point:
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

    // CROSS-REPO BANNER — sibling repos with the branch checked out or
    // tracking-shared will diverge from the rewritten history (issue 29).
    let siblings = state.rebase_affected_siblings();
    if !siblings.is_empty() && !replay.is_empty() {
        rebase_banner(ui, state, replay.len(), siblings.len() + 1);
        ui.add_space(4.0);
    }

    // FOOTER — the shared one, for the same reason the merge dialog's is: the
    // action slot is its own place, separated from the body by the footer rule.
    // Right-to-left: the start action lands rightmost, `Cancel` to its left.
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
            // The gate reads whichever base the rebase actually runs from: the
            // two `--onto` fields while the option is on, the plain target
            // otherwise. A half-filled pair withholds the action; it is never a
            // confirmation.
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
                    // Per root, by name: a rebase running in another repository
                    // is not this dialog's business.
                    format!(
                        "A {} is already in progress in {}",
                        operation.label(),
                        // The focused root's own name, which is what makes an
                        // in-progress refusal point at ONE repository instead
                        // of the whole workspace — and the empty fallback is the
                        // second level of that: no root selected reads as
                        // "this repository" instead.
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
                // Interactive mode replays the listed commits through the plan
                // path; Standard/Autosquash run a plain rebase with the
                // mode-mapped flags; `--onto` runs the two-base move, whose
                // flags come from the same mapping. All of them refuse a
                // protected branch, and all settle as one rebase (ADR-0020).
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

/// What one `--onto` base field says about itself right now.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BaseVerdict {
    /// Nothing entered — the field's own "Select a … base".
    Empty,
    /// Entered, and nothing in this repository answers to it.
    Missing,
    /// Entered and resolvable.
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

/// One `--onto` base field: the entry itself and the error that belongs to it,
/// painted inside the field rather than in a shared region — the user has to
/// know which of the two is wrong, which is the reason the option is two fields
/// and not one.
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

/// The cross-repo banner (screen 15): a warning-toned strip naming how many
/// commits the rewrite replays across how many repos, with the
/// "View affected →" hand-off to the affected-repo list.
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
            .selected_root
            .as_ref()
            .and_then(|root| state.tag_names(root));
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
        match &validation {
            Ok(()) => ui.colored_label(Palette::STATE_SUCCESS, "✓ valid"),
            Err(reason) => widgets::inline_error(ui, reason),
        };
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

    // OPTIONS
    group_title(ui, "OPTIONS");
    ui.checkbox(&mut state.ui.dlg.tag_push, "Push tag to origin immediately");
    ui.add_space(6.0);

    // FOOTER
    widgets::dialog_footer(ui, |ui| {
        // The first action in a right-to-left footer is rightmost. Create
        // comes first so the visible order stays Cancel · Create tag.
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

/// The target picker row (and TARGET display) for a commit:
/// "<short-sha>  <subject>".
fn commit_row_label(c: &turbogit_domain::model::Commit) -> String {
    format!("{}  {}", widgets::short_commit_ref(&c.id), c.subject())
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
    // The shared footer. Right-to-left, so the painted left-to-right order is
    // the one this dialog has always had: `Shelve selected` · `Cancel`.
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
                    // `canonical_key`, not `c.path`: the selection set is
                    // written by every row click under the absolute,
                    // root-scoped form, so reading the repo-relative one here
                    // matched nothing a real click could produce.
                    if state.ui.selected.contains(&root.canonical_key(c)) {
                        changes.push(c.clone());
                    }
                }
            }
            // A shelf belongs to the root whose changes it holds — the Shelf
            // pane lists per root, and an unshelve parks or restores work in
            // exactly one repository. With no focused root there is no such
            // repository, so there is nothing to record and nothing to stash.
            let Some(root) = state.selected_root.clone() else {
                close(state);
                return;
            };
            let shelf = shelve_stash::make_shelf(&name, &changes, root.clone());
            // A refused record is a refused verb: `add_shelf` writes before it
            // paints and reports the refusal itself, so the dispatch below parks
            // work only under a name the shelf index has actually got — which is
            // the one way an unshelve can find it again.
            if state.add_shelf(shelf).is_err() {
                close(state);
                return;
            }
            // The shelf record above is the IDE's own list; the git side is
            // to park the working changes, with nothing chosen to discard.
            state.dispatch(Operation::shelve(root, Vec::new(), name));
            close(state);
        }
    });
}

/// `Import Patches…` — the Shelf view accepting patch FILES as a source.
///
/// Importing is the reference's deferral, and it defers for real: the bytes are parked
/// beside the shelf index and **nothing is applied**, which is why a stash entry is not
/// the payload here (`shelve` can stash because the work is already in the tree; an
/// imported patch is work no tree holds). `Unshelve` is the later act that puts them in,
/// through the same apply engine the `Apply patch` window drives.
///
/// The root is **the pane's own**: this dialog opens from a shelf list already filtered to
/// one repository, which is the same reasoning the `Shelve` dialog states for its
/// `selected_root`. A row imported here therefore names the only tree it may be unshelved
/// into, and with no root in scope the verb refuses instead of parking work under nobody.
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
                // A refusal keeps the dialog OPEN with its reason in it, the way the
                // create settings dialog keeps a destination that could not be written:
                // the developer's next act is to correct the path, not to reopen it.
                Err(e) => state.ui.dlg.import_patch_error = Some(e.to_string()),
            }
        }
    });
}

fn stash(ui: &mut Ui, state: &mut AppState) {
    ui.label("Message:");
    ui.text_edit_singleline(&mut state.ui.dlg.stash_msg);
    ui.checkbox(&mut state.ui.dlg.stash_keep, "Keep index (--keep-index)");
    // The shared footer, preserving this dialog's three-button left-to-right
    // order: `Stash` · `Pop latest` · `Cancel`.
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

// ------------------------------------------------------- issue 32 dialogs --

/// Rename a branch: the new name, and — where the branch tracks something —
/// the second field that stops it tracking, with the consequence both fields
/// buy stated beside them.
///
/// The consequence line is not decoration. Renaming a local branch leaves the
/// remote branch it tracked in place under its old name, and nothing in the
/// dialog shows that; the checkbox only means anything once the sentence
/// beside it says what the next push will do. A branch that tracks nothing has
/// no second field and no remote left behind, so it gets neither.
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
    // Owned before the footer: the closure below mutates `state`, and a `&str`
    // borrowed out of it earlier would still be live when the closure starts.
    let new = state.ui.dlg.rename_branch_new.trim().to_string();
    let named = !new.is_empty() && new != old;
    // The shared footer, preserving the painted order `Rename` · `Cancel`.
    widgets::dialog_footer(ui, |ui| {
        if ui.button("Cancel").clicked() {
            close(state);
        }
        // Withheld, not greyed: a blank name has nothing to rename, so the
        // control loses its fill and its action together. `KitButton::Primary`
        // keeps the accent fill in its Disabled state, which would paint a
        // promise the click cannot keep.
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
    // The shared footer, preserving the painted order `Swap Branches` ·
    // `Close`. This dialog is the clearest case for it: its body is a
    // scrolling commit list, and without the rule the actions ran straight out
    // of the list with nothing saying where the list stopped.
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

/// What the unmerged-commits disclosure was asked to do — the deferred-intent
/// shape the surfaces here use: the body paints and returns, the host applies.
/// The viewer opened from the post-delete notification restores or closes; the
/// one opened from the pre-flight confirm can only close, because the branch it
/// describes still stands.
pub(crate) enum UnmergedAction {
    Nothing,
    Restore,
    Close,
}

/// The delete disclosure: a header naming **both** sides — the branch and what
/// it was not merged into — over the list of what is lost. One body, opened by
/// the pre-flight confirmation's link and by the post-delete notification's, so
/// the two can never state the fact differently.
///
/// It reads nothing from git: the payload is the snapshot taken while the
/// branch still existed, which is the only moment these commits were
/// addressable by name.
pub(crate) fn unmerged_disclosure(
    ui: &mut Ui,
    state: &AppState,
    unmerged: &UnmergedCommits,
) -> UnmergedAction {
    ui.label(unmerged.header());
    egui::ScrollArea::vertical()
        .max_height(240.0)
        .show(ui, |ui| {
            for c in &unmerged.commits {
                let short = widgets::short_commit_ref(&c.id);
                let line = match &c.subject {
                    Some(subject) => format!("{short}  {subject}"),
                    // A commit whose message could not be read is still a
                    // commit that is being lost: the hash says so, a blank gap
                    // would read as an empty row.
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
        // The reference's `Restore`, offered exactly where there is a branch to
        // restore: reached from the confirmation the branch still stands.
        if state.unmerged_restore_available(unmerged)
            && kit_button(ui, KitButton::Primary, "Restore").clicked()
        {
            action = UnmergedAction::Restore;
        }
    });
    action
}

/// The dialog the post-delete notification's link opens: the
/// reference's "Branch Was Not Fully Merged" list, with the recovery its
/// `&Restore` button names.
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
