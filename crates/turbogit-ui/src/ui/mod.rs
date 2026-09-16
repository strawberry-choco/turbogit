//! UI layout (egui 0.35 API).
//!
//! Issue #9 replaced the old panel layout outright with the IntelliJ-style
//! IDE shell (spec §6): a 38px topbar, 34px toolbar, 48px sidebar rail,
//! 32px tab strip and ~24px status bar — all composed in [`shell`]. The
//! central body routes between the Welcome placeholder ([`welcome`], shown
//! when no project is open) and the active tool window (Commit / Log).
//!
//! This module owns what wraps the shell: global shortcut dispatch lives in
//! `shell::render`, and the floating surfaces below are rendered on top of
//! it every frame — Branches popup, VCS operations popup, command palette,
//! modal dialogs, confirm prompts, the Settings modal (issue #16), and the
//! toast.

pub mod activity_panel;
pub mod banner;
pub mod blame_view;
pub mod branch_widget;
pub mod branches;
pub mod bulk_monitor;
pub mod bulk_preflight;
pub mod cherry_across;
pub mod commit_window;
pub mod components;
pub mod conflict_resolver;
pub mod conflicts;
pub mod dialogs;
pub mod diff;
pub mod hunk_nav;
pub mod icons;
pub mod interactive_rebase;
pub mod log_window;
pub mod multi_selection;
pub mod popups;
pub mod push_dialog;
pub mod remotes_dialog;
pub mod settings_modal;
pub mod shell;
pub mod sidebar;
pub mod smart_groups;
pub mod smart_rule_modal;
pub mod submodules;
pub mod welcome;
pub mod widgets;
pub mod worktrees;

use crate::theme::Palette;
use egui::{Color32, Context, Ui};
use turbogit_app::state::{AppState, Dialog, PendingConfirm, RetryAction, ToastKind};
pub fn render(ui: &mut Ui, state: &mut AppState) {
    // Banner strip (issue #02): when set, paints a severity-tinted
    // strip with deep-link actions above the shell. Each surface can
    // also call `banner::show` locally when a banner is surface-scoped.
    banner::maybe_show(ui, state);

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
        } else if d == Dialog::CherryPickAcross {
            // Issue 16: the cross-repo cherry-pick dialog lives in its own
            // module, next to the cherry run monitor.
            cherry_across::show(ui, state);
        } else if d == Dialog::InteractiveRebase {
            // Issue 30: the interactive rebase editor lives in its own
            // module — Plan / Preview / Log tabs over one plan.
            interactive_rebase::show(ui, state);
        } else if d == Dialog::ManageRemotes {
            // Issue 33: the remotes manager lives in its own module.
            remotes_dialog::show(ui, state);
        } else {
            dialogs::show(ui, state, d);
        }
    }
    render_confirm(ui, state);
    settings_modal::show(ui, state);
    smart_rule_modal::show(ui, state);
    bulk_preflight::show(ui, state);
    bulk_monitor::show(ui, state);
    bulk_monitor::show_cherry(ui, state);
    // Redesigned conflict resolver (issue #22, screen 07): dedicated tool
    // window reachable from the Commit tool window's "Resolve…" entry and
    // from the bulk monitor's Resolve deep link.
    conflict_resolver::render(ui, state);
    render_toast(ui, state);
}

// ----------------------------------------------------------------- toast ---

/// The STATE_* token a toast kind paints with (issue #22, spec §2).
fn toast_kind_color(kind: ToastKind) -> Color32 {
    match kind {
        ToastKind::Success => Palette::STATE_SUCCESS,
        ToastKind::Warning => Palette::STATE_WARNING,
        ToastKind::Error => Palette::STATE_ERROR,
        ToastKind::Info => Palette::STATE_INFO,
    }
}

/// The matching Lucide icon for a toast kind (issue #22).
fn toast_kind_icon(kind: ToastKind) -> icons::Icon {
    match kind {
        ToastKind::Success => icons::Icon::CHECK,
        ToastKind::Warning => icons::Icon::ALERT_TRIANGLE,
        ToastKind::Error => icons::Icon::ALERT_CIRCLE,
        ToastKind::Info => icons::Icon::BELL,
    }
}

fn render_toast(ui: &mut Ui, state: &mut AppState) {
    let Some(toast) = state.ui.toast.clone() else {
        return;
    };
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
    // Semantic kind drives accent bar, icon, and message tint (issue #22).
    let color = toast_kind_color(toast.kind);
    let icon = toast_kind_icon(toast.kind);
    let has_retry = toast.retry.is_some();
    let ctx: Context = ui.ctx().clone();
    egui::Window::new("Notice")
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -40.0))
        .resizable(false)
        .title_bar(false)
        .show(&ctx, |ui| {
            ui.horizontal(|ui| {
                // Kind-colored accent bar along the message (spec §10).
                let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(2), color);
                icons::icon(ui, icon, 16.0, color);
                ui.colored_label(color, &toast.message);
                // Retry button (issue #02): only painted when the toast
                // carries a replay handle. The button consumes the
                // action: clicking it clears the toast and re-dispatches
                // via `AppState::retry`. Dismiss is always available.
                if has_retry
                    && ui.small_button("Retry").clicked()
                    && let Some(action) = state.ui.toast.as_ref().and_then(|t| t.retry.clone())
                {
                    state.ui.toast = None;
                    state.ui.toast_shown_at = None;
                    state.retry(action);
                }
                if ui.small_button("Dismiss").clicked() {
                    state.ui.toast = None;
                    state.ui.toast_shown_at = None;
                }
            });
        });
}

/// Confirmation dialog for destructive actions (Epic C8).
/// Compute the ahead-of-upstream warning line for a delete-branch
/// confirm, if any (issue #02). Reads the branch's tracking upstream
/// from git config and queries `is_ancestor`. Returns `Some(message)`
/// when the branch carries unmerged commits relative to its upstream,
/// `None` when fully merged or when the query fails.
fn branch_ahead_warning(state: &AppState, branch: &str) -> Option<String> {
    let root = state.selected_path()?;
    let remote = state
        .executor
        .config_get(&root, &format!("branch.{branch}.remote"))
        .ok()
        .flatten()?;
    let merge = state
        .executor
        .config_get(&root, &format!("branch.{branch}.merge"))
        .ok()
        .flatten()?;
    // `merge` is a `refs/heads/<name>` path; strip the prefix.
    let upstream_branch = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    // The tracking ref is either `<remote>/<branch>` (real remote) or
    // just `<remote>` when `remote` is itself a local branch name
    // (common in test fixtures and in working trees that branch off
    // another local branch). Probe the real one first; fall back to
    // the plain name on lookup failure.
    let candidates = [
        format!("{remote}/{upstream_branch}"),
        upstream_branch.to_string(),
    ];
    let merged = candidates
        .iter()
        .find_map(|c| state.executor.is_ancestor(&root, c, branch).ok())?;
    if merged {
        None
    } else {
        let used = if state
            .executor
            .is_ancestor(&root, &candidates[0], branch)
            .unwrap_or(false)
        {
            &candidates[0]
        } else {
            &candidates[1]
        };
        Some(format!("'{branch}' is ahead of '{used}' and not merged"))
    }
}

/// Confirmation dialog for destructive actions (Epic C8).
fn render_confirm(ui: &mut Ui, state: &mut AppState) {
    let confirm = match &state.ui.confirm {
        Some(c) => c.clone(),
        None => return,
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Confirm")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .show(&ctx, |ui| match &confirm {
            PendingConfirm::Discard { changes } => {
                // Issue #02: rich confirmation — every affected file is
                // listed so the user can see the blast radius, and
                // "Shelve first" offers a non-destructive alternative
                // alongside Discard / Cancel.
                ui.label(format!(
                    "Discard changes to {} file(s)? This cannot be undone.",
                    changes.len()
                ));
                ui.horizontal(|ui| {
                    // Truncated file list — show up to 8 names; the
                    // count is already in the header so anything past
                    // 8 is summarized.
                    for c in changes.iter().take(8) {
                        ui.label(c.path.display().to_string());
                    }
                    if changes.len() > 8 {
                        ui.label(format!("(+{} more)", changes.len() - 8));
                    }
                });
                ui.horizontal(|ui| {
                    // Shelve first: stash the changes and close the
                    // confirm. The worktree stays clean; the stash
                    // entry preserves the changes for later recovery.
                    if ui.button("Shelve first").clicked() {
                        let root = state.selected_path();
                        let changes_owned = changes.clone();
                        state.ui.confirm = None;
                        if let Some(root) = root {
                            state.retry(RetryAction::Shelve {
                                root,
                                changes: changes_owned,
                                message: format!(
                                    "Shelved {} file(s) before discard",
                                    changes.len()
                                ),
                            });
                        }
                    }
                    // Suffix the label with the file count so it is
                    // unique against any other UI control named
                    // "Discard" (eg. toolbar actions that may carry
                    // the bare word).
                    if ui
                        .button(format!("Discard {}-file changes", changes.len()))
                        .clicked()
                    {
                        // Re-clone from the state's confirm field (it
                        // was already cleared if the user clicked
                        // Shelve first; this branch is the canonical
                        // destructive path).
                        if let Some(c) = state.ui.confirm.clone() {
                            state.run_confirmed(c);
                            state.ui.confirm = None;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::DeleteLocalBranch { name } => {
                ui.label(format!(
                    "Delete local branch '{name}'? This cannot be undone."
                ));
                // Issue #02: ahead-of-upstream warning. Query the
                // engine for the branch's tracking upstream; if it
                // exists and is NOT an ancestor of `name`, the branch
                // carries unmerged commits. The query is synchronous
                if let Some(warning) = branch_ahead_warning(state, name) {
                    ui.colored_label(crate::theme::Palette::STATE_WARNING, warning);
                }
                // Issue 12: what is lost, in human terms.
                if let Some(c) = &state.ui.branches_delete_consequence {
                    ui.label(c.clone());
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        // Capture the tip for the undo window before the
                        // branch is gone (issue 12).
                        if let Some(root) = state.selected_path() {
                            let args = ["rev-parse".to_string(), name.clone()];
                            if let Ok(sha) = state.executor.run_raw(&root, &args) {
                                state.ui.branches_delete_pending =
                                    Some(turbogit_app::state::BranchDeletePending {
                                        root: turbogit_domain::model::RootId(root.clone().into()),
                                        name: name.clone(),
                                        tip_sha: sha.trim().to_string(),
                                    });
                            }
                        }
                        state.run_confirmed(confirm.clone());
                        state.ui.confirm = None;
                        state.ui.branches_delete_consequence = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                        state.ui.branches_delete_consequence = None;
                    }
                });
            }
            PendingConfirm::DeleteRemoteBranch { remote, name } => {
                ui.label(format!(
                    "Delete remote branch '{remote}/{name}'? This cannot be undone."
                ));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::CheckoutDirty {
                root,
                target,
                kind,
            } => {
                // Issue 07: plain-language care, never a bare "cannot
                // checkout". The conflict implication is stated before acting.
                ui.label(format!("You have uncommitted changes. Switch to '{target}'?"));
                ui.label(
                    "Bring along — changes come with you; if any conflict with the other branch, checkout is refused and nothing changes.",
                );
                ui.label(
                    "Set aside — changes are stashed now and can be brought back afterwards.",
                );
                let mut chosen = None;
                ui.horizontal(|ui| {
                    if ui.button("Bring along").clicked() {
                        chosen = Some(0);
                    }
                    if ui.button("Set aside").clicked() {
                        chosen = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
                match chosen {
                    Some(0) => {
                        state.ui.confirm = None;
                        state.checkout_branch_op(root, *kind, target);
                    }
                    Some(1) => {
                        state.ui.confirm = None;
                        state.checkout_branch_set_aside(root, *kind, target);
                    }
                    _ => {}
                }
            }
            PendingConfirm::CheckoutInWorktree { branch, worktree } => {
                // Issue 07: refused up front, naming the worktree — no
                // confusing checkout failure.
                ui.label(format!("'{branch}' is already checked out in another worktree:"));
                ui.label(worktree.display().to_string());
                ui.label("Switch there, or close that worktree first.");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::RemoveWorktree { path } => {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<worktree>");
                ui.label(format!("Remove worktree '{name}'? This cannot be undone."));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::DeinitSubmodule { path } => {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("<submodule>");
                ui.label(format!(
                    "De-init submodule '{name}'? Its working copy is removed."
                ));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::InitHere => {
                ui.label("Initialize a git repository in this directory?");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::CloneRepo => {
                ui.label("Clone the repository at the given URL?");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            PendingConfirm::RevertCommit { commit } => {
                let short = &commit[..7.min(commit.len())];
                ui.label(format!(
                    "Revert commit {short}? A new inverse commit will be created on the current branch."
                ));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
        });
    if !open {
        state.ui.confirm = None;
    }
}
