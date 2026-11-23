//! The interactive rebase editor: the plan rows with their verb menu, the
//! mouse and keyboard reordering, the reword field, the preview and the raw
//! todo, the cautions rail, and the backup-backed start.

use nsh_base::theme;
use nsh_core::prelude::*;
use nsh_core::{TouchEvent, TouchPhase};
use turbogit_app::history_editor;
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, Dialog, RebaseEditorTab};
use turbogit_domain::model::{RebaseAction, RebasePlanEntry};

use crate::bridge::AppHandle;
use crate::screens::{rewrite, short};
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, text_area};
use crate::ui::row::{RowProps, RowState, row};

const ROW_HEIGHT: f32 = 26.0;
const PLAN_HEIGHT: f32 = 360.0;
const VERBS: [(&str, RebaseAction); 7] = [
    ("pick", RebaseAction::Pick),
    ("edit", RebaseAction::Edit),
    ("reword", RebaseAction::Reword),
    ("squash", RebaseAction::Squash),
    ("fixup", RebaseAction::Fixup),
    ("drop", RebaseAction::Drop),
    ("update-ref", RebaseAction::UpdateRef),
];

pub fn layer(handle: AppHandle) -> impl IntoElement {
    bind_shortcuts(&handle);
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.dialog == Some(Dialog::InteractiveRebase)),
        move || {
            let body = handle.clone();
            dynamic(move || {
                body.read(|_| ());
                editor_dialog(body.clone()).into_element()
            })
        },
    )
}

fn close(handle: &AppHandle) {
    handle.update(|s| {
        s.ui.dialog = None;
        s.ui.dlg.rebase_plan = None;
        s.ui.dlg.rebase_base = None;
        s.ui.dlg.rebase_edit_root = None;
        s.ui.dlg.rebase_edit_commit = None;
        s.ui.dlg.rebase_cautions = Vec::new();
        s.ui.dlg.rebase_todo = String::new();
        s.ui.dlg.rebase_todo_error = None;
        s.ui.dlg.rebase_selected = 0;
        s.ui.dlg.rebase_drag_from = None;
        s.ui.dlg.rebase_tab = RebaseEditorTab::default();
    });
}

pub fn editor_dialog(handle: AppHandle) -> impl IntoElement {
    let cancel = handle.clone();
    let start_handle = handle.clone();
    dialog(
        DialogProps::new("interactive-rebase", "Interactive rebase")
            .width(980.0)
            .on_cancel(move || close(&cancel))
            .child(editor_body(handle))
            .action("Start rebase")
            .on_action(move || start(&start_handle)),
    )
}

fn plan_of(handle: &AppHandle) -> Vec<RebasePlanEntry> {
    handle
        .peek(|s| s.ui.dlg.rebase_plan.clone())
        .unwrap_or_default()
}

fn editor_body(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let plan = plan_of(&handle);
    let tab = handle.peek(|s| s.ui.dlg.rebase_tab);
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::rebase_editor("body"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("Interactive rebase")
                        .text_size(px(data::TYPE_PANE_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                )
                .child(
                    text(format!("{} commits", plan.len()))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                ),
        )
        .child(tabs_row(handle.clone(), tab));
    body = body.child(match tab {
        RebaseEditorTab::Plan => plan_tab(handle.clone(), plan).into_element(),
        RebaseEditorTab::Preview => preview_tab(plan).into_element(),
        RebaseEditorTab::Log => todo_tab(handle.clone()).into_element(),
    });
    body.child(rail(handle.clone()))
}

fn tabs_row(handle: AppHandle, active: RebaseEditorTab) -> impl IntoElement {
    let mut row_view = view().flex_row().gap(px(data::SPACING_X));
    for (label, value) in [
        ("Plan", RebaseEditorTab::Plan),
        ("Preview", RebaseEditorTab::Preview),
        ("Log", RebaseEditorTab::Log),
    ] {
        let press = handle.clone();
        row_view = row_view.child(button(
            ButtonProps::new(label)
                .variant(if active == value {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .selected(active == value)
                .test_id(&test_ids::rebase_editor(format!("tab-{label}")))
                .on_press(move || {
                    press.update(move |s| {
                        s.ui.dlg.rebase_tab = value;
                        if value == RebaseEditorTab::Log
                            && let Some(plan) = s.ui.dlg.rebase_plan.clone()
                        {
                            s.ui.dlg.rebase_todo = history_editor::render_todo(&plan);
                        }
                    });
                }),
        ));
    }
    row_view
}

fn plan_tab(handle: AppHandle, plan: Vec<RebasePlanEntry>) -> impl IntoElement {
    let t = theme();
    let selected = handle.peek(|s| s.ui.dlg.rebase_selected);
    let drag_id = signal(Option::<String>::None);
    let drag_start = signal(Option::<(f32, usize)>::None);
    let height = (plan.len() as f32 * ROW_HEIGHT).clamp(ROW_HEIGHT, PLAN_HEIGHT);
    let mut list = virtual_list(plan.len(), {
        let row_handle = handle.clone();
        move |at| {
            let index = at.index();
            let entries = plan_of(&row_handle);
            let Some(entry) = entries.get(index).cloned() else {
                return view().into_element();
            };
            plan_row(
                row_handle.clone(),
                entry,
                index,
                index == selected,
                entries.len(),
                drag_id,
                drag_start,
            )
            .into_element()
        }
    });
    list = list
        .estimated_item_size(px(ROW_HEIGHT))
        .flex_1()
        .test_id(&test_ids::rebase_editor("plan"));
    view()
        .w_full()
        .h(px(height))
        .flex_col()
        .rounded(px(data::CONTROL_RADIUS))
        .border(px(1.0))
        .border_color(t.border)
        .child(list)
}

fn plan_row(
    handle: AppHandle,
    entry: RebasePlanEntry,
    index: usize,
    selected: bool,
    total: usize,
    drag_id: Signal<Option<String>>,
    drag_start: Signal<Option<(f32, usize)>>,
) -> impl IntoElement {
    let t = theme();
    let mut content = view()
        .flex_row()
        .w_full()
        .items_center()
        .gap(px(data::DENSITY_LIST_GAP))
        .h(px(ROW_HEIGHT))
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X));
    for (label, action) in VERBS {
        let press = handle.clone();
        let commit = entry.commit.clone();
        let chosen = action.clone();
        let active = entry.action == chosen;
        content = content.child(button(
            ButtonProps::new(label)
                .size(crate::ui::ControlSize::Compact)
                .variant(if active {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .selected(active)
                .test_id(&test_ids::rebase_editor(format!("chip:{label}")))
                .on_press(move || {
                    let action = chosen.clone();
                    let commit = commit.clone();
                    press.update(move |s| {
                        if let Some(plan) = s.ui.dlg.rebase_plan.as_mut()
                            && let Some(row) = plan.iter_mut().find(|row| row.commit == commit)
                        {
                            row.action = action.clone();
                        }
                    });
                }),
        ));
    }
    if entry.action == RebaseAction::Unknown {
        content = content.child(
            text("unknown")
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING),
        );
    }
    let select_commit = entry.commit.clone();
    let drag_handle = handle.clone();
    let drag_from = entry.commit.clone();
    let drag_signal = drag_id;
    let drag_start_signal = drag_start;
    let drag: std::rc::Rc<dyn Fn(&TouchEvent)> = std::rc::Rc::new(move |event: &TouchEvent| {
        let Some(point) = event.points.iter().find(|point| point.changed) else {
            return;
        };
        match event.phase {
            TouchPhase::Start => {
                drag_signal.set(Some(drag_from.clone()));
                drag_start_signal.set(Some((point.page.y.0, index)));
            }
            TouchPhase::Move => {
                let Some(id) = drag_signal.get() else {
                    return;
                };
                let Some((start_y, start_index)) = drag_start_signal.get() else {
                    return;
                };
                let steps = ((point.page.y.0 - start_y) / ROW_HEIGHT).floor() as isize;
                let entries = plan_of(&drag_handle);
                let Some(from) = entries.iter().position(|entry| entry.commit == id) else {
                    return;
                };
                let target = (start_index as isize + steps)
                    .clamp(0, entries.len().saturating_sub(1) as isize)
                    as usize;
                if from != target {
                    let mut entries = entries;
                    history_editor::reorder(&mut entries, from, target);
                    drag_handle.update(|s| {
                        s.ui.dlg.rebase_plan = Some(entries);
                        s.ui.dlg.rebase_selected = target;
                    });
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                drag_signal.set(None);
                drag_start_signal.set(None);
            }
        }
    });
    let select_touch = drag.clone();
    content = content.child(
        view()
            .role(AccessibilityRole::Button)
            .label(format!("plan row {}", entry.subject))
            .test_id(&test_ids::rebase_editor("row"))
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .flex_1()
            .claim_touches(nsh_core::element::TouchClaim::on_pan(
                PanActivation::vertical(px(4.0), px(24.0)),
            ))
            .keep_touches(true)
            .on_press({
                let press = handle.clone();
                move || {
                    let commit = select_commit.clone();
                    press.update(move |s| {
                        if let Some(plan) = s.ui.dlg.rebase_plan.as_ref()
                            && let Some(at) = plan.iter().position(|row| row.commit == commit)
                        {
                            s.ui.dlg.rebase_selected = at;
                        }
                    })
                }
            })
            .on_touch(move |event: &TouchEvent| select_touch(event))
            .on_pan_start({
                let commit = entry.commit.clone();
                move |_: &PanEvent| {
                    drag_id.set(Some(commit.clone()));
                    drag_start.set(Some((0.0, index)));
                }
            })
            .on_pan_end({
                let commit = entry.commit.clone();
                let pan_handle = handle.clone();
                move |event: &PanEvent| {
                    let Some(id) = drag_id.get() else {
                        return;
                    };
                    let Some((_, start_index)) = drag_start.get() else {
                        return;
                    };
                    let steps = (event.translation.y.0 / ROW_HEIGHT).floor() as isize;
                    drag_id.set(None);
                    drag_start.set(None);
                    if id != commit || steps == 0 {
                        return;
                    }
                    pan_handle.update(move |s| {
                        let Some(plan) = s.ui.dlg.rebase_plan.as_ref() else {
                            return;
                        };
                        let Some(from) = plan.iter().position(|row| row.commit == id) else {
                            return;
                        };
                        let target = (start_index as isize + steps)
                            .clamp(0, plan.len().saturating_sub(1) as isize)
                            as usize;
                        if from != target {
                            let mut plan = s.ui.dlg.rebase_plan.take().unwrap_or_default();
                            history_editor::reorder(&mut plan, from, target);
                            s.ui.dlg.rebase_plan = Some(plan);
                            s.ui.dlg.rebase_selected = target;
                        }
                    });
                }
            })
            .child(
                text(short(&entry.commit))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            )
            .child(
                text(entry.subject.clone())
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground)
                    .flex_1(),
            ),
    );
    let up = handle.clone();
    let down = handle.clone();
    let commit = entry.commit.clone();
    let up_commit = commit.clone();
    let down_commit = commit.clone();
    content = content
        .child(button(
            ButtonProps::new("↑")
                .size(crate::ui::ControlSize::Compact)
                .variant(ButtonVariant::Ghost)
                .disabled(index == 0)
                .test_id(&test_ids::rebase_editor("up"))
                .on_press(move || {
                    let commit = up_commit.clone();
                    up.update(move |s| move_row(s, &commit, -1));
                }),
        ))
        .child(button(
            ButtonProps::new("↓")
                .size(crate::ui::ControlSize::Compact)
                .variant(ButtonVariant::Ghost)
                .disabled(index + 1 >= total)
                .test_id(&test_ids::rebase_editor("down"))
                .on_press(move || {
                    let commit = down_commit.clone();
                    down.update(move |s| move_row(s, &commit, 1));
                }),
        ));
    let mut row_view = row(RowProps::new()
        .state(if selected {
            RowState::Selected
        } else {
            RowState::Default
        })
        .child(content))
    .w_full();
    let row_touch = drag.clone();
    row_view = row_view.on_touch(move |event: &TouchEvent| row_touch(event));
    let mut shell = view().flex_col().child(row_view);
    if entry.action == RebaseAction::Reword && selected {
        let change = handle.clone();
        let commit = entry.commit.clone();
        let message = entry.message.clone().unwrap_or_default();
        shell = shell.child(
            view().flex_col().pl(px(data::INDENT)).child(text_area(
                InputProps::new()
                    .multiline(true)
                    .default_value(message)
                    .placeholder("New message for this commit")
                    .accessibility_label("Reword message")
                    .test_id(&test_ids::rebase_editor("reword"))
                    .on_change(move |text: &str| {
                        let text = text.to_string();
                        let commit = commit.clone();
                        change.update(move |s| {
                            if let Some(plan) = s.ui.dlg.rebase_plan.as_mut()
                                && let Some(row) = plan.iter_mut().find(|row| row.commit == commit)
                            {
                                row.message = Some(text.clone());
                            }
                        });
                    }),
            )),
        );
    }
    shell
}

fn move_row(state: &mut AppState, commit: &str, by: isize) {
    let Some(plan) = state.ui.dlg.rebase_plan.as_mut() else {
        return;
    };
    let Some(from) = plan.iter().position(|e| e.commit == commit) else {
        return;
    };
    let target = (from as isize + by).clamp(0, plan.len().saturating_sub(1) as isize) as usize;
    if from != target {
        history_editor::reorder(plan, from, target);
        state.ui.dlg.rebase_selected = target;
    }
}

fn bind_shortcuts(handle: &AppHandle) {
    for (key, by) in [("shift+down", 1isize), ("shift+up", -1isize)] {
        let press = handle.clone();
        on_key(key, move |_| {
            press.update(|s| {
                if s.ui.dialog != Some(Dialog::InteractiveRebase) {
                    return;
                }
                let Some(plan) = s.ui.dlg.rebase_plan.as_ref() else {
                    return;
                };
                let from = s.ui.dlg.rebase_selected.min(plan.len().saturating_sub(1));
                let target =
                    (from as isize + by).clamp(0, plan.len().saturating_sub(1) as isize) as usize;
                if from != target {
                    let mut plan = s.ui.dlg.rebase_plan.take().unwrap_or_default();
                    history_editor::reorder(&mut plan, from, target);
                    s.ui.dlg.rebase_plan = Some(plan);
                    s.ui.dlg.rebase_selected = target;
                }
            });
        });
    }
    for (key, action) in [
        ("p", RebaseAction::Pick),
        ("r", RebaseAction::Reword),
        ("e", RebaseAction::Edit),
        ("s", RebaseAction::Squash),
        ("f", RebaseAction::Fixup),
        ("u", RebaseAction::UpdateRef),
        ("d", RebaseAction::Drop),
    ] {
        let press = handle.clone();
        on_key(key, move |_| {
            let action = action.clone();
            press.update(move |s| {
                if s.ui.dialog != Some(Dialog::InteractiveRebase) {
                    return;
                }
                let at = s.ui.dlg.rebase_selected;
                if let Some(plan) = s.ui.dlg.rebase_plan.as_mut()
                    && let Some(row) = plan.get_mut(at)
                {
                    row.action = action.clone();
                }
            });
        });
    }
}

fn preview_tab(plan: Vec<RebasePlanEntry>) -> impl IntoElement {
    let t = theme();
    let preview = history_editor::plan_preview(&plan);
    let mut body = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::rebase_editor("preview"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("RESULT PREVIEW")
                        .text_size(px(data::TYPE_SECTION))
                        .text_color(t.muted_foreground)
                        .flex_1(),
                )
                .child(
                    text(format!("{} → {} COMMITS", preview.before, preview.after))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.primary),
                ),
        );
    for entry in &preview.kept {
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(verb_word(&entry.action))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .child(
                    text(short(&entry.commit))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .child(
                    text(entry.subject.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        );
    }
    for fold in &preview.folds {
        let folded: Vec<String> = fold.folded.iter().map(|id| short(id)).collect();
        body = body.child(
            text(format!(
                "{} folded into {}",
                folded.join(" + "),
                short(&fold.into)
            ))
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
        );
    }
    if preview.dropped > 0 {
        body = body.child(
            text(format!(
                "{} dropped",
                if preview.dropped == 1 {
                    "1 commit".to_string()
                } else {
                    format!("{} commits", preview.dropped)
                }
            ))
            .text_size(px(data::TYPE_CHIP))
            .text_color(data::STATE_WARNING),
        );
    }
    body
}

fn verb_word(action: &RebaseAction) -> &'static str {
    match action {
        RebaseAction::Pick => "pick",
        RebaseAction::Reword => "reword",
        RebaseAction::Edit => "edit",
        RebaseAction::Squash => "squash",
        RebaseAction::Fixup => "fixup",
        RebaseAction::Drop => "drop",
        RebaseAction::UpdateRef => "update-ref",
        RebaseAction::Unknown => "unknown",
    }
}

fn todo_tab(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (todo, error) = handle.peek(|s| {
        (
            s.ui.dlg.rebase_todo.clone(),
            s.ui.dlg.rebase_todo_error.clone(),
        )
    });
    let change = handle.clone();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::rebase_editor("todo"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("REBASE-TODO")
                        .text_size(px(data::TYPE_SECTION))
                        .text_color(t.muted_foreground)
                        .flex_1(),
                )
                .child(
                    text(if error.is_some() {
                        "parse error"
                    } else {
                        "generated · editable"
                    })
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(if error.is_some() {
                        data::STATE_WARNING
                    } else {
                        t.muted_foreground
                    }),
                ),
        )
        .child(text_area(
            InputProps::new()
                .multiline(true)
                .default_value(todo)
                .placeholder("REBASE-TODO")
                .accessibility_label("REBASE-TODO")
                .test_id(&test_ids::rebase_editor("todo-input"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    change.update(move |s| {
                        s.ui.dlg.rebase_todo = text.clone();
                        match history_editor::parse_todo(&text) {
                            Ok(parsed) => {
                                s.ui.dlg.rebase_todo_error = None;
                                s.ui.dlg.rebase_plan = Some(parsed);
                            }
                            Err(e) => s.ui.dlg.rebase_todo_error = Some(e.to_string()),
                        }
                    });
                }),
        ));
    if let Some(error) = error {
        body = body.child(
            text(error)
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING),
        );
    }
    body
}

fn rail(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let plan = plan_of(&handle);
    let cautions = handle.peek(|s| s.ui.dlg.rebase_cautions.clone());
    let root = handle.peek(|s| s.ui.dlg.rebase_edit_root.clone());
    let mut rail = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::rebase_editor("rail"));
    if !cautions.is_empty() {
        rail = rail.child(
            text("CAUTIONS")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
        for caution in &cautions {
            rail = rail.child(
                text(rewrite::caution_line(caution))
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(data::STATE_WARNING),
            );
        }
    }
    if let Some(root) = root {
        let mut repos: Vec<(String, String)> = Vec::new();
        if let Some(repo) = handle.peek(|s| {
            s.multi.by_id(&root).map(|repo| {
                (
                    repo.current_branch
                        .clone()
                        .unwrap_or_else(|| "(detached)".into()),
                    repo.id.name(),
                )
            })
        }) {
            repos.push(repo);
        }
        for id in handle.peek(|s| s.rebase_affected_siblings(&root)) {
            if let Some(repo) = handle.peek(|s| {
                s.multi.by_id(&id).map(|repo| {
                    (
                        repo.current_branch
                            .clone()
                            .unwrap_or_else(|| "(detached)".into()),
                        repo.id.name(),
                    )
                })
            }) {
                repos.push(repo);
            }
        }
        rail = rail.child(
            text(format!("AFFECTED REPOS · {}", repos.len()))
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground)
                .test_id(&test_ids::rebase_editor("affected")),
        );
        for (branch, name) in repos {
            rail = rail.child(
                text(format!("{branch}  {name}"))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground),
            );
        }
    }
    rail = rail.child(
        text("SHORTCUTS")
            .text_size(px(data::TYPE_SECTION))
            .text_color(t.muted_foreground),
    );
    for line in [
        "⇧↑↓   Move commit within the plan",
        "P     Pick · reset the row as is",
        "R     Reword · change the message",
        "E     Edit · stop to amend the commit",
        "S     Squash into the commit above",
        "F     Fixup · discard the message",
        "U     Update Ref · follow the rewrite",
        "D     Drop the commit from the plan",
    ] {
        rail = rail.child(
            text(line)
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }
    let backup = handle.peek(|s| s.rewrite_backup_ref());
    rail = rail
        .child(
            text("RECOVERY")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            text(backup)
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        )
        .child(
            text("Abort at any time to restore the pre-rewrite state.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    let held = handle.peek(|s| {
        s.ui.dlg
            .rebase_edit_root
            .as_ref()
            .and_then(|root| s.holding_operation(root.as_path()))
    });
    if rewrite::operation_in_progress(held)
        && let Some(root) = handle.peek(|s| s.ui.dlg.rebase_edit_root.clone())
    {
        let abort = handle.clone();
        rail = rail.child(button(
            ButtonProps::new("Abort & restore")
                .variant(ButtonVariant::Danger)
                .test_id(&test_ids::rebase_editor("abort"))
                .on_press(move || {
                    let root = root.clone();
                    abort.update(move |s| {
                        s.dispatch(shell_writes::rewrite::abort_interactive_rebase(&root));
                    });
                    close(&abort);
                }),
        ));
    }
    rail = rail.child(
        text(format!(
            "Ready to rebase {}{}",
            if plan.len() == 1 {
                "1 commit".to_string()
            } else {
                format!("{} commits", plan.len())
            },
            {
                let estimate = history_editor::estimate(&plan);
                if estimate.is_empty() {
                    String::new()
                } else {
                    format!(" · {estimate}")
                }
            }
        ))
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.muted_foreground)
        .test_id(&test_ids::rebase_editor("footer")),
    );
    rail
}

fn start(handle: &AppHandle) {
    let plan = plan_of(handle);
    let Some(root) = handle.peek(|s| s.ui.dlg.rebase_edit_root.clone()) else {
        return;
    };
    let branch = handle.peek(|s| {
        s.multi
            .by_id(&root)
            .and_then(|r| r.current_branch.clone())
            .unwrap_or_default()
    });
    let settings = handle.peek(|s| s.settings.clone());
    handle.update(move |s| {
        s.dispatch(Operation::rebase_interactive(
            &root, &branch, plan, &settings, true,
        ));
        s.ui.dialog = None;
        s.ui.dlg.rebase_plan = None;
    });
}

pub fn editor_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let open = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("rebase_editor"))
        .child(
            text("Interactive rebase")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(button(
            ButtonProps::new("Edit the plan…")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::rebase_editor("open"))
                .on_press(move || open_interactive(open.clone())),
        ))
        .child(
            text("Choose a commit to plan its history rewrite.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
}

fn open_interactive(handle: AppHandle) {
    let Some(root) = handle.peek(|s| s.multi.roots.first().map(|r| r.id.clone())) else {
        return;
    };
    let Some(commits) = handle.peek(|s| s.recent_commits(&root, 50)) else {
        return;
    };
    // The oldest commit that has a parent: a plan starting at the root commit
    // has no base to rewrite from.
    let Some(commit) = commits
        .iter()
        .rev()
        .find(|c| handle.peek(|s| s.commit_base_ref(&root, &c.id).is_some()))
        .cloned()
    else {
        return;
    };
    handle.update(|s| s.open_interactive_rebase(&root, &commit.id));
}

/// Whether a plan row at `index` is the selected row.
pub fn is_selected(state: &AppState, index: usize) -> bool {
    state.ui.dlg.rebase_selected == index
}
