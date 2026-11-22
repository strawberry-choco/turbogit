//! Bulk operations: the fleet strip, the preflight matrix with its skip
//! reasons, the live monitor, the cherry-pick monitor, and the fleet
//! history with its rollback.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::bulk_history::{self, BulkRunRecord, RepoOutcome};
use turbogit_app::bulk_ops::{BulkOp, BulkPlan, PreflightRow, SkipReason};
use turbogit_app::bulk_run::RowState;

use crate::bridge::AppHandle;
use crate::screens::conflicts;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input};
use crate::ui::progress::{ProgressProps, progress};
use crate::ui::row::{RowProps, RowState as VisualRowState, row};

const MATRIX_HEIGHT: f32 = 260.0;

const OPERATIONS: [(BulkOp, &str); 6] = [
    (BulkOp::FetchAll, "Fetch all"),
    (BulkOp::PullAll, "Pull all"),
    (BulkOp::PushAll, "Push all"),
    (BulkOp::CreateBranch, "Create branch"),
    (BulkOp::StashAll, "Stash all"),
    (BulkOp::Custom, "Custom command"),
];

pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || {
            show.read(|s| {
                s.ui.bulk_op.is_some() || s.ui.bulk_run.is_some() || s.ui.cherry_run.is_some()
            })
        },
        move || {
            let paint = handle.clone();
            dynamic(move || {
                paint.read(|_| ());
                let (preflight, run, cherry) = paint.peek(|s| {
                    (
                        s.ui.bulk_op,
                        s.ui.bulk_run.is_some(),
                        s.ui.cherry_run.is_some(),
                    )
                });
                let mut out = view().into_element();
                if let Some(op) = preflight {
                    out = preflight_dialog(paint.clone(), op).into_element();
                }
                if run {
                    out = view()
                        .child(out)
                        .child(run_monitor(paint.clone()))
                        .into_element();
                }
                if cherry {
                    out = view()
                        .child(out)
                        .child(cherry_monitor(paint.clone()))
                        .into_element();
                }
                out
            })
        },
    )
}

fn open(handle: &AppHandle, op: BulkOp) {
    handle.update(move |s| s.open_bulk_preflight(op, &[]));
}

fn preflight_dialog(handle: AppHandle, op: BulkOp) -> impl IntoElement {
    let t = theme();
    let scope = handle.peek(|s| s.bulk_scope());
    let (will_run, total) = handle.peek(|s| {
        let preflight = s.bulk_preflight(op, &scope);
        (preflight.will_run(), preflight.total())
    });
    let summary = if will_run == total {
        format!("Will run on all {total} repositories.")
    } else {
        format!(
            "Will run on {will_run} repositories. {} will be skipped.",
            total - will_run
        )
    };
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::bulk("preflight"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(summary)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground)
                        .flex_1(),
                )
                .child(
                    text(format!("{will_run} of {total} will run"))
                        .text_size(px(data::TYPE_CONTROL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                ),
        );
    if op == BulkOp::PullAll {
        let rebase = handle.peek(|s| s.ui.bulk_rebase);
        let toggle = handle.clone();
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(checkbox(
                    CheckboxProps::new(move || {
                        if rebase {
                            CheckboxState::Checked
                        } else {
                            CheckboxState::Unchecked
                        }
                    })
                    .accessibility_label("Rebase onto upstream instead of merging")
                    .test_id(&test_ids::bulk("rebase-policy"))
                    .on_change(move |_| toggle.update(|s| s.ui.bulk_rebase = !s.ui.bulk_rebase)),
                ))
                .child(
                    text("Rebase onto upstream instead of merging")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        );
    }
    if op == BulkOp::CreateBranch {
        body = body.child(branch_controls(handle.clone()));
    }
    if op == BulkOp::Custom {
        body = body.child(command_controls(handle.clone()));
    }
    body = body.child(match op {
        BulkOp::CreateBranch => branch_matrix(handle.clone()).into_element(),
        BulkOp::Custom => command_matrix(handle.clone()).into_element(),
        _ => matrix(handle.clone(), op).into_element(),
    });
    let cancel = handle.clone();
    let run = handle.clone();
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(button(
                ButtonProps::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::bulk("cancel"))
                    .on_press(move || cancel.update(|s| s.ui.bulk_op = None)),
            ))
            .child(button(
                ButtonProps::new(format!("Run on {will_run} of {total}"))
                    .variant(ButtonVariant::Primary)
                    .disabled(will_run == 0)
                    .test_id(&test_ids::bulk("run"))
                    .on_press(move || confirm_run(&run, op)),
            )),
    );
    dialog(
        DialogProps::new("bulk-preflight", op.label())
            .width(760.0)
            .on_cancel({
                let cancel = handle.clone();
                move || cancel.update(|s| s.ui.bulk_op = None)
            })
            .child(body),
    )
    .into_element()
}

fn confirm_run(handle: &AppHandle, op: BulkOp) {
    let scope = handle.peek(|s| s.bulk_scope());
    let will_run = handle.peek(|s| s.bulk_preflight(op, &scope).will_run());
    if will_run == 0 {
        return;
    }
    let (roots, rebase, branch, command) = handle.peek(|s| {
        (
            s.bulk_preflight(op, &scope)
                .rows
                .iter()
                .filter(|row| row.outcome.is_ok())
                .map(|row| row.root.clone())
                .collect::<Vec<_>>(),
            s.ui.bulk_rebase,
            s.ui.bulk_branch_name.trim().to_string(),
            s.ui.bulk_command.trim().to_string(),
        )
    });
    handle.update(move |s| {
        s.run_bulk_confirmed(BulkPlan {
            op,
            roots,
            rebase,
            branch,
            command,
        });
    });
}

fn status_text(row: &PreflightRow) -> (String, AdaptiveColor) {
    if row.outcome == Err(SkipReason::Diverged) {
        ("Diverged".to_string(), data::STATE_ERROR)
    } else if row.outcome == Err(SkipReason::NoUpstream) {
        ("No upstream".to_string(), data::STATE_WARNING)
    } else if row.dirty > 0 {
        ("Dirty".to_string(), data::STATE_WARNING)
    } else {
        ("Clean".to_string(), data::STATE_SUCCESS)
    }
}

fn outcome_text(row: &PreflightRow, op: BulkOp, rebase: bool) -> String {
    match &row.outcome {
        Ok(()) => match op {
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
            BulkOp::CreateBranch | BulkOp::Custom | BulkOp::Commit | BulkOp::Merge => String::new(),
        },
        Err(reason) => format!("Skipped ({}) — {}", reason.label(), skip_detail(*reason)),
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

fn column_header() -> impl IntoElement {
    let t = theme();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .h(px(data::DENSITY_COMMAND_ROW_HEIGHT + 8.0))
        .child(
            text("REPO")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .w(px(120.0)),
        )
        .child(
            text("CURRENT BRANCH")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .w(px(130.0)),
        )
        .child(
            text("STATUS")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .w(px(100.0)),
        )
        .child(
            text("OUTCOME")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .flex_1(),
        )
}

fn matrix(handle: AppHandle, op: BulkOp) -> impl IntoElement {
    let t = theme();
    let scope = handle.peek(|s| s.bulk_scope());
    let rows = handle.peek(|s| s.bulk_preflight(op, &scope).rows);
    let rebase = handle.peek(|s| s.ui.bulk_rebase);
    let count = rows.len();
    let list = virtual_list(count, {
        let each = handle.clone();
        move |at| {
            let index = at.index();
            let rows = each.peek(|s| {
                let scope = s.bulk_scope();
                s.bulk_preflight(op, &scope).rows
            });
            let Some(found) = rows.get(index) else {
                return view().into_element();
            };
            let (status, status_color) = status_text(found);
            row(RowProps::new()
                .state(VisualRowState::Default)
                .test_id(&test_ids::bulk(format!("row:{}", found.name)))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(data::CELL_GAP))
                        .child(
                            text(found.name.clone())
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.foreground)
                                .w(px(120.0)),
                        )
                        .child(
                            text(found.branch.clone().unwrap_or_else(|| "<detached>".into()))
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.muted_foreground)
                                .w(px(130.0)),
                        )
                        .child(
                            text(status)
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(status_color)
                                .w(px(100.0)),
                        )
                        .child(
                            text(outcome_text(found, op, rebase))
                                .text_size(px(data::TYPE_CONTROL))
                                .text_color(t.foreground)
                                .flex_1(),
                        ),
                ))
            .into_element()
        }
    })
    .estimated_item_size(px(data::GROUP_ROW_HEIGHT))
    .flex_1();
    view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .h(px(MATRIX_HEIGHT))
        .test_id(&test_ids::bulk("matrix"))
        .child(column_header())
        .child(list)
}

fn branch_controls(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (name, broad) = handle.peek(|s| (s.ui.bulk_branch_name.clone(), s.ui.bulk_from_upstream));
    let change = handle.clone();
    let broad_handle = handle.clone();
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("Branch name")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )
                .child(input(
                    InputProps::new()
                        .default_value(name)
                        .placeholder("feature/branch")
                        .accessibility_label("Branch name")
                        .test_id(&test_ids::bulk("branch-name"))
                        .on_change(move |text: &str| {
                            let text = text.to_string();
                            change.update(move |s| s.ui.bulk_branch_name = text);
                        }),
                )),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(checkbox(
                    CheckboxProps::new(move || {
                        if broad {
                            CheckboxState::Checked
                        } else {
                            CheckboxState::Unchecked
                        }
                    })
                    .accessibility_label("Apply broadly")
                    .test_id(&test_ids::bulk("apply-broadly"))
                    .on_change(move |_| {
                        broad_handle.update(|s| s.ui.bulk_from_upstream = !s.ui.bulk_from_upstream)
                    }),
                ))
                .child(
                    text("Apply broadly")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        )
}

fn branch_matrix(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let scope = handle.peek(|s| s.bulk_scope());
    let rows = handle.peek(|s| s.bulk_branch_preflight(&scope).rows);
    let mut list = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::bulk("matrix"))
        .child(column_header());
    for row in rows {
        list = list.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .h(px(data::GROUP_ROW_HEIGHT))
                .test_id(&test_ids::bulk(format!("row:{}", row.name)))
                .child(
                    text(row.name.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground)
                        .w(px(120.0)),
                )
                .child(
                    text(row.branch.clone().unwrap_or_else(|| "<detached>".into()))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground)
                        .w(px(130.0)),
                )
                .child(
                    text(format!("> {}", row.target))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground)
                        .w(px(130.0)),
                )
                .child(
                    text(branch_action_text(&row.action))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground)
                        .flex_1(),
                ),
        );
    }
    list
}

fn branch_action_text(action: &turbogit_app::bulk_ops::BranchAction) -> String {
    use turbogit_app::bulk_ops::BranchAction;
    match action {
        BranchAction::CheckoutExisting => "Check out existing branch".to_string(),
        BranchAction::CreateFromHead => "Create from HEAD, checkout".to_string(),
        BranchAction::CreateFromUpstream { from } => {
            format!("Rebase onto {from}, then create")
        }
        BranchAction::Skip(reason) => {
            format!("Skipped ({}) — {}", reason.label(), skip_detail(*reason))
        }
    }
}

fn command_controls(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let command = handle.peek(|s| s.ui.bulk_command.clone());
    let destructive = handle.peek(|s| s.bulk_command_is_destructive());
    let change = handle.clone();
    let mut controls = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("Command")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )
                .child(input(
                    InputProps::new()
                        .default_value(command)
                        .placeholder("status --short")
                        .accessibility_label("Command input")
                        .test_id(&test_ids::bulk("command"))
                        .on_change(move |text: &str| {
                            let text = text.to_string();
                            change.update(move |s| {
                                s.ui.bulk_command = text;
                                s.ui.bulk_command_armed = false;
                            });
                        }),
                )),
        )
        .child(
            text("Runs verbatim on every selected repo; the leading “git” is optional.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    let recent = handle.peek(|s| s.ui.recent_custom_commands.clone());
    if !recent.is_empty() {
        let mut row_view = view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text("Recent commands")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        for command in recent {
            let press = handle.clone();
            let label = command.clone();
            row_view = row_view.child(button(
                ButtonProps::new(command.clone())
                    .size(crate::ui::ControlSize::Compact)
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::bulk(format!("recent:{command}")))
                    .on_press(move || {
                        let label = label.clone();
                        press.update(move |s| {
                            s.ui.bulk_command = label.clone();
                            s.ui.bulk_command_armed = false;
                        })
                    }),
            ));
        }
        controls = controls.child(row_view);
    }
    if destructive {
        let reason = handle
            .peek(|s| turbogit_app::bulk_ops::parse_command(&s.ui.bulk_command))
            .and_then(|args| turbogit_app::bulk_ops::destructive_reason(&args))
            .unwrap_or("this command can discard work");
        controls = controls.child(
            text(format!("⚠ Destructive command — {reason}."))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(data::STATE_ERROR),
        );
    }
    controls
}

fn command_matrix(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let scope = handle.peek(|s| s.bulk_scope());
    let rows = handle.peek(|s| s.bulk_preflight(BulkOp::Custom, &scope).rows);
    let command = handle.peek(|s| s.ui.bulk_command.clone());
    let mut list = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::bulk("matrix"))
        .child(column_header());
    for row in rows {
        let (status, status_color) = status_text(&row);
        list = list.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .h(px(data::GROUP_ROW_HEIGHT))
                .test_id(&test_ids::bulk(format!("row:{}", row.name)))
                .child(
                    text(row.name.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground)
                        .w(px(120.0)),
                )
                .child(
                    text(row.branch.clone().unwrap_or_else(|| "<detached>".into()))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground)
                        .w(px(130.0)),
                )
                .child(
                    text(status)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(status_color)
                        .w(px(100.0)),
                )
                .child(
                    text(if command.is_empty() {
                        "—".to_string()
                    } else {
                        format!("> {command}")
                    })
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground)
                    .flex_1(),
                ),
        );
    }
    list
}

fn run_monitor(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (title, tally, total, command, rows, elapsed, eta) = handle.peek(|s| {
        let Some(view) = s.ui.bulk_run.as_ref() else {
            return Default::default();
        };
        let rows = view
            .rows
            .iter()
            .map(|row| {
                (
                    row.name.clone(),
                    row.state.clone(),
                    row.duration,
                    row.root.clone(),
                )
            })
            .collect::<Vec<_>>();
        (
            format!("{} — live", view.op.label()),
            view.tally(),
            view.total(),
            view.running_command(),
            rows,
            view.started_at.elapsed().as_secs_f32(),
            view.eta.map(|d| d.as_secs_f32()),
        )
    });
    let (done, running, queued, skipped, failed) = tally;
    let settled = done + failed;
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::bulk("monitor"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(progress(
                    ProgressProps::new(if total == 0 {
                        100.0
                    } else {
                        settled as f32 / total as f32 * 100.0
                    })
                    .accessibility_label(format!("{settled} of {total} done")),
                ))
                .child(
                    text(elapsed_eta(elapsed, eta))
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.foreground),
                ),
        );
    if !command.is_empty() {
        body = body.child(
            text(command.clone())
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    }
    let count = rows.len();
    let list = virtual_list(count, {
        let each = handle.clone();
        move |at| {
            let index = at.index();
            let Some((name, state, duration, root)) = each.peek(|s| {
                s.ui.bulk_run.as_ref().and_then(|view| {
                    view.rows.get(index).map(|row| {
                        (
                            row.name.clone(),
                            row.state.clone(),
                            row.duration,
                            row.root.clone(),
                        )
                    })
                })
            }) else {
                return view().into_element();
            };
            run_row(each.clone(), name, state, duration, root).into_element()
        }
    })
    .estimated_item_size(px(data::GROUP_ROW_HEIGHT))
    .h(px(240.0));
    body = body
        .child(list)
        .child(
            text(format!(
                "{done} done · {running} running · {queued} queued · {skipped} skipped · {failed} failed"
            ))
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.foreground),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(button(
                    ButtonProps::new(format!("Retry {skipped} skipped"))
                        .variant(ButtonVariant::Secondary)
                        .disabled(skipped == 0)
                        .test_id(&test_ids::bulk("retry"))
                        .on_press({
                            let retry = handle.clone();
                            move || retry.update(|s| s.bulk_retry_skipped())
                        }),
                ))
                .child(button(
                    ButtonProps::new("Stop remaining")
                        .variant(ButtonVariant::Secondary)
                        .disabled(queued == 0)
                        .test_id(&test_ids::bulk("stop"))
                        .on_press({
                            let stop = handle.clone();
                            move || stop.update(|s| s.bulk_stop_remaining())
                        }),
                ))
                .child(button(
                    ButtonProps::new("Close")
                        .variant(ButtonVariant::Ghost)
                        .test_id(&test_ids::bulk("close-monitor"))
                        .on_press({
                            let close = handle.clone();
                            move || close.update(|s| s.ui.bulk_run = None)
                        }),
                )),
        );
    dialog(
        DialogProps::new("bulk-monitor", title)
            .width(780.0)
            .child(body),
    )
    .into_element()
}

fn elapsed_eta(elapsed: f32, eta: Option<f32>) -> String {
    format!(
        "elapsed {} · ETA {}",
        fmt_secs(elapsed),
        eta.map(|s| format!("~{}", fmt_secs(s)))
            .unwrap_or_else(|| "—".into())
    )
}

fn fmt_secs(s: f32) -> String {
    if s < 10.0 {
        format!("{s:.1}s")
    } else {
        format!("{:.0}s", s)
    }
}

fn fmt_duration(d: std::time::Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.1} s", d.as_secs_f32())
    }
}

fn run_row(
    handle: AppHandle,
    name: String,
    state: RowState,
    duration: Option<std::time::Duration>,
    root: turbogit_domain::model::RootId,
) -> impl IntoElement {
    let t = theme();
    let (state_text, color, detail) = match &state {
        RowState::Queued { slot } => (
            format!("Queued — waiting for worker slot {slot}"),
            data::STATE_WARNING,
            String::new(),
        ),
        RowState::Running => ("Running".to_string(), data::STATE_INFO, String::new()),
        RowState::Done => ("Done".to_string(), data::STATE_SUCCESS, String::new()),
        RowState::Skipped { reason } => (
            format!("Skipped ({reason})"),
            t.muted_foreground,
            String::new(),
        ),
        RowState::Failed { error, .. } => (
            "Failed".to_string(),
            data::STATE_ERROR,
            error.lines().next().unwrap_or_default().to_string(),
        ),
    };
    let failed = matches!(state, RowState::Failed { .. });
    let mut content = view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .child(
            text(name.clone())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground)
                .w(px(120.0)),
        )
        .child(
            text(state_text)
                .text_size(px(data::TYPE_CONTROL))
                .text_color(color)
                .w(px(150.0)),
        )
        .child(
            text(detail)
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .flex_1(),
        )
        .child(
            text(duration.map(fmt_duration).unwrap_or_default())
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground)
                .w(px(70.0)),
        );
    if failed {
        let resolve = handle.clone();
        content = content.child(button(
            ButtonProps::new(format!("Resolve {name}"))
                .size(crate::ui::ControlSize::Compact)
                .variant(ButtonVariant::Secondary)
                .test_id(&test_ids::bulk(format!("resolve:{name}")))
                .on_press(move || {
                    let root = root.clone();
                    let conflict = resolve.peek(|s| {
                        s.multi.by_id(&root).and_then(|repo| {
                            repo.status
                                .conflicted
                                .first()
                                .map(|path| (repo.path.clone(), path.clone()))
                        })
                    });
                    resolve.update(move |s| {
                        s.bulk_resolve(root.clone());
                        s.ui.bulk_run = None;
                    });
                    if let Some((repo, path)) = conflict {
                        conflicts::open_merge_editor(
                            &resolve,
                            &turbogit_domain::model::RootId(repo.into()),
                            &path,
                        );
                    }
                }),
        ));
    }
    row(RowProps::new()
        .state(VisualRowState::Default)
        .test_id(&test_ids::bulk(format!("run:{name}")))
        .child(content))
}

fn cherry_monitor(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (tally, total, rows, commits, policy, elapsed, eta) = handle.peek(|s| {
        let Some(view) = s.ui.cherry_run.as_ref() else {
            return Default::default();
        };
        (
            view.tally(),
            view.total(),
            view.rows
                .iter()
                .map(|row| (row.name.clone(), row.state.clone(), row.root.clone()))
                .collect::<Vec<_>>(),
            view.commit_count,
            view.stop_on_conflict,
            view.started_at.elapsed().as_secs_f32(),
            view.eta.map(|d| d.as_secs_f32()),
        )
    });
    let (done, running, queued, skipped, failed) = tally;
    let settled = done + failed;
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::bulk("cherry-monitor"))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(format!(
                        "{} · {}",
                        if commits == 1 {
                            "1 commit".to_string()
                        } else {
                            format!("{commits} commits")
                        },
                        if policy {
                            "stop on first conflict"
                        } else {
                            "continue past conflicts"
                        }
                    ))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground)
                    .flex_1(),
                )
                .child(
                    text(elapsed_eta(elapsed, eta))
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.foreground),
                ),
        )
        .child(progress(
            ProgressProps::new(if total == 0 {
                100.0
            } else {
                settled as f32 / total as f32 * 100.0
            })
            .accessibility_label(format!("{settled} of {total} done")),
        ));
    for (name, state, root) in rows {
        body = body.child(run_row(handle.clone(), name, state, None, root));
    }
    body = body.child(
        text(format!(
            "{done} done · {running} running · {queued} queued · {skipped} skipped · {failed} failed"
        ))
        .text_size(px(data::TYPE_CONTROL))
        .text_color(t.foreground),
    );
    body = body.child(button(
        ButtonProps::new("Close")
            .variant(ButtonVariant::Ghost)
            .test_id(&test_ids::bulk("cherry-close"))
            .on_press({
                let close = handle.clone();
                move || close.update(|s| s.ui.cherry_run = None)
            }),
    ));
    let _ = t;
    dialog(
        DialogProps::new("cherry-monitor", "Cherry-pick across — live")
            .width(780.0)
            .child(body),
    )
    .into_element()
}

fn op_word(op: BulkOp) -> &'static str {
    match op {
        BulkOp::CreateBranch => "Create branch",
        BulkOp::Custom => "Custom command",
        other => other.label(),
    }
}

fn history_panel(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let records = handle.peek(|s| s.ui.bulk_history.clone());
    let open = handle.peek(|s| s.ui.bulk_history_open);
    let mut body = view()
        .flex_col()
        .gap(px(data::DENSITY_LIST_GAP))
        .test_id(&test_ids::bulk("history"))
        .child(
            text("RECENT FLEET OPERATIONS")
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        );
    if records.is_empty() {
        body = body.child(
            text("No fleet operations yet.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    }
    for record in records.iter().rev() {
        let title = if record.branch.is_empty() {
            format!("{} · {} repos", op_word(record.op), record.repos.len())
        } else {
            format!(
                "{} {} · {} repos",
                op_word(record.op),
                record.branch,
                record.repos.len()
            )
        };
        body = body.child(
            view()
                .flex_col()
                .gap(px(data::DENSITY_LIST_GAP))
                .child(
                    text(title)
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                )
                .child(
                    text(record.summary())
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                ),
        );
        let details = handle.clone();
        let rollback = handle.clone();
        let id = record.id;
        let mut actions = view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(button(
                ButtonProps::new(format!("Details {}", op_word(record.op)))
                    .size(crate::ui::ControlSize::Compact)
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::bulk(format!("details:{id}")))
                    .on_press(move || {
                        details.update(move |s| {
                            s.ui.bulk_history_open = if s.ui.bulk_history_open == Some(id) {
                                None
                            } else {
                                Some(id)
                            }
                        })
                    }),
            ));
        if record.reversible() && !record.rolled_back {
            actions = actions.child(button(
                ButtonProps::new(format!("Rollback {}", op_word(record.op)))
                    .size(crate::ui::ControlSize::Compact)
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::bulk(format!("rollback:{id}")))
                    .on_press(move || {
                        rollback.update(move |s| {
                            s.bulk_rollback(id);
                            s.ui.bulk_history_open = None;
                        })
                    }),
            ));
        }
        body = body.child(actions);
        if open == Some(id) {
            for repo in &record.repos {
                let line = match &repo.outcome {
                    RepoOutcome::Done => format!("{} · done", repo.name),
                    RepoOutcome::Skipped { reason } => {
                        format!("{} · skipped ({reason})", repo.name)
                    }
                    RepoOutcome::Failed { error } => {
                        format!("{} · failed ({error})", repo.name)
                    }
                };
                body = body.child(
                    text(line)
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.foreground),
                );
            }
        }
    }
    body
}

/// The bulk route: the shared fleet strip, the history, and the layers.
pub fn bulk_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let paint = handle.clone();
    let mut strip = view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .test_id(&test_ids::bulk("strip"));
    for (op, label) in OPERATIONS {
        let press = handle.clone();
        strip = strip.child(button(
            ButtonProps::new(label)
                .variant(ButtonVariant::Secondary)
                .test_id(&test_ids::bulk(format!("open:{label}")))
                .on_press(move || open(&press, op)),
        ));
    }
    let history = handle.clone();
    strip = strip.child(button(
        ButtonProps::new("Fleet history")
            .variant(ButtonVariant::Secondary)
            .test_id(&test_ids::bulk("history-open"))
            .on_press(move || {
                history.update(|s| s.ui.fleet_history_open = !s.ui.fleet_history_open)
            }),
    ));
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("bulk"))
        .child(
            text("Bulk operations")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(strip)
        .child(dynamic(move || {
            paint.read(|_| ());
            let mut body = view().flex_col().into_element();
            if paint.peek(|s| s.ui.fleet_history_open) {
                body = view()
                    .child(body)
                    .child(history_panel(paint.clone()))
                    .into_element();
            }
            body
        }))
}

/// The record a rollback left, for the route's summary row.
pub fn rollback_line(record: &BulkRunRecord) -> String {
    format!(
        "{} · {} · {}",
        record.op.label(),
        record.summary(),
        bulk_history::format_time(record.at)
    )
}

/// The state a record names per repository, for the details row's kind.
pub fn outcome_label(outcome: &RepoOutcome) -> &'static str {
    match outcome {
        RepoOutcome::Done => "done",
        RepoOutcome::Failed { .. } => "failed",
        RepoOutcome::Skipped { .. } => "skipped",
    }
}
