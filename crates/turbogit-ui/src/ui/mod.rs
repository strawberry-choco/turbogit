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
/// The Local Changes selection rule: one pure reducer over a row-body click and
/// the modifier it was clicked with.
pub mod changes_selection;
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
/// The chrome the two patch windows share; internal to the surfaces that call it.
pub(crate) mod patch_chrome;
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
pub mod shortcuts_window;
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
use turbogit_app::confirm;
use turbogit_app::state::{AppState, Dialog, ToastKind};
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
    shortcuts_window::show(ui, state);
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
    // The diff window the Log view's diff verbs open: floating over whatever
    // tab is up, so the listing the verb came from stays exactly as it was.
    diff::show_diff(ui, state);
    // The blame window the same view's blame verbs open, in the same chrome
    // and for the same reason: the listing the verb came from stays as it was.
    blame_view::show_blame(ui, state);
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
///
/// Chrome and painting only: the question, its disclosures and what each of its
/// controls does belong to [`confirm`], so this paints the description it is
/// handed and hands the clicked control back to `apply`. A verb cannot be
/// half-painted, because the renderer no longer decides what a verb shows.
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
        .show(&ctx, |ui| {
            let prompt = confirm::prompt(state, &confirm);
            for row in &prompt.rows {
                ui.horizontal(|ui| {
                    for line in &row.lines {
                        if line.warning {
                            ui.colored_label(Palette::STATE_WARNING, line.text.as_str());
                        } else {
                            ui.label(line.text.as_str());
                        }
                    }
                });
            }
            // Quiet, so the confirm keeps one filled control and OK stays the thing
            // that looks pressable.
            if let Some(disclosure) = &prompt.disclosure
                && kit_button(ui, KitButton::Quiet, &disclosure.label).clicked()
            {
                confirm::apply(state, &confirm, disclosure.action);
            }
            let mut clicked = Vec::new();
            ui.horizontal(|ui| {
                for button in &prompt.buttons {
                    if ui.button(button.label.as_str()).clicked() {
                        clicked.push(button.action);
                    }
                }
            });
            // Applied in the row's declared order, not in the order the clicks
            // arrived: the discard's destructive control decides whether the
            // question is still there by reading the slot its own left neighbour
            // clears, so which of the two ran first is part of the behaviour.
            for action in clicked {
                confirm::apply(state, &confirm, action);
            }
        });
    if !open {
        state.ui.confirm = None;
    }
}
