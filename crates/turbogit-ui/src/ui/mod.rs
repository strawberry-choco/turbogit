//! UI layout (egui 0.35 API).
//!
//! Issue #9 replaced the old panel layout outright with the IntelliJ-style
//! IDE shell (spec §6): the workspace sidebar, a 48px repo header, a 32px
//! tab strip and a 24px status bar — all composed in [`shell`], with no
//! topbar above them. The central body routes between the Welcome
//! placeholder ([`welcome`], shown when no project is open) and the active
//! tool window (Commit / Log).
//!
//! This module owns what wraps the shell: global shortcut dispatch lives in
//! `shell::render`, and the floating surfaces below are rendered on top of
//! it every frame — Branches popup, VCS operations popup, command palette,
//! modal dialogs, confirm prompts, the Settings modal (issue #16), and the
//! toast.
//!
//! Four surfaces share the deferred-intent pattern (a row returns an intent, a
//! collector applies it after rendering, because rows hold `&AppState` and the
//! verbs need `&mut AppState`). They are **four types, not one** — two were both
//! called `RowIntent` and shared nothing, so a sweep for a shared name among them
//! is a mistake — and their collectors differ by design: the commit table needs
//! two slots (a right-click is two facts), `branch_widget` rows are last-wins
//! (one row raises several quick-actions), and the tree emits several per frame.

pub mod activity_panel;
pub mod apply_patch;
pub mod banner;
pub mod blame_view;
pub mod branch_divergence;
pub mod branch_menu;
pub mod branch_tree_view;
pub mod branch_widget;
pub mod branches;
pub mod branches_tree;
pub mod bulk_monitor;
pub mod bulk_preflight;
pub mod cherry_across;
/// The one column-table grammar: the tool panes of `worktrees`, `submodules` and
/// the commit window, plus the fleet matrices; internal like [`kit`].
pub(crate) mod column_table;
pub mod commit_menu;
pub mod commit_window;
pub mod components;
pub mod conflict_resolver;
pub mod conflicts;
pub mod create_patch;
pub mod dialogs;
pub mod diff;
/// The `Get from Branch` window and its three-rung
/// comparison list; internal to the dialog host like [`cherry_across`].
pub mod get_from_branch;
pub mod hunk_nav;
pub mod icons;
pub mod interactive_rebase;
pub(crate) mod kit;
pub mod log_window;
pub mod multi_selection;
pub mod popups;
pub mod project_tree;
pub mod push_control;
pub mod push_dialog;
pub mod push_rejected;
pub mod remotes_dialog;
pub mod reword_editor;
pub mod rewrite_gating;
pub mod rewrite_preflight;
pub mod rewrite_undo;
pub mod settings_modal;
pub mod shell;
pub mod sidebar;
pub mod smart_groups;
pub mod smart_rule_modal;
pub mod status_inflight;
pub mod submodules;
pub mod tree_selection;
pub mod unshelve_dialog;
pub mod welcome;
pub mod widgets;
pub mod worktrees;

use crate::theme::Palette;
use crate::ui::components::{KitButton, kit_button};
use egui::{Color32, Context, Ui};
use turbogit_app::operation::Operation;
use turbogit_app::state::{AppState, Dialog, PendingConfirm, ToastKind};
pub fn render(ui: &mut Ui, state: &mut AppState) {
    // Banner strip (issue #02): when set, paints a severity-tinted
    // strip with deep-link actions above the shell. This is the app-wide
    // host; the accent strip itself is `widgets::accent_bar`.
    banner::maybe_show(ui, state);

    // Shell frame + central body (Welcome placeholder or tool window).
    // The five frozen shortcuts (ADR-0009) dispatch inside the shell.
    shell::render(ui, state);

    // Floating surfaces.
    branch_widget::branches_popup(ui, state);
    popups::vcs_operations(ui, state);
    popups::command_palette(ui, state);
    // Workspace picker (issue #34): painted after the shell and after the
    // palette so a same-frame open works from either trigger — the sidebar's
    // workspace header row, or the palette's Switch Workspace action.
    popups::workspace_picker(ui, state);
    // The clipboard is a `Context` act, and the create routes decide its content:
    // drained on every frame, because the route that copies opens no dialog and so
    // would never reach a drain living inside one.
    create_patch::drain_clipboard(ui.ctx(), state);
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
        } else if d == Dialog::RewritePreflight {
            // Ticket 10: the preflight a targeted history verb shows before it
            // runs, for both verbs — its own module, next to the plan editor it
            // shares a rail and a backup ref with.
            rewrite_preflight::show(ui, state);
        } else if d == Dialog::Reword {
            // Ticket 11: the commit-message editor Reword commit opens first,
            // next to the preflight its confirm opens.
            reword_editor::show(ui, state);
        } else if d == Dialog::ManageRemotes {
            // Issue 33: the remotes manager lives in its own module.
            remotes_dialog::show(ui, state);
        } else if d == Dialog::GetFromBranch {
            // The fifth integration verb's window lives in its own module — it owns
            // a rung list of its own, and `dialogs.rs` is the host for the
            // field-form dialogs, not for a second file tree.
            get_from_branch::show(ui, state);
        } else if d == Dialog::Unshelve {
            // Surface 07: the unshelve window owns a per-root shelf picker and the
            // remove-after-apply choice, so it lives in its own module rather than in
            // the field-form host `dialogs.rs` is.
            unshelve_dialog::show(ui, state);
        } else if d == Dialog::ApplyPatch {
            // The apply window owns a patch payload, git's
            // preflight verdict and the three-way ending of the last apply, none of
            // which is a field form `dialogs.rs` hosts.
            apply_patch::show(ui, state);
        } else if d == Dialog::CreatePatchComment || d == Dialog::PatchFileSettings {
            // The same surface's other half, in one module: the two create dialogs are
            // one flow whose first step names the file the second step writes, and the
            // settings dialog's removal count is state this flow holds between them.
            create_patch::show(ui, state);
        } else {
            dialogs::show(ui, state, d);
        }
    }
    // The `When patch is created: Ask` follow-up, painted outside the dialog slot: the
    // settings step that produced it has already closed, and an ask that could not
    // coexist with `Cancel` on another window would be a policy that vanishes on contact.
    create_patch::created_patch_ask(ui, state);
    render_confirm(ui, state);
    // The classified reply to a rejected push: unconditional, like
    // every other floating surface, because the Push dialog has already closed
    // by the time the rejection settles.
    push_rejected::render(ui, state);
    settings_modal::show(ui, state);
    smart_rule_modal::show(ui, state);
    bulk_preflight::show(ui, state);
    bulk_monitor::show(ui, state);
    bulk_monitor::show_cherry(ui, state);
    // The rollback offer for a branch run that landed in some roots
    // and failed in others. Rendered after the monitor it refers to, so the
    // offer floats above the run whose per-root rows named the failure.
    branch_divergence::render(ui, state);
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
    let ctx: Context = ui.ctx().clone();
    egui::Window::new("Notice")
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -40.0))
        .resizable(false)
        .title_bar(false)
        .show(&ctx, |ui| {
            ui.horizontal(|ui| {
                // Kind-colored accent bar along the message (spec §10) —
                // the same shared strip the banner leads with.
                widgets::accent_bar(ui, color);
                icons::icon(ui, icon, 16.0, color);
                ui.colored_label(color, &toast.message);
                // `Dismiss` is the toast's only action (ADR-0020): no
                // production path could ever hand a toast a replay handle, so
                // the `Retry` button was deleted as unreachable rather than
                // repaired. Bringing it back is a feature with its own ticket.
                if ui.small_button("Dismiss").clicked() {
                    state.ui.toast = None;
                    state.ui.toast_shown_at = None;
                }
            });
        });
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
        // A pending confirmation is answered, not hunted for, so it outranks every
        // ordinary window: the surface that armed it — `Get from Branch`'s rung list
        // is the case that puts the two over the same pixels — is free to raise
        // itself on a later click and bury the question's buttons underneath.
        // `Order::Foreground` is egui's documented order for a window that stays on
        // top, and a confirmation is the one floating surface that always is.
        .order(egui::Order::Foreground)
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
                        let changes_owned = changes.clone();
                        let n = changes.len();
                        state.ui.confirm = None;
                        if let Some(root) = state.selected_root.clone() {
                            state.dispatch(Operation::shelve(
                                root,
                                changes_owned,
                                format!("Shelved {n} file(s) before discard"),
                            ));
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
                if let Some(warning) = state.branch_ahead_warning(name) {
                    ui.colored_label(crate::theme::Palette::STATE_WARNING, warning);
                }
                // Issue 12: what is lost, in human terms.
                if let Some(c) = &state.ui.branches_delete_consequence {
                    ui.label(c.clone());
                }
                // The count is a **link**, and it is here rather than only in
                // the notification after the ref is gone: TurboGit asks first,
                // so the evidence belongs at the moment of decision. Quiet, so
                // the confirm keeps one filled control and OK stays the thing
                // that looks pressable.
                if let Some(unmerged) = state.ui.branches_delete_unmerged.clone()
                    && kit_button(ui, KitButton::Quiet, &unmerged.link_label()).clicked()
                {
                    state.open_unmerged_commits(&unmerged);
                }
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        // Capture the tip for the undo window before the
                        // branch is gone (issue 12).
                        if let Some(root) = state.selected_root.clone()
                            && let Some(tip_sha) = state.branch_tip(&root, name)
                        {
                            // Read here rather than at settlement: the delete
                            // removes the local ref, and with it the upstream
                            // entry and the commits the branch alone reached.
                            let upstream = state.branch_upstream(&root, name);
                            let unmerged = state.ui.branches_delete_unmerged.clone();
                            state.ui.branches_delete_pending =
                                Some(turbogit_app::state::BranchDeletePending {
                                    root,
                                    name: name.clone(),
                                    tip_sha,
                                    upstream,
                                    unmerged,
                                });
                        }
                        state.run_confirmed(confirm.clone());
                        state.ui.confirm = None;
                        state.ui.branches_delete_consequence = None;
                        state.ui.branches_delete_unmerged = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                        state.ui.branches_delete_consequence = None;
                        state.ui.branches_delete_unmerged = None;
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
                detach,
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
                        if *detach {
                            state.checkout_detached_op(root, target);
                        } else {
                            state.checkout_branch_op(root, *kind, target);
                        }
                    }
                    Some(1) => {
                        state.ui.confirm = None;
                        if *detach {
                            state.checkout_detached_set_aside(root, target);
                        } else {
                            state.checkout_branch_set_aside(root, *kind, target);
                        }
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
                let short = widgets::short_commit_ref(commit);
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
            PendingConfirm::GetFileFromBranch { root, source, path } => {
                // The whole blast radius, stated because the reference documents no
                // confirm step at all for this verb, an absence this app departs from
                // on purpose: one file, in one named root, losing its
                // entire current contents — and nothing committed, since the take is a
                // working-tree write the developer then commits by hand.
                ui.label(format!(
                    "Replace {} in {} with the entire contents it has on '{source}'? The file's current contents are lost, and nothing is committed.",
                    path.display(),
                    root.name()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Replace file").clicked() {
                        state.run_confirmed(confirm);
                        state.ui.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        state.ui.confirm = None;
                    }
                });
            }
            // Surface 07's two destructive verbs. **Whether the reference
            // confirms either of them was never observed at runtime**, so these
            // carry TurboGit's own confirming stance — the documented fallback,
            // recorded as a divergence rather than a silent invention.
            PendingConfirm::DeleteShelf { name, .. } => {
                ui.label(format!(
                    "Delete shelf '{name}'? Its row leaves the shelf, and the changes it parked \
                     are dropped from this repository's stash list."
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
            PendingConfirm::DeleteStash { index, .. } => {
                ui.label(format!(
                    "Delete stash {}? The entry is removed, and with it the only copy of what it \
                     holds.",
                    turbogit_services::shelve_stash::stash_ref(*index)
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
