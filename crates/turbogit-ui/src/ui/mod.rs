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
pub mod changes_selection;
pub mod cherry_across;
pub(crate) mod column_table;
pub mod commit_menu;
pub mod commit_window;
pub mod components;
pub mod conflict_resolver;
pub mod conflicts;
pub mod create_patch;
pub mod dialogs;
pub mod diff;
pub mod get_from_branch;
pub mod hunk_nav;
pub mod icons;
pub mod interactive_rebase;
pub(crate) mod kit;
pub mod log_window;
pub mod multi_selection;
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
    banner::maybe_show(ui, state);

    shell::render(ui, state);

    branch_widget::branches_popup(ui, state);
    popups::vcs_operations(ui, state);
    popups::command_palette(ui, state);
    popups::workspace_picker(ui, state);
    create_patch::drain_clipboard(ui.ctx(), state);
    if let Some(d) = state.ui.dialog {
        if d == Dialog::Push {
            push_dialog::show(ui, state);
        } else if d == Dialog::CherryPickAcross {
            cherry_across::show(ui, state);
        } else if d == Dialog::InteractiveRebase {
            interactive_rebase::show(ui, state);
        } else if d == Dialog::RewritePreflight {
            rewrite_preflight::show(ui, state);
        } else if d == Dialog::Reword {
            reword_editor::show(ui, state);
        } else if d == Dialog::ManageRemotes {
            remotes_dialog::show(ui, state);
        } else if d == Dialog::GetFromBranch {
            get_from_branch::show(ui, state);
        } else if d == Dialog::Unshelve {
            unshelve_dialog::show(ui, state);
        } else if d == Dialog::ApplyPatch {
            apply_patch::show(ui, state);
        } else if d == Dialog::CreatePatchComment || d == Dialog::PatchFileSettings {
            create_patch::show(ui, state);
        } else {
            dialogs::show(ui, state, d);
        }
    }
    create_patch::created_patch_ask(ui, state);
    render_confirm(ui, state);
    push_rejected::render(ui, state);
    settings_modal::show(ui, state);
    shortcuts_window::show(ui, state);
    smart_rule_modal::show(ui, state);
    bulk_preflight::show(ui, state);
    bulk_monitor::show(ui, state);
    bulk_monitor::show_cherry(ui, state);
    branch_divergence::render(ui, state);
    conflict_resolver::render(ui, state);
    diff::show_diff(ui, state);
    blame_view::show_blame(ui, state);
    render_toast(ui, state);
}

fn toast_kind_color(kind: ToastKind) -> Color32 {
    match kind {
        ToastKind::Success => Palette::STATE_SUCCESS,
        ToastKind::Warning => Palette::STATE_WARNING,
        ToastKind::Error => Palette::STATE_ERROR,
        ToastKind::Info => Palette::STATE_INFO,
    }
}

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
    let color = toast_kind_color(toast.kind);
    let icon = toast_kind_icon(toast.kind);
    let ctx: Context = ui.ctx().clone();
    egui::Window::new("Notice")
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -40.0))
        .resizable(false)
        .title_bar(false)
        .show(&ctx, |ui| {
            ui.horizontal(|ui| {
                widgets::accent_bar(ui, color);
                icons::icon(ui, icon, 16.0, color);
                ui.colored_label(color, &toast.message);
                if ui.small_button("Dismiss").clicked() {
                    state.ui.toast = None;
                    state.ui.toast_shown_at = None;
                }
            });
        });
}

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
            for action in clicked {
                confirm::apply(state, &confirm, action);
            }
        });
    if !open {
        state.ui.confirm = None;
    }
}
