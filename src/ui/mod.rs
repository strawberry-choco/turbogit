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

pub mod branch_widget;
pub mod commit_window;
pub mod conflicts;
pub mod dialogs;
pub mod diff;
pub mod icons;
pub mod log_window;
pub mod popups;
pub mod push_dialog;
pub mod settings_modal;
pub mod shell;
pub mod welcome;
pub mod widgets;

use crate::state::{AppState, Dialog, PendingConfirm, ToastKind};
use crate::theme::Palette;
use egui::{Color32, Context, Ui};

/// Render one full frame: the IDE shell plus its floating surfaces.
pub fn render(ui: &mut Ui, state: &mut AppState) {
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
        } else {
            dialogs::show(ui, state, d);
        }
    }
    render_confirm(ui, state);
    settings_modal::show(ui, state);
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
                // Kind-colored accent bar along the message (spec §10).
                let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(2), color);
                icons::icon(ui, icon, 16.0, color);
                ui.colored_label(color, &toast.message);
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
    let msg = match &confirm {
        PendingConfirm::Discard { changes } => {
            format!(
                "Discard changes to {} file(s)? This cannot be undone.",
                changes.len()
            )
        }
        PendingConfirm::DeleteLocalBranch { name } => {
            format!("Delete local branch '{name}'? This cannot be undone.")
        }
        PendingConfirm::DeleteRemoteBranch { remote, name } => {
            format!("Delete remote branch '{remote}/{name}'? This cannot be undone.")
        }
        PendingConfirm::InitHere => "Initialize a git repository in this directory?".to_string(),
        PendingConfirm::CloneRepo => "Clone the repository at the given URL?".to_string(),
    };
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Confirm")
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .show(&ctx, |ui| {
            ui.label(&msg);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    // `Window::show` takes `FnOnce`, so `confirm` moves here —
                    // the outer clone above is the only copy.
                    state.run_confirmed(confirm);
                    state.ui.confirm = None;
                }
                if ui.button("Cancel").clicked() {
                    state.ui.confirm = None;
                }
            });
        });
    if !open {
        state.ui.confirm = None;
    }
}
