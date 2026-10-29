use egui::{
    Align, Align2, Color32, Key, Layout, Sense, TextEdit, Ui, Vec2, WidgetInfo, WidgetType,
};
use turbogit_app::bulk_ops::{BranchAction, BulkOp, BulkPlan, PreflightRow, SkipReason};
use turbogit_app::state::AppState;

use crate::theme::Palette;
use crate::ui::widgets::PaneColumn;

const HEADER_H: f32 = 22.0;

const MATRIX_COLUMNS: [PaneColumn; 4] = [
    PaneColumn::start("REPO", 0.0),
    PaneColumn::start("CURRENT BRANCH", 170.0),
    PaneColumn::start("STATUS", 300.0),
    PaneColumn::start("OUTCOME", 390.0),
];

const BRANCH_COLUMNS: [PaneColumn; 5] = [
    MATRIX_COLUMNS[0],
    PaneColumn::start("CURRENT BRANCH", 150.0),
    PaneColumn::start("TARGET", 290.0),
    PaneColumn::start("STATUS", 410.0),
    PaneColumn::start("OUTCOME", 520.0),
];

const ROW_H: f32 = 30.0;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let Some(op) = state.ui.bulk_op else {
        return;
    };
    // Esc cancels before the run (issue 11): the raw event is readable even
    // while a text field inside the modal holds focus.
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state.ui.bulk_op = None;
        return;
    }
    let ctx = ui.ctx().clone();
    let mut open = true;
    let mut win = egui::Window::new(op.label())
        .open(&mut open)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO);
    if op == BulkOp::Custom {
        win = win.min_width(560.0);
    }
    win.show(&ctx, |ui| match op {
        BulkOp::CreateBranch => branch_body(ui, state),
        BulkOp::Custom => command_body(ui, state),
        _ => body(ui, state, op),
    });
    if !open {
        state.ui.bulk_op = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState, op: BulkOp) {
    let pf = state.bulk_preflight(op);
    let will_run = pf.will_run();
    let total = pf.total();

    ui.horizontal(|ui| {
        if will_run == total {
            ui.label(format!("Will run on all {total} repositories."));
        } else {
            ui.label(format!(
                "Will run on {will_run} repositories. {} will be skipped.",
                total - will_run
            ));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(format!("{will_run} of {total} will run"));
        });
    });
    ui.separator();

    if op == BulkOp::PullAll {
        ui.checkbox(
            &mut state.ui.bulk_rebase,
            "Rebase onto upstream instead of merging",
        );
    }

    let width = ui.available_width();
    let col_branch = MATRIX_COLUMNS[1].x;
    let col_status = MATRIX_COLUMNS[2].x;
    let col_outcome = MATRIX_COLUMNS[3].x;
    super::column_table::matrix_table(
        ui,
        width,
        &MATRIX_COLUMNS,
        HEADER_H,
        ROW_H,
        &pf.rows,
        &mut |ui, r, row| {
            ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::hover());
            let cy = r.center().y;
            let cell = |x: f32, text: String, color: Color32| {
                super::column_table::paint_cell(ui.painter(), r.left(), x, cy, text, color)
            };
            cell(0.0, row.name.clone(), Palette::INK);
            cell(
                col_branch,
                row.branch.clone().unwrap_or_else(|| "<detached>".into()),
                Palette::INK_2,
            );
            let (status, status_color) = row_status(row);
            cell(col_status, status, status_color);
            match &row.outcome {
                Ok(()) => cell(
                    col_outcome,
                    outcome_text(op, state.ui.bulk_rebase, &state.ui.dlg.merge_target),
                    Palette::INK_2,
                ),
                Err(reason) => cell(
                    col_outcome,
                    format!("Skipped ({}) — {}", reason.label(), skip_detail(*reason)),
                    Palette::INK_3,
                ),
            }
        },
    );

    ui.add_space(8.0);

    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            state.ui.bulk_op = None;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let run_label = format!("Run on {will_run} of {total}");
            let run = ui.add_enabled(will_run > 0, egui::Button::new(run_label));
            run.widget_info(|| {
                WidgetInfo::labeled(
                    WidgetType::Button,
                    will_run > 0,
                    format!("Run on {will_run} of {total}"),
                )
            });
            if run.clicked() {
                let branch = if op == BulkOp::Merge {
                    state.ui.dlg.merge_target.clone()
                } else {
                    String::new()
                };
                let plan = BulkPlan {
                    op,
                    roots: pf
                        .rows
                        .iter()
                        .filter(|r| r.outcome.is_ok())
                        .map(|r| r.root.clone())
                        .collect(),
                    rebase: state.ui.bulk_rebase,
                    branch,
                    command: String::new(),
                };
                state.run_bulk_confirmed(plan);
            }
        });
    });
}

fn row_status(row: &PreflightRow) -> (String, Color32) {
    if row.outcome == Err(SkipReason::Diverged)
        || crate::theme::RepoState::is_diverged(row.ahead, row.behind)
    {
        ("Diverged".to_string(), Palette::STATE_ERROR)
    } else if row.outcome == Err(SkipReason::NoUpstream) {
        ("No upstream".to_string(), Palette::STATE_WARNING)
    } else if row.dirty > 0 {
        ("Dirty".to_string(), Palette::STATE_WARNING)
    } else {
        ("Clean".to_string(), Palette::STATE_SUCCESS)
    }
}

fn outcome_text(op: BulkOp, rebase: bool, merge_target: &str) -> String {
    match op {
        BulkOp::FetchAll => "Fetch from all remotes".to_string(),
        BulkOp::PullAll => {
            if rebase {
                "Rebase onto upstream".to_string()
            } else {
                "Merge upstream".to_string()
            }
        }
        BulkOp::PushAll => "Push to upstream".to_string(),
        BulkOp::StashAll => "Stash local changes".to_string(),
        BulkOp::CreateBranch => String::new(),
        BulkOp::Custom => String::new(),
        BulkOp::Commit => String::new(),
        BulkOp::Merge => {
            let target = merge_target.trim();
            if target.is_empty() {
                "Merge <no source branch>".to_string()
            } else {
                format!("Merge {target}")
            }
        }
    }
}

fn skip_detail(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::DirtyWorktree => "commit or stash first",
        SkipReason::Diverged => "requires manual resolve",
        SkipReason::NoUpstream => "nothing to sync with",
        SkipReason::CleanTree => "nothing to stash",
        SkipReason::ProtectedBranch => "the run may not touch it",
    }
}

fn command_body(ui: &mut Ui, state: &mut AppState) {
    let pf = state.bulk_preflight(BulkOp::Custom);
    let will_run = pf.will_run();
    let total = pf.total();
    let command = state.ui.bulk_command.trim().to_string();
    let destructive = state.bulk_command_is_destructive();
    let armed = state.ui.bulk_command_armed;

    ui.horizontal(|ui| {
        if will_run == total {
            ui.label(format!("Will run on all {total} repositories."));
        } else {
            ui.label(format!(
                "Will run on {will_run} repositories. {} will be skipped.",
                total - will_run
            ));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(format!("{will_run} of {total} will run"));
        });
    });
    ui.separator();

    ui.weak("Command");
    let edit = ui.add(
        TextEdit::singleline(&mut state.ui.bulk_command)
            .desired_width(420.0)
            .font(egui::TextStyle::Monospace),
    );
    edit.widget_info(|| {
        WidgetInfo::labeled(WidgetType::TextEdit, true, "Command input".to_string())
    });
    ui.weak("Runs verbatim on every selected repo; the leading “git” is optional.");
    ui.add_space(4.0);

    if !state.ui.recent_custom_commands.is_empty() {
        ui.horizontal(|ui| {
            ui.weak("Recent commands");
            for cmd in state.ui.recent_custom_commands.clone() {
                let btn = ui.button(cmd.clone());
                btn.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, format!("Use {cmd}"))
                });
                if btn.clicked() {
                    state.ui.bulk_command = cmd;
                    state.ui.bulk_command_armed = false;
                }
            }
        });
    }
    ui.separator();

    if destructive {
        let reason = turbogit_app::bulk_ops::parse_command(&state.ui.bulk_command)
            .and_then(|args| turbogit_app::bulk_ops::destructive_reason(&args))
            .unwrap_or("this command can discard work");
        ui.colored_label(
            Palette::STATE_ERROR,
            format!("⚠ Destructive command — {reason}."),
        );
    }

    let width = ui.available_width();
    let col_branch = MATRIX_COLUMNS[1].x;
    let col_status = MATRIX_COLUMNS[2].x;
    let col_outcome = MATRIX_COLUMNS[3].x;
    super::column_table::matrix_table(
        ui,
        width,
        &MATRIX_COLUMNS,
        HEADER_H,
        ROW_H,
        &pf.rows,
        &mut |ui, r, row| {
            ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::hover());
            let cy = r.center().y;
            let cell = |x: f32, text: String, color: Color32| {
                super::column_table::paint_cell(ui.painter(), r.left(), x, cy, text, color)
            };
            cell(0.0, row.name.clone(), Palette::INK);
            cell(
                col_branch,
                row.branch.clone().unwrap_or_else(|| "<detached>".into()),
                Palette::INK_2,
            );
            let (status, status_color) = row_status(row);
            cell(col_status, status, status_color);
            if command.is_empty() {
                cell(col_outcome, "—".to_string(), Palette::INK_3);
            } else {
                cell(col_outcome, format!("> {command}"), Palette::INK_2);
            }
        },
    );

    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.weak("Esc to cancel");
        if ui.button("Cancel").clicked() {
            state.ui.bulk_op = None;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let enabled = will_run > 0 && !command.is_empty();
            let (confirm_label, dispatching) = if armed && destructive {
                (format!("Yes, run {command}"), true)
            } else {
                (format!("Run on {will_run} of {total}"), false)
            };
            let run = ui.add_enabled(enabled, egui::Button::new(confirm_label.clone()));
            run.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, enabled, confirm_label.clone())
            });
            if run.clicked() && enabled {
                if destructive && !dispatching {
                    state.ui.bulk_command_armed = true;
                } else {
                    let plan = BulkPlan {
                        op: BulkOp::Custom,
                        roots: pf
                            .rows
                            .iter()
                            .filter(|r| r.outcome.is_ok())
                            .map(|r| r.root.clone())
                            .collect(),
                        rebase: false,
                        branch: String::new(),
                        command,
                    };
                    state.run_bulk_confirmed(plan);
                }
            }
        });
    });
}

fn branch_body(ui: &mut Ui, state: &mut AppState) {
    let bpf = state.bulk_branch_preflight();
    let will_run = bpf.will_run();
    let total = bpf.total();
    let name = state.ui.bulk_branch_name.trim().to_string();

    ui.horizontal(|ui| {
        if will_run == total {
            ui.label(format!("Will run on all {total} repositories."));
        } else {
            ui.label(format!(
                "Will run on {will_run} repositories. {} will be skipped.",
                total - will_run
            ));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(format!("{will_run} of {total} will run"));
        });
    });
    ui.separator();

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.weak("Branch name");
            let edit = ui.add(
                TextEdit::singleline(&mut state.ui.bulk_branch_name)
                    .desired_width(220.0)
                    .font(egui::TextStyle::Monospace),
            );
            edit.widget_info(|| {
                WidgetInfo::labeled(WidgetType::TextEdit, true, "Branch name input".to_string())
            });
        });
        ui.vertical(|ui| {
            ui.label(
                "Will be created from current HEAD on each repo. Existing matches are checked out.",
            );
            ui.weak("Pattern: type / owner / value");
        });
    });
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        let totals = bpf
            .skip_totals()
            .iter()
            .map(|(r, n)| format!("{n} skipped ({})", r.label()))
            .collect::<Vec<_>>()
            .join(" · ");
        if totals.is_empty() {
            ui.weak("Nothing will be skipped.");
        } else {
            ui.weak(format!("{totals} · will skip worktrees with local changes"));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.checkbox(&mut state.ui.bulk_from_upstream, "Apply broadly")
                .on_hover_text("Base the branch on the upstream tip in repos that are behind it");
        });
    });
    ui.separator();

    let width = ui.available_width();
    let col_branch = BRANCH_COLUMNS[1].x;
    let col_target = BRANCH_COLUMNS[2].x;
    let col_status = BRANCH_COLUMNS[3].x;
    let col_outcome = BRANCH_COLUMNS[4].x;
    super::column_table::matrix_table(
        ui,
        width,
        &BRANCH_COLUMNS,
        HEADER_H,
        ROW_H,
        &bpf.rows,
        &mut |ui, r, row| {
            ui.allocate_exact_size(Vec2::new(r.width(), ROW_H), Sense::hover());
            let cy = r.center().y;
            let cell = |x: f32, text: String, color: Color32| {
                super::column_table::paint_cell(ui.painter(), r.left(), x, cy, text, color)
            };
            cell(0.0, row.name.clone(), Palette::INK);
            cell(
                col_branch,
                row.branch.clone().unwrap_or_else(|| "<detached>".into()),
                Palette::INK_2,
            );
            cell(col_target, format!("> {}", row.target), Palette::INK_2);
            let (status, status_color) = branch_row_status(&row.action);
            cell(col_status, status, status_color);
            cell(
                col_outcome,
                branch_outcome_text(&row.action),
                Palette::INK_2,
            );
        },
    );

    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.weak("Esc to cancel");
        if ui.button("Cancel").clicked() {
            state.ui.bulk_op = None;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let enabled = will_run > 0 && !name.is_empty();
            let run_label = format!("Run on {will_run} of {total}");
            let run = ui.add_enabled(enabled, egui::Button::new(run_label.clone()));
            run.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, run_label.clone()));
            if run.clicked() {
                let plan = BulkPlan {
                    op: BulkOp::CreateBranch,
                    roots: bpf.plan_roots(),
                    rebase: state.ui.bulk_from_upstream,
                    branch: name,
                    command: String::new(),
                };
                state.run_bulk_confirmed(plan);
            }
        });
    });
}

fn branch_row_status(action: &BranchAction) -> (String, Color32) {
    match action {
        BranchAction::Skip(SkipReason::DirtyWorktree) => {
            ("Dirty".to_string(), Palette::STATE_WARNING)
        }
        BranchAction::Skip(_) => ("Diverged".to_string(), Palette::STATE_ERROR),
        BranchAction::CreateFromUpstream { .. } => ("Rebase".to_string(), Palette::STATE_INFO),
        BranchAction::CheckoutExisting | BranchAction::CreateFromHead => {
            ("Clean".to_string(), Palette::STATE_SUCCESS)
        }
    }
}

fn branch_outcome_text(action: &BranchAction) -> String {
    match action {
        BranchAction::CheckoutExisting => "Check out existing branch".to_string(),
        BranchAction::CreateFromHead => "Create from HEAD, checkout".to_string(),
        BranchAction::CreateFromUpstream { from } => format!("Rebase onto {from}, then create"),
        BranchAction::Skip(reason) => {
            format!("Skipped ({}) — {}", reason.label(), skip_detail(*reason))
        }
    }
}
