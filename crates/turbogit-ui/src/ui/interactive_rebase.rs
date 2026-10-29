use egui::{Align, Layout, Sense, Ui};
use turbogit_app::history_editor;
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, RebaseEditorTab};
use turbogit_domain::model::RebaseAction;

use crate::theme::Palette;
use crate::ui::components;
use crate::ui::widgets;

const CHIPS: [(&str, RebaseAction); 7] = [
    ("pick", RebaseAction::Pick),
    ("edit", RebaseAction::Edit),
    ("reword", RebaseAction::Reword),
    ("squash", RebaseAction::Squash),
    ("fixup", RebaseAction::Fixup),
    ("drop", RebaseAction::Drop),
    ("update-ref", RebaseAction::UpdateRef),
];

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Interactive Rebase")
        .open(&mut open)
        .default_width(1240.0)
        .show(&ctx, |ui| body(ui, state));
    if !open {
        close(state);
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
    state.ui.dlg.rebase_plan = None;
    state.ui.dlg.rebase_base = None;
    state.ui.dlg.rebase_cautions = Vec::new();
    state.ui.dlg.rebase_todo = String::new();
    state.ui.dlg.rebase_todo_error = None;
    state.ui.dlg.rebase_selected = 0;
    state.ui.dlg.rebase_drag_from = None;
    state.ui.dlg.rebase_tab = RebaseEditorTab::default();
}

fn ensure_plan(state: &mut AppState) {
    if state.ui.dlg.rebase_plan.is_some() {
        return;
    }
    let Some(cid) = state.ui.selected_commit.clone() else {
        return;
    };
    let Some(id) = state.selected_root.clone() else {
        return;
    };
    let Some(base) = state.commit_base_ref(&id, &cid) else {
        return;
    };
    let Some(plan) = state.rebase_plan(&id, &base) else {
        return;
    };
    state.ui.dlg.rebase_base = Some(base);
    state.ui.dlg.rebase_cautions = state.rebase_cautions(&id, &plan);
    state.ui.dlg.rebase_todo = history_editor::render_todo(&plan);
    state.ui.dlg.rebase_plan = Some(plan);
}

fn body(ui: &mut Ui, state: &mut AppState) {
    ensure_plan(state);
    let Some(mut plan) = state.ui.dlg.rebase_plan.take() else {
        ui.label("Select a commit in the Log tab first, then open this editor.");
        if ui.button("Close").clicked() {
            close(state);
        }
        return;
    };

    ui.horizontal(|ui| {
        ui.strong("Interactive rebase");
        ui.label(egui::RichText::new(format!("{} commits", plan.len())).color(Palette::INK_3));
    });

    ui.horizontal(|ui| {
        for (label, tab) in [
            ("Plan", RebaseEditorTab::Plan),
            ("Preview", RebaseEditorTab::Preview),
            ("Log", RebaseEditorTab::Log),
        ] {
            if ui
                .selectable_label(state.ui.dlg.rebase_tab == tab, label)
                .clicked()
            {
                state.ui.dlg.rebase_tab = tab;
                if tab == RebaseEditorTab::Log {
                    state.ui.dlg.rebase_todo = history_editor::render_todo(&plan);
                }
            }
        }
    });
    ui.separator();

    let mut sel = state
        .ui
        .dlg
        .rebase_selected
        .min(plan.len().saturating_sub(1));
    ui.columns(2, |columns| {
        if state.ui.dlg.rebase_tab == RebaseEditorTab::Plan {
            shortcuts(&mut columns[0], &mut plan, &mut sel);
        }
        match state.ui.dlg.rebase_tab {
            RebaseEditorTab::Plan => plan_rows(&mut columns[0], state, &mut plan, &mut sel),
            RebaseEditorTab::Preview => preview_tab(&mut columns[0], &plan),
            RebaseEditorTab::Log => todo_tab(&mut columns[0], state, &mut plan),
        }
        rail(&mut columns[1], state);
    });
    state.ui.dlg.rebase_selected = sel;

    ui.add_space(5.0);
    widgets::footer_rule(ui);
    let mut dispatched = false;
    ui.horizontal(|ui| {
        ui.label(format!(
            "Ready to rebase {}",
            components::counted(plan.len(), "commit", "commits")
        ));
        ui.label(
            egui::RichText::new(format!("· {}", history_editor::estimate(&plan)))
                .color(Palette::INK_3),
        );
        if ui.button("Cancel").clicked() {
            close(state);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Start rebase").clicked() {
                let root = state.selected_root.clone();
                let settings = state.settings.clone();
                let branch = current_branch(state).unwrap_or_default();
                if let Some(root) = root {
                    let op = Operation::rebase_interactive(
                        &root,
                        &branch,
                        plan.clone(),
                        &settings,
                        true,
                    );
                    state.dispatch(op);
                }
                dispatched = true;
                close(state);
            }
        });
    });
    if !dispatched && state.ui.dialog == Some(turbogit_app::state::Dialog::InteractiveRebase) {
        state.ui.dlg.rebase_plan = Some(plan);
    }
}

fn current_branch(state: &AppState) -> Option<String> {
    let id = state.selected_root.as_ref()?;
    state.multi.by_id(id)?.current_branch.clone()
}

fn rail(ui: &mut Ui, state: &mut AppState) {
    widgets::cautions_rail(ui, &state.ui.dlg.rebase_cautions);

    let siblings = state.rebase_affected_siblings();
    widgets::group_title(ui, &format!("AFFECTED REPOS · {}", siblings.len() + 1));
    for id in state
        .selected_root
        .iter()
        .cloned()
        .chain(siblings.iter().cloned())
    {
        if let Some(root) = state.multi.by_id(&id) {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(
                        root.current_branch
                            .clone()
                            .unwrap_or_else(|| "(detached)".into()),
                    )
                    .monospace()
                    .color(Palette::BRAND),
                );
                ui.monospace(id.name());
            });
        }
    }
    ui.add_space(6.0);

    widgets::group_title(ui, "SHORTCUTS");
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
        ui.label(egui::RichText::new(line).monospace());
    }
    ui.add_space(6.0);

    widgets::recovery_note(ui, &state.rewrite_backup_ref());
    ui.add_space(6.0);
    ui.label("Abort at any time to restore the pre-rewrite state.");
    let held = state
        .selected_path()
        .and_then(|root| state.holding_operation(&root));
    if turbogit_app::integrate_service::operation_in_progress(held)
        && ui.button("Abort & restore").clicked()
    {
        if let Some(root) = state.selected_root.clone() {
            state.dispatch(shell_writes::rewrite::abort_interactive_rebase(&root));
        }
        close(state);
    }
}

fn shortcuts(
    ui: &mut Ui,
    plan: &mut Vec<turbogit_domain::model::RebasePlanEntry>,
    sel: &mut usize,
) {
    let typing = ui.ctx().memory(|m| m.focused().is_some());
    if typing {
        return;
    }
    let pressed = |key: egui::Key| ui.ctx().input(|i| i.key_pressed(key));
    let shift = ui.ctx().input(|i| i.modifiers.shift);
    if shift && pressed(egui::Key::ArrowUp) && *sel > 0 {
        history_editor::reorder(plan, *sel, *sel - 1);
        *sel -= 1;
    } else if shift && pressed(egui::Key::ArrowDown) && *sel + 1 < plan.len() {
        history_editor::reorder(plan, *sel, *sel + 1);
        *sel += 1;
    } else if !shift {
        let action = if pressed(egui::Key::P) {
            Some(RebaseAction::Pick)
        } else if pressed(egui::Key::R) {
            Some(RebaseAction::Reword)
        } else if pressed(egui::Key::E) {
            Some(RebaseAction::Edit)
        } else if pressed(egui::Key::S) {
            Some(RebaseAction::Squash)
        } else if pressed(egui::Key::F) {
            Some(RebaseAction::Fixup)
        } else if pressed(egui::Key::U) {
            Some(RebaseAction::UpdateRef)
        } else if pressed(egui::Key::D) {
            Some(RebaseAction::Drop)
        } else {
            None
        };
        if let Some(act) = action {
            history_editor::set_action(plan, *sel, act);
        }
    }
}

fn plan_rows(
    ui: &mut Ui,
    state: &mut AppState,
    plan: &mut Vec<turbogit_domain::model::RebasePlanEntry>,
    sel: &mut usize,
) {
    let ctx = ui.ctx().clone();
    let (any_pressed, any_released, any_click, press_pos, pointer_pos) = ctx.input(|inp| {
        (
            inp.pointer.any_pressed(),
            inp.pointer.any_released(),
            inp.pointer.any_click(),
            inp.pointer.press_origin(),
            inp.pointer.interact_pos(),
        )
    });
    let n = plan.len();
    for i in 0..n {
        let selected = *sel == i;
        // Hand-rolled row (screen 17): the row's interaction is registered
        // BEFORE its contents, so the chips and buttons painted inside it
        // win clicks over the full-row hit area.
        let width = ui.available_width();
        let (rect, row) = ui.allocate_exact_size(egui::vec2(width, 26.0), Sense::click_and_drag());
        widgets::paint_row(
            ui,
            rect,
            components::RowState::from_flags(selected, row.hovered()),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        row.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("plan row {}", plan[i].subject),
            )
        });
        for (lbl, act) in CHIPS {
            if child.selectable_label(plan[i].action == act, lbl).clicked() {
                history_editor::set_action(plan, i, act.clone());
            }
        }
        if plan[i].action == RebaseAction::Unknown {
            child.label(egui::RichText::new("unknown").color(Palette::STATE_WARNING));
        }
        child.monospace(widgets::short_commit_ref(&plan[i].commit));
        child.label(&plan[i].subject);
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.small_button("↓").clicked() && i + 1 < n {
                history_editor::reorder(plan, i, i + 1);
                *sel = i + 1;
            }
            if ui.small_button("↑").clicked() && i > 0 {
                history_editor::reorder(plan, i, i - 1);
                *sel = i - 1;
            }
        });
        let clicked_here = any_click && pointer_pos.is_some_and(|p| rect.contains(p));
        let released_from_here = any_released && state.ui.dlg.rebase_drag_from == Some(i);
        if row.clicked() || clicked_here || released_from_here {
            *sel = i;
        }

        if any_pressed && press_pos.is_some_and(|p| rect.contains(p)) {
            state.ui.dlg.rebase_drag_from = Some(i);
        }
        if let Some(from) = state.ui.dlg.rebase_drag_from {
            if any_released {
                state.ui.dlg.rebase_drag_from = None;
            } else if let Some(pos) = pointer_pos
                && rect.contains(pos)
                && from != i
            {
                history_editor::reorder(plan, from, i);
                *sel = i;
                state.ui.dlg.rebase_drag_from = Some(i);
            }
        }
    }
}

fn preview_tab(ui: &mut Ui, plan: &[turbogit_domain::model::RebasePlanEntry]) {
    let preview = history_editor::plan_preview(plan);
    ui.horizontal(|ui| {
        widgets::group_title(ui, "RESULT PREVIEW");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} → {} COMMITS", preview.before, preview.after))
                    .monospace()
                    .color(Palette::BRAND),
            );
        });
    });
    widgets::note(ui, None, |ui| {
        for e in &preview.kept {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(verb(&e.action)).color(Palette::INK_3));
                ui.monospace(widgets::short_commit_ref(&e.commit));
                ui.label(&e.subject);
            });
        }
        ui.add_space(4.0);
        for fold in &preview.folds {
            let folded: Vec<String> = fold
                .folded
                .iter()
                .map(|s| widgets::short_commit_ref(s))
                .collect();
            ui.label(
                egui::RichText::new(format!(
                    "{} folded into {}",
                    folded.join(" + "),
                    widgets::short_commit_ref(&fold.into)
                ))
                .color(Palette::INK_3),
            );
        }
        if preview.dropped > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{} dropped",
                    components::counted(preview.dropped, "commit", "commits")
                ))
                .color(Palette::STATE_WARNING),
            );
        }
    });
}

fn verb(action: &RebaseAction) -> &'static str {
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

fn todo_tab(
    ui: &mut Ui,
    state: &mut AppState,
    plan: &mut Vec<turbogit_domain::model::RebasePlanEntry>,
) {
    ui.horizontal(|ui| {
        widgets::group_title(ui, "REBASE-TODO");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                egui::RichText::new(if state.ui.dlg.rebase_todo_error.is_some() {
                    "parse error"
                } else {
                    "generated · editable"
                })
                .color(if state.ui.dlg.rebase_todo_error.is_some() {
                    Palette::STATE_WARNING
                } else {
                    Palette::INK_3
                }),
            );
        });
    });
    let response = egui::TextEdit::multiline(&mut state.ui.dlg.rebase_todo)
        .hint_text("REBASE-TODO")
        .code_editor()
        .desired_rows(6)
        .show(ui);
    response
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "REBASE-TODO"));
    if response.response.changed() {
        match history_editor::parse_todo(&state.ui.dlg.rebase_todo) {
            Ok(parsed) => {
                state.ui.dlg.rebase_todo_error = None;
                *plan = parsed;
            }
            Err(e) => state.ui.dlg.rebase_todo_error = Some(e.to_string()),
        }
    }
    if let Some(err) = &state.ui.dlg.rebase_todo_error {
        ui.colored_label(Palette::STATE_WARNING, err);
    }
}
