//! The feedback layer: the banner strip, and the confirm path every verb
//! routes through. A confirm is always the state layer's `PendingConfirm`;
//! the prompt it carries decides the surface: a multi-line prompt with a
//! disclosure paints the styled dialog, a plain yes-and-no shows the
//! platform alert. Every answer runs `confirm::apply` on the confirm that
//! is live at answer time, never a captured one.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::banner::BannerSeverity;
use turbogit_app::confirm::{self, ConfirmAction, Prompt};
use turbogit_app::state::{AppState, PendingConfirm};

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::alert::{AlertAnswer, alert_dialog};
use crate::ui::button::ButtonVariant;
use crate::ui::dialog::{DialogAction, DialogProps, dialog};

/// The banner strip above the shell: the severity color, the message, and
/// the actions the banner carries. A click removes the action it names and
/// runs it, exactly as the egui strip did.
pub fn banner_layer(handle: AppHandle) -> impl IntoElement {
    let show_handle = handle.clone();
    when(
        move || show_handle.read(|s| s.ui.banner.is_some()),
        move || {
            let t = theme();
            let paint_handle = handle.clone();
            let (severity, message, labels) = paint_handle.peek(|s| {
                let banner = s.ui.banner.as_ref().expect("the layer is showing a banner");
                (
                    banner.severity,
                    banner.message.clone(),
                    banner
                        .actions
                        .iter()
                        .map(|a| a.label.clone())
                        .collect::<Vec<_>>(),
                )
            });
            let color = match severity {
                BannerSeverity::Info => data::STATE_INFO,
                BannerSeverity::Warning => data::STATE_WARNING,
                BannerSeverity::Error => data::STATE_ERROR,
                BannerSeverity::Success => data::STATE_SUCCESS,
            };
            let click_handle = handle.clone();
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .px(px(data::PANEL_PADDING))
                .py(px(data::SPACING_Y))
                .bg(t.card)
                .test_id(&test_ids::banner())
                .child(
                    view()
                        .w(px(data::RAIL_WIDTH))
                        .h(px(data::TYPE_PANE_TITLE))
                        .rounded(px(data::MARK_RADIUS))
                        .bg(color),
                )
                .child(
                    text(message)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(color),
                )
                .children(labels.into_iter().map(move |label| {
                    let click_handle = click_handle.clone();
                    let press_label = label.clone();
                    button_like(label, move || {
                        let press_label = press_label.clone();
                        click_handle.update(move |s| {
                            if let Some(banner) = s.ui.banner.as_mut()
                                && let Some(at) =
                                    banner.actions.iter().position(|a| a.label == press_label)
                            {
                                let action = banner.actions.remove(at);
                                (action.on_click)(s);
                            }
                        });
                    })
                }))
        },
    )
}

/// A quiet text press target for a banner action.
fn button_like(label: String, on_press: impl Fn() + 'static) -> impl IntoElement {
    let t = theme();
    view()
        .role(AccessibilityRole::Button)
        .label(label.clone())
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.foreground)
        .hover(|s| s.bg(t.muted))
        .on_press(on_press)
        .child(text(label).text_size(px(data::TYPE_CONTROL)))
}

/// The confirm surface over the state layer's `PendingConfirm`.
pub fn confirm_layer(handle: AppHandle) -> impl IntoElement {
    let show_handle = handle.clone();
    when(
        move || show_handle.read(|s| s.ui.confirm.is_some()),
        move || {
            let paint_handle = handle.clone();
            let (simple, dialog) = paint_handle.peek(|s| {
                let confirm =
                    s.ui.confirm
                        .clone()
                        .expect("the layer is showing a confirm");
                let prompt = confirm::prompt(s, &confirm);
                let name = confirm_slug(&confirm).to_string();
                if is_simple(&prompt) {
                    (true, None)
                } else {
                    (
                        false,
                        Some(present_dialog(paint_handle.clone(), name, prompt).into_element()),
                    )
                }
            });
            if simple {
                present_alert(handle.clone());
            }
            dialog.unwrap_or_else(|| view().into_element())
        },
    )
}

/// A prompt the platform alert answers: no disclosure, and exactly the
/// confirm-and-close pair.
fn is_simple(prompt: &Prompt) -> bool {
    prompt.disclosure.is_none()
        && prompt.buttons.len() == 2
        && prompt
            .buttons
            .iter()
            .any(|b| b.action == ConfirmAction::Confirm)
        && prompt
            .buttons
            .iter()
            .any(|b| b.action == ConfirmAction::Close)
}

/// Shows the platform alert; the answer runs on the confirm that is live
/// when the answer lands.
fn present_alert(handle: AppHandle) {
    let prompt = handle.peek(|s| {
        s.ui.confirm
            .as_ref()
            .map(|confirm| confirm::prompt(s, confirm))
    });
    let Some(prompt) = prompt else {
        return;
    };
    let lines: Vec<&str> = prompt
        .rows
        .iter()
        .flat_map(|row| row.lines.iter().map(|line| line.text.as_str()))
        .collect();
    let (title, description) = match lines.split_first() {
        Some((first, rest)) => (first.to_string(), rest.join("\n")),
        None => ("Confirm".to_string(), String::new()),
    };
    spawn(async move {
        let answer =
            alert_dialog(title, description, ("cancel", "Cancel"), ("confirm", "OK")).await;
        handle.update(move |s| match answer {
            AlertAnswer::Action(action) if action == "confirm" => {
                run_live(s, ConfirmAction::Confirm)
            }
            _ => run_live(s, ConfirmAction::Close),
        });
    });
}

/// The styled dialog for a prompt with more than a yes and a no: the rows
/// as body lines, the disclosure as a quiet verb, the prompt's buttons in
/// the footer.
fn present_dialog(handle: AppHandle, name: String, prompt: Prompt) -> impl IntoElement {
    let t = theme();
    let rows_handle = handle;
    let cancel_handle = rows_handle.clone();
    let mut props = DialogProps::new(name, "Confirm").on_cancel(move || {
        run_confirm(&cancel_handle, ConfirmAction::Close);
    });
    for row in &prompt.rows {
        for line in &row.lines {
            let text_color = if line.warning {
                data::STATE_WARNING
            } else {
                t.card_foreground
            };
            props = props.child(
                text(line.text.clone())
                    .text_size(px(data::TYPE_BODY))
                    .text_color(text_color),
            );
        }
    }
    if let Some(disclosure) = &prompt.disclosure {
        let disclosure_handle = rows_handle.clone();
        let action = disclosure.action;
        props = props.disclosure(disclosure.label.clone(), move || {
            run_confirm(&disclosure_handle, action);
        });
    }
    for button in prompt.buttons.iter().rev() {
        // The dialog's own Cancel carries the prompt's Close action; drawing
        // the prompt's button too would give the confirm two Cancels.
        if button.action == ConfirmAction::Close {
            continue;
        }
        let button_handle = rows_handle.clone();
        let action = button.action;
        let variant = match action {
            ConfirmAction::Confirm => ButtonVariant::Primary,
            ConfirmAction::Close => ButtonVariant::Ghost,
            _ => ButtonVariant::Secondary,
        };
        props = props.with_action(DialogAction::new(
            button.label.clone(),
            variant,
            move || {
                run_confirm(&button_handle, action);
            },
        ));
    }
    dialog(props)
}

/// Runs `action` on the confirm that is live at press time.
fn run_confirm(handle: &AppHandle, action: ConfirmAction) {
    handle.update(move |s| run_live(s, action));
}

fn run_live(state: &mut AppState, action: ConfirmAction) {
    if let Some(live) = state.ui.confirm.clone() {
        confirm::apply(state, &live, action);
    }
}

/// The dialog name a confirm's selector uses (`dialog:revert-commit`).
fn confirm_slug(confirm: &PendingConfirm) -> &'static str {
    match confirm {
        PendingConfirm::DiscardAcross { .. } => "discard-across",
        PendingConfirm::Discard { .. } => "discard",
        PendingConfirm::DeleteLocalBranch { .. } => "delete-local-branch",
        PendingConfirm::DeleteRemoteBranch { .. } => "delete-remote-branch",
        PendingConfirm::CheckoutDirty { .. } => "checkout-dirty",
        PendingConfirm::CheckoutInWorktree { .. } => "checkout-in-worktree",
        PendingConfirm::RemoveWorktree { .. } => "remove-worktree",
        PendingConfirm::DeinitSubmodule { .. } => "deinit-submodule",
        PendingConfirm::InitHere => "init-here",
        PendingConfirm::CloneRepo => "clone-repo",
        PendingConfirm::RevertCommit { .. } => "revert-commit",
        PendingConfirm::GetFileFromBranch { .. } => "get-file-from-branch",
        PendingConfirm::DeleteShelf { .. } => "delete-shelf",
        PendingConfirm::DeleteStash { .. } => "delete-stash",
    }
}
