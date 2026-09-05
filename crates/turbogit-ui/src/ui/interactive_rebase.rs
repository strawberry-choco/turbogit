//! Interactive rebase editor (issue 30, screen 17): the plan editor for F5
//! history editing. Plan / Preview / Log tabs over one plan; plan rows with
//! action chips and move buttons plus ⇧↑/⇧↓ and p/s/f/d shortcuts; a RESULT
//! PREVIEW of the post-plan commit sequence; the raw REBASE-TODO editable
//! as text; a right rail with CAUTIONS, AFFECTED REPOS and RECOVERY naming
//! the backup ref; a footer with the time estimate and Start rebase.
//!
//! The plan is built once on open from the selected log commit
//! ([`history_editor::base_of`] → [`history_editor::build_plan`]) and
//! mutated in place; execution goes through the guarded, backup-ref dispatch
//! [`history_editor::execute_with_backup`].

use std::path::Path;

use egui::{Align, Layout, Sense, Ui};
use turbogit_app::state::{AppState, RebaseEditorTab};
use turbogit_domain::model::RebaseAction;
use turbogit_services::history_editor;

use crate::theme::Palette;
use crate::ui::widgets;

/// The action chips a plan row offers (screen 17). `edit` stays parseable in
/// the REBASE-TODO but has no chip — it has no shortcut and the editor's
/// scope is the five chip actions.
const CHIPS: [(&str, RebaseAction); 5] = [
    ("pick", RebaseAction::Pick),
    ("reword", RebaseAction::Reword),
    ("squash", RebaseAction::Squash),
    ("fixup", RebaseAction::Fixup),
    ("drop", RebaseAction::Drop),
];

/// Render the editor while `ui.dialog` is `Dialog::InteractiveRebase`.
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    egui::Window::new("Interactive Rebase")
        .open(&mut open)
        .default_width(1080.0)
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

/// Build the plan (and its cached cautions + todo text) on first frame from
/// the selected log commit. Without a selection the editor shows its hint.
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
    let Some(base) = history_editor::base_of(state.executor.as_ref(), &id.0, &cid) else {
        return;
    };
    let Ok(plan) = history_editor::build_plan(state.executor.as_ref(), &id.0, &base) else {
        return;
    };
    state.ui.dlg.rebase_base = Some(base);
    state.ui.dlg.rebase_cautions = history_editor::cautions(state.executor.as_ref(), &id.0, &plan);
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

    // Header (screen 17): the title plus the plan size it starts from.
    ui.horizontal(|ui| {
        ui.strong("Interactive rebase");
        ui.label(egui::RichText::new(format!("{} commits", plan.len())).color(Palette::INK_3));
    });

    // Tab strip: Plan / Preview / Log.
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
    // Left: the active tab's pane. Right: the CAUTIONS / AFFECTED REPOS /
    // SHORTCUTS / RECOVERY rail (screen 17).
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

    // Footer (screen 17): the estimate on the left, Cancel and Start on the
    // right. Start goes through the guarded, backup-backed dispatch.
    ui.separator();
    let mut dispatched = false;
    ui.horizontal(|ui| {
        ui.label(format!(
            "Ready to rebase {} {}",
            plan.len(),
            if plan.len() == 1 { "commit" } else { "commits" }
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
                let root = state.selected_path();
                let plan2 = plan.clone();
                let settings = state.settings.clone();
                let branch = current_branch(state).unwrap_or_default();
                state.run_git(
                    "Interactive rebase".into(),
                    turbogit_app::root_caches::Affected::from_optional_root(root.as_deref()),
                    move |v| match &root {
                        Some(r) => {
                            history_editor::execute_with_backup(v, r, &plan2, &settings, &branch)
                        }
                        None => Ok(()),
                    },
                );
                dispatched = true;
                close(state);
            }
        });
    });
    // A dispatch or Cancel closes the editor; don't hand it the plan back.
    if !dispatched && state.ui.dialog == Some(turbogit_app::state::Dialog::InteractiveRebase) {
        state.ui.dlg.rebase_plan = Some(plan);
    }
}

/// The focused root's current branch name, for the guarded dispatch.
fn current_branch(state: &AppState) -> Option<String> {
    let id = state.selected_root.as_ref()?;
    state.multi.by_id(id)?.current_branch.clone()
}

// ------------------------------------------------------------------- rail ---

/// The right rail (screen 17): CAUTIONS computed from the plan, the
/// AFFECTED REPOS list, the SHORTCUTS legend, and RECOVERY naming the
/// backup ref that restores the pre-rebase state.
fn rail(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.dlg.rebase_cautions.is_empty() {
        widgets::group_title(
            ui,
            &format!("CAUTIONS · {}", state.ui.dlg.rebase_cautions.len()),
        );
        for caution in &state.ui.dlg.rebase_cautions {
            let text = match caution {
                history_editor::RebaseCaution::ConflictRisk { files } => format!(
                    "Conflicts likely on {files} {}",
                    if *files == 1 { "file" } else { "files" }
                ),
                history_editor::RebaseCaution::MixedIdentities { authors } => format!(
                    "Mixed committer identities ({authors} {}) — verify signatures",
                    if *authors == 1 { "author" } else { "authors" }
                ),
            };
            ui.colored_label(Palette::STATE_WARNING, format!("⚠ {text}"));
        }
        ui.add_space(6.0);
    }

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
        "P     Pick · keep the commit as is",
        "S     Squash into the commit above",
        "F     Fixup · discard the message",
        "D     Drop the commit from the plan",
    ] {
        ui.label(egui::RichText::new(line).monospace());
    }
    ui.add_space(6.0);

    widgets::group_title(ui, "RECOVERY");
    ui.label(
        "Abort any time to restore the pre-rebase state. A backup ref is          written before the first commit is replayed.",
    );
    ui.monospace(history_editor::BACKUP_REF);
    if turbogit_services::integrate_service::in_progress(
        state.selected_path().as_deref().unwrap_or(Path::new(".")),
    ) && ui.button("Abort & restore").clicked()
    {
        let root = state.selected_path();
        state.run_git(
            "Abort interactive rebase".into(),
            turbogit_app::root_caches::Affected::from_optional_root(root.as_deref()),
            move |v| match &root {
                Some(r) => history_editor::abort_to_backup(v, r),
                None => Ok(()),
            },
        );
        close(state);
    }
}

/// The Plan tab's keyboard shortcuts (screen 17): ⇧↑/⇧↓ move the selected
/// row (the selection follows), and p/s/f/d retarget its action. Skipped
/// while a text field owns the keyboard — the Log tab's REBASE-TODO must
/// take the letters literally.
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
        } else if pressed(egui::Key::S) {
            Some(RebaseAction::Squash)
        } else if pressed(egui::Key::F) {
            Some(RebaseAction::Fixup)
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

// ------------------------------------------------------------- plan rows ---

fn plan_rows(
    ui: &mut Ui,
    state: &mut AppState,
    plan: &mut Vec<turbogit_domain::model::RebasePlanEntry>,
    sel: &mut usize,
) {
    let ctx = ui.ctx().clone();
    let (any_pressed, any_released, press_pos, pointer_pos) = ctx.input(|inp| {
        (
            inp.pointer.any_pressed(),
            inp.pointer.any_released(),
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
        let fill = widgets::row_fill(selected, row.hovered());
        if fill != egui::Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 4.0, fill);
        }
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        for (lbl, act) in CHIPS {
            if child.selectable_label(plan[i].action == act, lbl).clicked() {
                history_editor::set_action(plan, i, act.clone());
            }
        }
        child.monospace(short(&plan[i].commit));
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
        if row.clicked() {
            *sel = i;
        }

        // Drag reorder, tracked from the raw pointer: a press inside the
        // row begins the drag; moving over another row while the button is
        // down reorders live (the drag follows the moved row); release ends
        // it. Tracked manually because the rows sit in an egui Window whose
        // surface claims widget-level drag attribution.
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

/// The first 7 chars of a commit SHA — the plan rows' short form.
fn short(sha: &str) -> &str {
    &sha[..7.min(sha.len())]
}

// ----------------------------------------------------------- preview tab ---

/// The RESULT PREVIEW tab (screen 17): the post-plan commit sequence with
/// the N → M counts badge, one line per fold group, and the dropped count.
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
    egui::Frame::new()
        .fill(Palette::SURFACE_2)
        .inner_margin(8.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            for e in &preview.kept {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(verb(&e.action)).color(Palette::INK_3));
                    ui.monospace(short(&e.commit));
                    ui.label(&e.subject);
                });
            }
            ui.add_space(4.0);
            for fold in &preview.folds {
                let folded: Vec<String> =
                    fold.folded.iter().map(|s| short(s).to_string()).collect();
                ui.label(
                    egui::RichText::new(format!(
                        "{} folded into {}",
                        folded.join(" + "),
                        short(&fold.into)
                    ))
                    .color(Palette::INK_3),
                );
            }
            if preview.dropped > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "{} {} dropped",
                        preview.dropped,
                        if preview.dropped == 1 {
                            "commit"
                        } else {
                            "commits"
                        }
                    ))
                    .color(Palette::STATE_WARNING),
                );
            }
        });
}

/// The lowercase verb a plan action renders as.
fn verb(action: &RebaseAction) -> &'static str {
    match action {
        RebaseAction::Pick => "pick",
        RebaseAction::Reword => "reword",
        RebaseAction::Edit => "edit",
        RebaseAction::Squash => "squash",
        RebaseAction::Fixup => "fixup",
        RebaseAction::Drop => "drop",
    }
}

// -------------------------------------------------------------- todo tab ---

/// The Log tab (screen 17): the raw generated REBASE-TODO, editable as
/// text. Every edit re-parses back into the plan; a line with an unknown
/// verb flags an error instead of silently rewriting history.
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
    // Name the editor for accessibility (the widgets::text_input pattern)
    // so tests and screen readers can address it.
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
