//! The commit card: the message well, the amend and options row, the recent
//! messages, and the Commit control. Every verb routes through the state
//! layer's `dispatch_commit` / `run_commit_across_roots`, so the index is the
//! only thing a commit reads.

use std::sync::Arc;
use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::state::AppState;

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::icon::Icon;
use crate::ui::icon_button::{IconButtonProps, icon_button};
use crate::ui::input::{InputProps, text_area};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};
use crate::ui::popover::{PopoverProps, popover};

const TICK_MS: u64 = 120;

/// The card's held read of the state's commit fields: the message emptiness,
/// the amend and hook flags, and the recent list.
#[derive(Clone)]
struct CommitWindow(Arc<CommitData>);

#[derive(PartialEq)]
struct CommitData {
    empty_message: bool,
    can_commit: bool,
    amend: bool,
    no_verify: bool,
    recent: Vec<String>,
}

impl PartialEq for CommitWindow {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl std::ops::Deref for CommitWindow {
    type Target = CommitData;

    fn deref(&self) -> &CommitData {
        &self.0
    }
}

fn snapshot(state: &AppState) -> CommitData {
    CommitData {
        empty_message: state.ui.commit_message.trim().is_empty(),
        can_commit: !state.ui.commit_message.trim().is_empty() && state.has_commit_target(),
        amend: state.ui.amend,
        no_verify: state.ui.commit_no_verify,
        recent: state.ui.recent_messages.iter().take(6).cloned().collect(),
    }
}

/// The standalone Commit route: the card over its own page.
pub fn commit_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let card_handle = handle;
    view().flex_1().flex_col().bg(t.background).child(
        view()
            .w(px(420.0))
            .p(px(data::PANEL_PADDING))
            .child(commit_card(card_handle)),
    )
}

/// The commit card the Commit tab docks and the commit route paints.
pub fn commit_card(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let window = signal(CommitWindow(Arc::new(handle.peek(snapshot))));
    let generation = signal(0_u64);

    let tick = handle.clone();
    let tick_window = window;
    set_interval(Duration::from_millis(TICK_MS), move || {
        let next = tick.peek(snapshot);
        if next != *tick_window.get().0 {
            tick_window.set(CommitWindow(Arc::new(next)));
        }
    });

    let message_handle = handle.clone();
    let message_generation = generation;
    let options_window = window;
    let no_verify_handle = handle.clone();
    let amend_handle = handle.clone();
    let amend_window = window;
    let commit_handle = handle.clone();
    let commit_window = window;
    let push_handle = handle.clone();
    let recent_handle = handle.clone();
    let recent_generation = generation;
    let recent_window = window;
    let shelve_handle = handle.clone();
    let stash_handle = handle.clone();

    let amend_checked = move || {
        if amend_window.get().amend {
            CheckboxState::Checked
        } else {
            CheckboxState::Unchecked
        }
    };
    let no_verify_checked = move || {
        if options_window.get().no_verify {
            CheckboxState::Checked
        } else {
            CheckboxState::Unchecked
        }
    };
    let can_commit = move || !commit_window.get().empty_message && commit_window.get().can_commit;

    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .rounded(px(data::CARD_RADIUS))
        .bg(t.card)
        .test_id(&test_ids::route("commit"))
        .child(pane_header(
            PaneHeaderProps::new("Commit message").test_id(&test_ids::commit("header")),
        ))
        .child(
            view()
                .p(px(data::PANEL_PADDING))
                .flex_col()
                .gap(px(data::SPACING_Y))
                .child(
                    text_area(
                        InputProps::new()
                            .multiline(true)
                            .placeholder("Description")
                            .accessibility_label("Commit message")
                            .default_value_of({
                                let read = message_handle.clone();
                                move || read.peek(|s| s.ui.commit_message.clone())
                            })
                            .remount(move || message_generation.get())
                            .on_change(move |text: &str| {
                                let text = text.to_string();
                                message_handle.update(move |s| s.ui.commit_message = text);
                            })
                            .test_id(&test_ids::commit("message")),
                    )
                    .into_element(),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(data::SPACING_X))
                        .child(popover(
                            PopoverProps::new(icon_button(
                                IconButtonProps::new(Icon::More, "Commit card options")
                                    .test_id(&test_ids::commit("options")),
                            ))
                            .width(300.0)
                            .test_id(&test_ids::commit("options-popover"))
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(data::CELL_GAP))
                                    .child(
                                        checkbox(
                                            CheckboxProps::new(no_verify_checked)
                                                .accessibility_label("Do not run git commit hooks")
                                                .test_id(&test_ids::commit("no-verify"))
                                                .on_change(move |_| {
                                                    no_verify_handle.update(|s| {
                                                        s.ui.commit_no_verify =
                                                            !s.ui.commit_no_verify
                                                    });
                                                }),
                                        )
                                        .into_element(),
                                    )
                                    .child(
                                        text("Do not run git commit hooks")
                                            .text_size(px(data::TYPE_CONTROL))
                                            .text_color(t.muted_foreground),
                                    ),
                            ),
                        ))
                        .child(view().flex_1())
                        .child(checkbox(
                            CheckboxProps::new(amend_checked)
                                .accessibility_label("Amend")
                                .test_id(&test_ids::commit("amend"))
                                .on_change(move |_| {
                                    let generation = message_generation;
                                    let seeded = amend_handle.update_with(|s| {
                                        s.ui.amend = !s.ui.amend;
                                        if s.ui.amend {
                                            s.prefill_amend_message();
                                            true
                                        } else {
                                            false
                                        }
                                    });
                                    if seeded {
                                        generation.update(|n| *n += 1);
                                    }
                                }),
                        ))
                        .child(
                            text("Amend")
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.muted_foreground),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(button(
                            ButtonProps::new("Commit")
                                .variant(ButtonVariant::Primary)
                                .disabled(!can_commit())
                                .test_id(&test_ids::commit("action"))
                                .on_press({
                                    let commit_handle = commit_handle.clone();
                                    move || do_commit(&commit_handle, false, generation)
                                }),
                        ))
                        .child(button(
                            ButtonProps::new("Commit and Push…")
                                .variant(ButtonVariant::Secondary)
                                .disabled(!can_commit())
                                .test_id(&test_ids::commit("commit-and-push"))
                                .on_press({
                                    let push_handle = push_handle.clone();
                                    move || do_commit(&push_handle, true, generation)
                                }),
                        )),
                )
                .child(
                    // The quiet verbs are a second line: the commit is the
                    // one decision on this card, and four equal buttons made
                    // it look like a choice among four.
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(2.0))
                        .child(button(
                            ButtonProps::new("Shelve…")
                                .variant(ButtonVariant::Ghost)
                                .size(crate::ui::ControlSize::Compact)
                                .test_id(&test_ids::commit("shelve"))
                                .on_press(move || {
                                    shelve_handle.update(|s| s.open_shelve_dialog(None))
                                }),
                        ))
                        .child(button(
                            ButtonProps::new("Stash…")
                                .variant(ButtonVariant::Ghost)
                                .size(crate::ui::ControlSize::Compact)
                                .test_id(&test_ids::commit("stash"))
                                .on_press(move || {
                                    stash_handle.update(|s| {
                                        s.open_dialog(turbogit_app::state::Dialog::Stash, None)
                                    })
                                }),
                        )),
                )
                .child(when(
                    move || !recent_window.get().recent.is_empty(),
                    move || {
                        let t = theme();
                        view()
                            .flex_row()
                            .items_center()
                            .flex_wrap()
                            .gap(px(4.0))
                            .child(
                                text("Recent:")
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(t.muted_foreground),
                            )
                            .children(recent_window.get().recent.iter().map(|message| {
                                let message = message.clone();
                                let short: String = message.chars().take(22).collect();
                                let press_handle = recent_handle.clone();
                                let press_message = message.clone();
                                let generation = recent_generation;
                                button(
                                    ButtonProps::new(short)
                                        .variant(ButtonVariant::Ghost)
                                        .size(crate::ui::ControlSize::Compact)
                                        .test_id(&test_ids::commit(format!(
                                            "recent:{}",
                                            message.replace(' ', "-")
                                        )))
                                        .on_press(move || {
                                            let message = press_message.clone();
                                            press_handle.update(move |s| {
                                                s.ui.commit_message = message;
                                            });
                                            generation.update(|n| *n += 1);
                                        }),
                                )
                                .into_element()
                            }))
                    },
                )),
        )
}

/// The one Commit path: recent history first, the targets from the state,
/// then the message clears and the UI is saved. A second target set commits
/// across roots with the same message.
fn do_commit(handle: &AppHandle, and_push: bool, generation: Signal<u64>) {
    handle.update(|s| {
        let msg = s.ui.commit_message.clone();
        if msg.trim().is_empty() {
            return;
        }
        let amend = s.ui.amend;
        let no_verify = s.ui.commit_no_verify;
        if !s.ui.recent_messages.contains(&msg) {
            s.ui.recent_messages.insert(0, msg.clone());
            s.ui.recent_messages.truncate(12);
        }
        let targets = s.commit_targets();
        match targets.len() {
            0 => {}
            1 => s.dispatch_commit(&targets[0].id, &msg, amend, no_verify, and_push),
            _ => s.run_commit_across_roots(targets, &msg, amend, and_push),
        }
        s.ui.commit_message.clear();
        s.save_durable_ui();
    });
    generation.update(|n| *n += 1);
}
