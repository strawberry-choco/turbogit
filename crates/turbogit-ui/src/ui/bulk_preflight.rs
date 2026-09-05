//! Bulk preflight matrix modal (issue 09, screen 02): before a fleet
//! operation runs, one row per selected repo shows its current state, what
//! will happen, and why it would be skipped, under a "N of M will run"
//! header with a per-op policy control. Nothing runs until the user
//! confirms the predicted plan.
//!
//! The cascade create-&-checkout branch operation (issue 11) shares the
//! modal: a branch-name input with the type/owner/value pattern hint, the
//! apply-broadly policy control, and a TARGET column showing where every
//! running repo will land. Existing matching branches are checked out
//! rather than recreated; Esc cancels before anything runs.
//!
//! The matrix is plain data — `turbogit_services::bulk_ops::Preflight`
//! over the live selection — computed here at render time so it can never
//! drift from the repos' actual state.

use egui::{
    Align, Align2, Color32, FontFamily, FontId, Key, Layout, Pos2, Rect, Sense, TextEdit, Ui, Vec2,
    WidgetInfo, WidgetType,
};
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::{BranchAction, BulkOp, BulkPlan, PreflightRow, SkipReason};

use crate::theme::Palette;

/// Render the modal when a bulk operation is awaiting preflight
/// confirmation (`ui.bulk_op` is `Some`).
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
        // The command input, recents row, and 390px-offset matrix columns
        // need a wider modal than the auto-sized window would pick.
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

    // Header: the "N of M will run" line (screen 02), left the skipped
    // summary, right the count.
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

    // Policy control (pull only): merge vs rebase, seeded from settings.
    if op == BulkOp::PullAll {
        ui.checkbox(
            &mut state.ui.bulk_rebase,
            "Rebase onto upstream instead of merging",
        );
    }

    // Matrix table: REPO / CURRENT BRANCH / STATUS / OUTCOME.
    let width = ui.available_width();
    let col_branch = 170.0;
    let col_status = 300.0;
    let header_h = 22.0;
    let header_rect = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, header_h));
    ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
    let hp = ui.painter().clone();
    for (label, x) in [
        ("REPO", 0.0),
        ("CURRENT BRANCH", col_branch),
        ("STATUS", col_status),
        ("OUTCOME", 390.0),
    ] {
        let galley = hp.layout_no_wrap(
            label.to_string(),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::INK_3,
        );
        hp.galley_with_override_text_color(
            Pos2::new(
                header_rect.left() + x,
                header_rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::INK_3,
        );
    }

    const ROW_H: f32 = 30.0;
    for row in &pf.rows {
        let r = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, ROW_H));
        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        let cy = r.center().y;
        let cell = |x: f32, text: String, color: Color32| {
            let galley = ui.painter().layout_no_wrap(
                text,
                FontId::new(12.0, FontFamily::Proportional),
                color,
            );
            ui.painter().galley_with_override_text_color(
                Pos2::new(r.left() + x, cy - galley.size().y / 2.0),
                galley,
                color,
            );
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
                390.0,
                outcome_text(op, state.ui.bulk_rebase, &state.ui.dlg.merge_target),
                Palette::INK_2,
            ),
            Err(reason) => cell(
                390.0,
                format!("Skipped ({}) — {}", reason.label(), skip_detail(*reason)),
                Palette::INK_3,
            ),
        }
    }

    ui.add_space(8.0);

    // Footer: cancel / confirm. The confirm button names the exact scope
    // ("Run on 9 of 12") and is disabled when nothing will run.
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
                // Cascade merge (issue 28): the source branch picked in the
                // merge dialog rides in the plan's branch field, the same
                // slot the create-&-checkout cascade uses.
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

/// The STATUS column text and color for one row.
fn row_status(row: &PreflightRow) -> (String, Color32) {
    if row.outcome == Err(SkipReason::Diverged) || (row.ahead > 0 && row.behind > 0) {
        ("Diverged".to_string(), Palette::STATE_ERROR)
    } else if row.outcome == Err(SkipReason::NoUpstream) {
        ("No upstream".to_string(), Palette::STATE_WARNING)
    } else if row.dirty > 0 {
        ("Dirty".to_string(), Palette::STATE_WARNING)
    } else {
        ("Clean".to_string(), Palette::STATE_SUCCESS)
    }
}

/// The OUTCOME column text for a will-run row. `merge_target` is the merge
/// dialog's picked source branch (cascade merge, issue 28).
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
        // The cascade renders through `branch_body` / `branch_outcome_text`.
        // The cascade renders through `branch_body` / `branch_outcome_text`.
        BulkOp::CreateBranch => String::new(),
        // The custom command renders through `command_body`.
        BulkOp::Custom => String::new(),
        // Cascade commit doesn't have a preflight modal (issue 21).
        BulkOp::Commit => String::new(),
        // Cascade merge (issue 28): the outcome names the source branch the
        // dialog picked, which rides in the dialog state until dispatch.
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

/// The trailing detail of a skip outcome ("will retry on clean" style).
fn skip_detail(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::DirtyWorktree => "commit or stash first",
        SkipReason::Diverged => "requires manual resolve",
        SkipReason::NoUpstream => "nothing to sync with",
        SkipReason::CleanTree => "nothing to stash",
    }
}

// -- Custom command (issue 13, screen 04) -------------------------------------

/// The custom-command variant of the modal: a command input with the
/// workspace's recent commands for reuse, the same matrix (custom commands
/// never preflight-skip), and — for a destructive-looking command (reset,
/// clean, history rewrites, force pushes, `-D`) — a two-stage confirmation:
/// the first Run click only arms an explicit "Yes, run <command>" button.
fn command_body(ui: &mut Ui, state: &mut AppState) {
    let pf = state.bulk_preflight(BulkOp::Custom);
    let will_run = pf.will_run();
    let total = pf.total();
    let command = state.ui.bulk_command.trim().to_string();
    let destructive = state.bulk_command_is_destructive();
    let armed = state.ui.bulk_command_armed;

    // Header: the "N of M will run" line (screen 02).
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

    // Command row: the input with its semantics hint.
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

    // Recent commands (issue 13): one chip per remembered command; clicking
    // refills the input.
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

    // Destructive warning: painted as soon as the command looks destructive;
    // the run itself still needs the second, explicit confirmation.
    if destructive {
        let reason = turbogit_services::bulk_ops::parse_command(&state.ui.bulk_command)
            .and_then(|args| turbogit_services::bulk_ops::destructive_reason(&args))
            .unwrap_or("this command can discard work");
        ui.colored_label(
            Palette::STATE_ERROR,
            format!("⚠ Destructive command — {reason}."),
        );
    }

    // Matrix table: REPO / CURRENT BRANCH / STATUS / OUTCOME.
    let width = ui.available_width();
    let col_branch = 170.0;
    let col_status = 300.0;
    let header_h = 22.0;
    let header_rect = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, header_h));
    ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
    let hp = ui.painter().clone();
    for (label, x) in [
        ("REPO", 0.0),
        ("CURRENT BRANCH", col_branch),
        ("STATUS", col_status),
        ("OUTCOME", 390.0),
    ] {
        let galley = hp.layout_no_wrap(
            label.to_string(),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::INK_3,
        );
        hp.galley_with_override_text_color(
            Pos2::new(
                header_rect.left() + x,
                header_rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::INK_3,
        );
    }

    const ROW_H: f32 = 30.0;
    for row in &pf.rows {
        let r = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, ROW_H));
        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        let cy = r.center().y;
        let cell = |x: f32, text: String, color: Color32| {
            let galley = ui.painter().layout_no_wrap(
                text,
                FontId::new(12.0, FontFamily::Proportional),
                color,
            );
            ui.painter().galley_with_override_text_color(
                Pos2::new(r.left() + x, cy - galley.size().y / 2.0),
                galley,
                color,
            );
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
            cell(390.0, "—".to_string(), Palette::INK_3);
        } else {
            cell(390.0, format!("> {command}"), Palette::INK_2);
        }
    }

    ui.add_space(8.0);

    // Footer: cancel / confirm. A destructive command's first Run click only
    // arms; the explicit confirm names the exact command it will run.
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

// -- Cascade create & checkout branch (issue 11) -----------------------------

/// The cascade variant of the modal: a branch-name input with the
/// type/owner/value pattern hint, the apply-broadly policy, and a matrix
/// with a TARGET column predicting where each repo will land.
fn branch_body(ui: &mut Ui, state: &mut AppState) {
    let bpf = state.bulk_branch_preflight();
    let will_run = bpf.will_run();
    let total = bpf.total();
    let name = state.ui.bulk_branch_name.trim().to_string();

    // Header: the "N of M will run" line (screen 02).
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

    // Branch-name row: the input with its pattern hint and semantics.
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

    // Skip summary + the apply-broadly policy control.
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

    // Matrix table: REPO / CURRENT BRANCH / TARGET / STATUS / OUTCOME.
    let width = ui.available_width();
    let col_branch = 150.0;
    let col_target = 290.0;
    let col_status = 410.0;
    let header_h = 22.0;
    let header_rect = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, header_h));
    ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
    let hp = ui.painter().clone();
    for (label, x) in [
        ("REPO", 0.0),
        ("CURRENT BRANCH", col_branch),
        ("TARGET", col_target),
        ("STATUS", col_status),
        ("OUTCOME", 520.0),
    ] {
        let galley = hp.layout_no_wrap(
            label.to_string(),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::INK_3,
        );
        hp.galley_with_override_text_color(
            Pos2::new(
                header_rect.left() + x,
                header_rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::INK_3,
        );
    }

    const ROW_H: f32 = 30.0;
    for row in &bpf.rows {
        let r = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, ROW_H));
        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        let cy = r.center().y;
        let cell = |x: f32, text: String, color: Color32| {
            let galley = ui.painter().layout_no_wrap(
                text,
                FontId::new(12.0, FontFamily::Proportional),
                color,
            );
            ui.painter().galley_with_override_text_color(
                Pos2::new(r.left() + x, cy - galley.size().y / 2.0),
                galley,
                color,
            );
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
        cell(520.0, branch_outcome_text(&row.action), Palette::INK_2);
    }

    ui.add_space(8.0);

    // Footer: cancel / confirm. The confirm button names the exact scope
    // ("Run on N of M") and stays disabled without a name or will-run rows.
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

/// The STATUS column text and color for one cascade row.
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

/// The OUTCOME column text for one cascade row.
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
