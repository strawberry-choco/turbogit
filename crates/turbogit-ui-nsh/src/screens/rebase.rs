//! The Rebase dialog: the onto branch, the three modes, the option rows,
//! the `--onto` base pair, the affected-sibling banner, and the start gate
//! that refuses a protected or held repository.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::operation::Operation;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{BranchKind, RebaseMode};

use crate::bridge::AppHandle;
use crate::screens::short;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input};
use crate::ui::row::{RowProps, RowState, row};

const DIALOG_WIDTH: f32 = 560.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BaseVerdict {
    Empty,
    Missing,
    Usable,
}

pub fn open(handle: &AppHandle) {
    handle.update(|s| s.open_dialog(Dialog::Rebase, None));
}

pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.dialog == Some(Dialog::Rebase)),
        move || {
            let body_handle = handle.clone();
            dynamic(move || {
                body_handle.read(|_| ());
                rebase_dialog(body_handle.clone()).into_element()
            })
        },
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| s.ui.dialog = None);
}

fn root_of(state: &AppState) -> Option<turbogit_domain::model::RootId> {
    state.ui.dlg.repo_scope.first().cloned()
}

pub fn rebase_dialog(handle: AppHandle) -> impl IntoElement {
    let cancel_handle = handle.clone();
    let press_handle = handle.clone();
    let disabled = !allowed_to_start(&handle);
    dialog(
        DialogProps::new("rebase", format!("Rebase {}", current_branch(&handle)))
            .width(DIALOG_WIDTH)
            .action_disabled(disabled)
            .on_cancel(move || close(&cancel_handle))
            .child(rebase_body(handle))
            .action(start_label(&press_handle))
            .on_action(move || start(&press_handle)),
    )
}

fn allowed_to_start(handle: &AppHandle) -> bool {
    let ready = handle.peek(|s| {
        if s.ui.dlg.rebase_onto_enabled {
            !s.ui.dlg.rebase_new_base.trim().is_empty()
                && !s.ui.dlg.rebase_old_base.trim().is_empty()
        } else {
            !s.ui.dlg.rebase_onto.trim().is_empty()
        }
    });
    if !ready {
        return false;
    }
    let Some(root) = handle.peek(root_of) else {
        return false;
    };
    let protected = handle.peek(|s| s.rebase_branch_is_protected(&root));
    let held = handle.peek(|s| s.holding_operation(root.as_path()));
    !protected && held.is_none()
}

fn current_branch(handle: &AppHandle) -> String {
    handle.peek(|s| {
        root_of(s)
            .and_then(|id| s.multi.by_id(&id))
            .and_then(|r| r.current_branch.clone())
            .unwrap_or_else(|| "branch".to_string())
    })
}

fn mode_label(mode: RebaseMode) -> &'static str {
    match mode {
        RebaseMode::Interactive => "Interactive",
        RebaseMode::Standard => "Standard",
        RebaseMode::Autosquash => "Autosquash",
    }
}

fn start_label(handle: &AppHandle) -> &'static str {
    match handle.peek(|s| s.ui.dlg.rebase_mode) {
        RebaseMode::Interactive => "Start interactive rebase",
        RebaseMode::Standard => "Start rebase",
        RebaseMode::Autosquash => "Start autosquash rebase",
    }
}

fn branch_names(handle: &AppHandle, root: &Option<turbogit_domain::model::RootId>) -> Vec<String> {
    let current = root
        .as_ref()
        .and_then(|id| handle.peek(|s| s.multi.by_id(id).and_then(|r| r.current_branch.clone())));
    root.as_ref()
        .map(|id| {
            handle.peek(|s| {
                s.multi
                    .by_id(id)
                    .map(|repo| {
                        repo.branches
                            .iter()
                            .filter(|b| b.kind == BranchKind::Local)
                            .filter(|b| Some(&b.name) != current.as_ref())
                            .map(|b| b.name.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        })
        .unwrap_or_default()
}

fn base_verdicts(
    handle: &AppHandle,
    root: &Option<turbogit_domain::model::RootId>,
    onto_enabled: bool,
) -> (BaseVerdict, BaseVerdict) {
    if !onto_enabled {
        return (BaseVerdict::Usable, BaseVerdict::Usable);
    }
    let Some(root) = root else {
        return (BaseVerdict::Empty, BaseVerdict::Empty);
    };
    let (new_value, old_value) = handle.peek(|s| {
        (
            s.ui.dlg.rebase_new_base.trim().to_string(),
            s.ui.dlg.rebase_old_base.trim().to_string(),
        )
    });
    let (new_cached, old_cached) = handle.peek(|s| {
        (
            s.ui.dlg.rebase_new_base_check.clone(),
            s.ui.dlg.rebase_old_base_check.clone(),
        )
    });
    let new_ok = match new_cached.filter(|(key, _)| *key == new_value) {
        Some((_, ok)) => ok,
        None => !new_value.is_empty() && handle.peek(|s| s.revision_exists(root, &new_value)),
    };
    let old_ok = match old_cached.filter(|(key, _)| *key == old_value) {
        Some((_, ok)) => ok,
        None => !old_value.is_empty() && handle.peek(|s| s.revision_exists(root, &old_value)),
    };
    let next_new = Some((new_value.clone(), new_ok));
    let next_old = Some((old_value.clone(), old_ok));
    let stale = handle.peek(|s| {
        s.ui.dlg.rebase_new_base_check != next_new || s.ui.dlg.rebase_old_base_check != next_old
    });
    if stale {
        handle.update(|s| {
            s.ui.dlg.rebase_new_base_check = next_new;
            s.ui.dlg.rebase_old_base_check = next_old;
        });
    }
    (
        verdict_of(&new_value, new_ok),
        verdict_of(&old_value, old_ok),
    )
}

fn verdict_of(value: &str, resolves: bool) -> BaseVerdict {
    if value.is_empty() {
        BaseVerdict::Empty
    } else if resolves {
        BaseVerdict::Usable
    } else {
        BaseVerdict::Missing
    }
}

fn rebase_body(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let root = handle.peek(root_of);
    let onto_enabled = handle.peek(|s| s.ui.dlg.rebase_onto_enabled);
    let rebase_onto = handle.peek(|s| s.ui.dlg.rebase_onto.clone());
    let mode = handle.peek(|s| s.ui.dlg.rebase_mode);
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::rebase("body"))
        .child(
            text("ONTO BRANCH")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    if onto_enabled {
        body = body.child(
            text("The plain target is superseded by the two base fields")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }
    let branches = branch_names(&handle, &root);
    let picker_open = handle.peek(|s| s.ui.dlg.rebase_onto_picker_open);
    let change_handle = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text(if rebase_onto.trim().is_empty() {
                    "Rebase onto…".to_string()
                } else {
                    rebase_onto.clone()
                })
                .text_size(px(data::TYPE_CONTROL))
                .text_color(if rebase_onto.trim().is_empty() {
                    t.muted_foreground
                } else {
                    t.foreground
                })
                .flex_1(),
            )
            .child(button(
                ButtonProps::new("Change…")
                    .variant(ButtonVariant::Ghost)
                    .disabled(onto_enabled)
                    .test_id(&test_ids::rebase("onto"))
                    .on_press(move || {
                        change_handle.update(|s| {
                            s.ui.dlg.rebase_onto_picker_open = !s.ui.dlg.rebase_onto_picker_open
                        })
                    }),
            )),
    );
    if picker_open && !onto_enabled {
        let mut list = view()
            .flex_col()
            .gap(px(data::DENSITY_LIST_GAP))
            .test_id(&test_ids::rebase("onto-options"));
        for name in branches {
            let pick = handle.clone();
            let value = name.clone();
            list = list.child(row(RowProps::new()
                .state(if rebase_onto == name {
                    RowState::Selected
                } else {
                    RowState::Default
                })
                .test_id(&test_ids::rebase(format!("onto-option:{name}")))
                .on_press(move || {
                    let value = value.clone();
                    pick.update(move |s| {
                        s.ui.dlg.rebase_onto = value;
                        s.ui.dlg.rebase_onto_picker_open = false;
                        s.ui.dlg.rebase_preview = None;
                        s.ui.dlg.rebase_preview_key = None;
                    });
                })
                .child(
                    text(name)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )));
        }
        body = body.child(list);
    }
    body = body.child(
        text("MODE")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    let mut modes = view().flex_row().gap(px(data::SPACING_X));
    for value in [
        RebaseMode::Interactive,
        RebaseMode::Standard,
        RebaseMode::Autosquash,
    ] {
        modes = modes.child(mode_row(
            handle.clone(),
            value,
            mode == value,
            onto_enabled && value == RebaseMode::Interactive,
        ));
    }
    body = body.child(modes);

    let (new_verdict, old_verdict) = base_verdicts(&handle, &root, onto_enabled);
    let effective_base = handle.peek(|s| {
        if s.ui.dlg.rebase_onto_enabled {
            if old_verdict == BaseVerdict::Usable {
                s.ui.dlg.rebase_old_base.trim().to_string()
            } else {
                String::new()
            }
        } else {
            s.ui.dlg.rebase_onto.trim().to_string()
        }
    });
    let mut replay: Vec<turbogit_domain::model::RebasePlanEntry> = Vec::new();
    if !effective_base.is_empty() {
        let needs =
            handle.peek(|s| s.ui.dlg.rebase_preview_key.as_deref() != Some(&effective_base));
        if needs {
            let preview = root
                .as_ref()
                .and_then(|root| handle.peek(|s| s.rebase_plan(root, &effective_base)));
            handle.update(|s| {
                s.ui.dlg.rebase_preview = preview;
                s.ui.dlg.rebase_preview_key = Some(effective_base.clone());
            });
        }
        if let Some(plan) = handle.peek(|s| s.ui.dlg.rebase_preview.clone()) {
            body = body.child(
                text(format!("{} COMMITS TO REBASE", plan.len()))
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground)
                    .test_id(&test_ids::rebase("commits-header")),
            );
            let mut list = view()
                .flex_col()
                .gap(px(data::DENSITY_LIST_GAP))
                .test_id(&test_ids::rebase("commits"));
            for entry in &plan {
                list = list.child(
                    text(format!("{}  {}", short(&entry.commit), entry.subject))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                );
            }
            body = body.child(list);
            replay = plan;
        }
    }

    body = body.child(
        text("OPTIONS")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    let (autosquash, update_refs, keep_empty, onto_flag) = handle.peek(|s| {
        (
            s.ui.dlg.rebase_autosquash,
            s.ui.dlg.rebase_update_refs,
            s.ui.dlg.rebase_keep_empty,
            s.ui.dlg.rebase_onto_enabled,
        )
    });
    body = body
        .child(option_row(
            handle.clone(),
            "Autosquash fixup commits",
            autosquash,
            |s| s.ui.dlg.rebase_autosquash = !s.ui.dlg.rebase_autosquash,
        ))
        .child(option_row(
            handle.clone(),
            "Update branches (--update-refs)",
            update_refs,
            |s| s.ui.dlg.rebase_update_refs = !s.ui.dlg.rebase_update_refs,
        ))
        .child(option_row(
            handle.clone(),
            "Keep empty commits",
            keep_empty,
            |s| s.ui.dlg.rebase_keep_empty = !s.ui.dlg.rebase_keep_empty,
        ))
        .child(option_row(
            handle.clone(),
            "Custom base (--onto)",
            onto_flag,
            |s| {
                s.ui.dlg.rebase_onto_enabled = !s.ui.dlg.rebase_onto_enabled;
                if s.ui.dlg.rebase_onto_enabled && s.ui.dlg.rebase_mode == RebaseMode::Interactive {
                    s.ui.dlg.rebase_mode = RebaseMode::Standard;
                }
            },
        ));

    if onto_enabled {
        let super_handle = handle.clone();
        body = body
            .child(
                text("NEW BASE")
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground),
            )
            .child(input(
                InputProps::new()
                    .placeholder("branch or hash")
                    .accessibility_label("New base")
                    .test_id(&test_ids::rebase("new-base"))
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        super_handle.update(move |s| s.ui.dlg.rebase_new_base = text);
                    }),
            ));
        if new_verdict == BaseVerdict::Missing {
            body = body.child(
                text("There is no such branch or hash")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(data::STATE_WARNING)
                    .test_id(&test_ids::rebase("new-base-missing")),
            );
        } else if new_verdict == BaseVerdict::Empty {
            body = body.child(
                text("Select a new base")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(data::STATE_WARNING)
                    .test_id(&test_ids::rebase("new-base-empty")),
            );
        }
        let old_handle = handle.clone();
        body = body
            .child(
                text("OLD BASE")
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground),
            )
            .child(input(
                InputProps::new()
                    .placeholder("branch or hash")
                    .accessibility_label("Old base")
                    .test_id(&test_ids::rebase("old-base"))
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        old_handle.update(move |s| s.ui.dlg.rebase_old_base = text);
                    }),
            ));
        if old_verdict == BaseVerdict::Missing {
            body = body.child(
                text("There is no such branch or hash")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(data::STATE_WARNING)
                    .test_id(&test_ids::rebase("old-base-missing")),
            );
        } else if old_verdict == BaseVerdict::Empty {
            body = body.child(
                text("Select an old base")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(data::STATE_WARNING)
                    .test_id(&test_ids::rebase("old-base-empty")),
            );
        }
    }

    let siblings = root
        .as_ref()
        .map(|root| handle.peek(|s| s.rebase_affected_siblings(root)))
        .unwrap_or_default();
    if !siblings.is_empty() && !replay.is_empty() {
        body = body.child(
            text(format!(
                "⚠ Rewrites {} commits across {}",
                replay.len(),
                if siblings.len() + 1 == 1 {
                    "1 repo".to_string()
                } else {
                    format!("{} repos", siblings.len() + 1)
                }
            ))
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::STATE_WARNING)
            .test_id(&test_ids::rebase("banner")),
        );
    }

    let (protected, held) = match &root {
        Some(root) => (
            handle.peek(|s| s.rebase_branch_is_protected(root)),
            handle.peek(|s| s.holding_operation(root.as_path())),
        ),
        None => (false, None),
    };
    let ready = handle.peek(|s| {
        if s.ui.dlg.rebase_onto_enabled {
            !s.ui.dlg.rebase_new_base.trim().is_empty()
                && !s.ui.dlg.rebase_old_base.trim().is_empty()
        } else {
            !s.ui.dlg.rebase_onto.trim().is_empty()
        }
    });
    let allowed = ready && held.is_none() && !protected;
    let blocker = if protected {
        format!(
            "'{}' is a protected branch — rebasing it is not allowed",
            current_branch(&handle)
        )
    } else if let Some(operation) = held {
        format!("A {} is already in progress", operation.label())
    } else if !ready {
        if onto_enabled {
            "Fill in both base fields".to_string()
        } else {
            "Pick an onto branch first".to_string()
        }
    } else {
        String::new()
    };
    let start_handle = handle.clone();
    let cancel = handle.clone();
    body = body
        .child(button(
            ButtonProps::new(start_label(&handle))
                .variant(ButtonVariant::Primary)
                .disabled(!allowed)
                .test_id(&test_ids::rebase("start"))
                .on_press(move || start(&start_handle)),
        ))
        .child(button(
            ButtonProps::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .test_id(&test_ids::rebase("cancel"))
                .on_press(move || close(&cancel)),
        ));
    if !blocker.is_empty() {
        body = body.child(
            text(blocker)
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING)
                .test_id(&test_ids::rebase("blocker")),
        );
    }
    body
}

fn mode_row(
    handle: AppHandle,
    value: RebaseMode,
    active: bool,
    disabled: bool,
) -> impl IntoElement {
    let t = theme();
    let press = handle;
    row(RowProps::new()
        .state(if active {
            RowState::Selected
        } else {
            RowState::Default
        })
        .test_id(&test_ids::rebase(format!("mode-{}", mode_label(value))))
        .on_press(move || {
            if !disabled {
                press.update(move |s| s.ui.dlg.rebase_mode = value);
            }
        })
        .child(
            text(mode_label(value))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(if disabled {
                    t.muted_foreground
                } else {
                    t.foreground
                }),
        ))
}

fn option_row(
    handle: AppHandle,
    label: &'static str,
    on: bool,
    toggle: impl Fn(&mut AppState) + Copy + 'static,
) -> impl IntoElement {
    let press = handle;
    let shared = std::rc::Rc::new(toggle);
    let change = shared.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .child(checkbox(
            CheckboxProps::new(move || {
                let _ = &change;
                if on {
                    CheckboxState::Checked
                } else {
                    CheckboxState::Unchecked
                }
            })
            .accessibility_label(label)
            .test_id(&test_ids::rebase(format!("option-{label}")))
            .on_change(move |_| {
                let toggle = shared.clone();
                press.update(move |s| {
                    toggle(s);
                    s.ui.dlg.rebase_preview = None;
                    s.ui.dlg.rebase_preview_key = None;
                });
            }),
        ))
        .child(
            text(label)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(theme().foreground),
        )
}

fn start(handle: &AppHandle) {
    let Some(root) = handle.peek(root_of) else {
        return;
    };
    let (mode, opts, onto_enabled, onto, new_base, old_base, preview) = handle.peek(|s| {
        (
            s.ui.dlg.rebase_mode,
            s.rebase_dialog_opts(),
            s.ui.dlg.rebase_onto_enabled,
            s.ui.dlg.rebase_onto.trim().to_string(),
            s.ui.dlg.rebase_new_base.trim().to_string(),
            s.ui.dlg.rebase_old_base.trim().to_string(),
            s.ui.dlg.rebase_preview.clone().unwrap_or_default(),
        )
    });
    if onto_enabled && (new_base.is_empty() || old_base.is_empty()) {
        return;
    }
    if !onto_enabled && onto.is_empty() {
        return;
    }
    let (roots, settings) = handle.peek(|s| {
        let roots: Vec<_> = if s.ui.dlg.repo_scope.is_empty() {
            vec![root.clone()]
        } else {
            s.ui.dlg.repo_scope.clone()
        };
        (roots, s.settings.clone())
    });
    handle.update(move |s| {
        for root in roots {
            let branch = s
                .multi
                .by_id(&root)
                .and_then(|r| r.current_branch.clone())
                .unwrap_or_default();
            let op = if onto_enabled {
                Operation::rebase_with_onto(&root, &branch, &new_base, &old_base, &opts, &settings)
            } else {
                match mode {
                    RebaseMode::Interactive => Operation::rebase_interactive(
                        &root,
                        &branch,
                        preview.clone(),
                        &settings,
                        false,
                    ),
                    RebaseMode::Standard => {
                        Operation::rebase_onto(&root, &branch, &onto, &opts, &settings)
                    }
                    RebaseMode::Autosquash => {
                        Operation::rebase_autosquash(&root, &branch, &onto, &opts, &settings)
                    }
                }
            };
            s.dispatch(op);
        }
        s.ui.dialog = None;
    });
}

pub fn rebase_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let open_handle = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("rebase"))
        .child(
            text("Rebase")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            view()
                .flex_col()
                .gap(px(data::SPACING_Y))
                .rounded(px(data::CARD_RADIUS))
                .border(px(1.0))
                .border_color(t.border)
                .bg(t.card)
                .p(px(data::PANEL_PADDING))
                .child(button(
                    ButtonProps::new("Choose a branch to rebase onto…")
                        .variant(ButtonVariant::Primary)
                        .test_id(&test_ids::rebase("open"))
                        .on_press(move || open(&open_handle)),
                ))
                .child(
                    text("Opening the dialog reads the onto branch and plans the replay.")
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                ),
        )
}
