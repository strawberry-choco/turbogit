//! Cascade run monitor modal (issue 10, screen 03): the live view of a
//! fleet run — one row per repo (Done / Skipped / Running with the current
//! git command / Failed / Queued with its worker slot), per-row durations,
//! an overall progress header with elapsed time and ETA, live footer
//! tallies, and the Stop remaining / Retry skipped / Resolve actions.
//!
//! The monitor is plain data — `turbogit_app::bulk_run_view::BulkRunView`
//! advanced by the event pump — rendered here; it never derives run state
//! from git itself.

use egui::{
    Align, Align2, Color32, FontFamily, FontId, Layout, Pos2, ProgressBar, Rect, Sense, Ui, Vec2,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::RowState;

use crate::theme::Palette;
use crate::ui::conflicts;

/// Render the monitor while a cascade run is live or just finished
/// (`ui.bulk_run` is `Some`).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    if state.ui.bulk_run.is_none() {
        return;
    }
    let title = format!("{} — live", state.ui.bulk_run.as_ref().unwrap().op.label());
    let ctx = ui.ctx().clone();
    // Keep the elapsed ticker and progress bar moving between events.
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
    let mut open = true;
    egui::Window::new(title)
        .open(&mut open)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(&ctx, |ui| body(ui, state));
    if !open {
        state.ui.bulk_run = None;
    }
}

fn body(ui: &mut Ui, state: &mut AppState) {
    // Snapshot everything the frame needs up front so the action handlers
    // can take `&mut AppState` freely.
    let Some(view) = state.ui.bulk_run.as_ref() else {
        return;
    };
    let (done, running, queued, skipped, failed) = view.tally();
    let total = view.total();
    let settled = done + failed;
    let workers = view.workers;
    let running_command = view.running_command();
    let elapsed_secs = view.started_at.elapsed().as_secs_f32();
    let eta_secs = view.eta.map(|d| d.as_secs_f32());
    let rows: Vec<(String, String, Color32, String, String, Option<RootId>)> = view
        .rows
        .iter()
        .map(|row| {
            let (state_text, color, detail) = match &row.state {
                RowState::Queued { slot } => (
                    format!("Queued — waiting for worker slot {slot}/{workers}"),
                    Palette::STATE_WARNING,
                    String::new(),
                ),
                RowState::Running => (
                    "Running".to_string(),
                    Palette::STATE_INFO,
                    running_command.clone(),
                ),
                RowState::Done => ("Done".to_string(), Palette::STATE_SUCCESS, String::new()),
                RowState::Skipped { reason } => {
                    (format!("Skipped ({reason})"), Palette::INK_3, String::new())
                }
                RowState::Failed { error } => (
                    "Failed".to_string(),
                    Palette::STATE_ERROR,
                    first_line(error),
                ),
            };
            (
                row.name.clone(),
                state_text,
                color,
                detail,
                row.duration.map(fmt_duration).unwrap_or_default(),
                match &row.state {
                    RowState::Failed { .. } => Some(row.root.clone()),
                    _ => None,
                },
            )
        })
        .collect();

    // Keep custom-command identity visible even when all Running events are
    // drained before the first monitor frame, or a failed row shows its error.
    if view.op == BulkOp::Custom {
        ui.label(
            egui::RichText::new(&running_command)
                .monospace()
                .color(Palette::INK_2),
        );
        ui.add_space(4.0);
    }

    // Header: overall progress bar + elapsed/ETA.
    ui.horizontal(|ui| {
        let frac = if total == 0 {
            1.0
        } else {
            settled as f32 / total as f32
        };
        ui.add(
            ProgressBar::new(frac)
                .desired_height(18.0)
                .text(format!("{settled} of {total} done")),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(format!(
                "elapsed {} · ETA {}",
                fmt_secs(elapsed_secs),
                eta_secs
                    .map(|s| format!("~{}", fmt_secs(s)))
                    .unwrap_or_else(|| "—".into())
            ));
        });
    });
    ui.separator();

    // Table: REPO / STATE / DETAIL / DURATION (+ Resolve on failed rows).
    let width = ui.available_width();
    let header_h = 22.0;
    let header_rect = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, header_h));
    ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
    let hp = ui.painter().clone();
    for (label, x) in [
        ("REPO", 0.0),
        ("STATE", 130.0),
        ("DETAIL", 400.0),
        ("DURATION", 700.0),
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

    const ROW_H: f32 = 26.0;
    for (name, state_text, color, detail, duration, resolve) in rows {
        ui.horizontal(|ui| {
            let left = ui.cursor().left();
            let cy = ui.cursor().top() + ROW_H / 2.0;
            let cells_w = if resolve.is_some() {
                width - 120.0
            } else {
                width
            };
            ui.allocate_exact_size(Vec2::new(cells_w, ROW_H), Sense::hover());
            let cell = |x: f32, text: String, color: Color32| {
                let galley = ui.painter().layout_no_wrap(
                    text,
                    FontId::new(12.0, FontFamily::Proportional),
                    color,
                );
                ui.painter().galley_with_override_text_color(
                    Pos2::new(left + x, cy - galley.size().y / 2.0),
                    galley,
                    color,
                );
            };
            cell(0.0, name.clone(), Palette::INK);
            cell(130.0, state_text, color);
            cell(400.0, detail, Palette::INK_2);
            cell(700.0, duration, Palette::INK_2);
            if let Some(root) = resolve
                && ui.button(format!("Resolve {name}")).clicked()
            {
                resolve_root(state, root);
            }
        });
    }

    ui.add_space(8.0);
    ui.separator();

    // Footer: live tallies + the run actions.
    ui.horizontal(|ui| {
        ui.label(format!(
            "{done} done · {running} running · {queued} queued · {skipped} skipped · {failed} failed"
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.ui.bulk_run = None;
            }
            let retry = ui.add_enabled(
                skipped > 0,
                egui::Button::new(format!("Retry {skipped} skipped")),
            );
            if retry.clicked() {
                state.bulk_retry_skipped();
            }
            let stop = ui.add_enabled(queued > 0, egui::Button::new("Stop remaining"));
            if stop.clicked() {
                state.bulk_stop_remaining();
            }
        });
    });
}

/// Resolve deep link (issue 10): jump to the failed row's repo, and when
/// its snapshot shows unresolved conflicts, straight into the conflict
/// resolver.
fn resolve_root(state: &mut AppState, root: RootId) {
    let conflict = state
        .multi
        .roots
        .iter()
        .find(|r| r.id == root)
        .and_then(|r| {
            r.status
                .conflicted
                .first()
                .map(|p| (r.path.clone(), p.clone()))
        });
    state.bulk_resolve(root);
    if let Some((root_path, rel)) = conflict {
        conflicts::open_conflict_editor(state, &root_path, &rel);
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn fmt_duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", d.as_secs_f32())
    }
}

fn fmt_secs(s: f32) -> String {
    if s < 10.0 {
        format!("{s:.1}s")
    } else {
        format!("{:.0}s", s)
    }
}

// ------------------------------------------------ cherry-pick across ----

/// Cherry-pick-across run monitor (issue 16): the same row model and pool
/// events as the bulk run above, one row per target repo with its commits
/// applied in order. No Retry pass — a conflicted repo is held for manual
/// resolution and re-run through the dialog. Failed rows deep-link into the
/// held repo's conflict, never auto-resolving anything.
pub fn show_cherry(ui: &mut Ui, state: &mut AppState) {
    if state.ui.cherry_run.is_none() {
        return;
    }
    let ctx = ui.ctx().clone();
    // Keep the elapsed ticker and progress bar moving between events.
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
    let mut open = true;
    egui::Window::new("Cherry-pick across — live")
        .open(&mut open)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(&ctx, |ui| cherry_body(ui, state));
    if !open {
        state.ui.cherry_run = None;
    }
}

fn cherry_body(ui: &mut Ui, state: &mut AppState) {
    let Some(view) = state.ui.cherry_run.as_ref() else {
        return;
    };
    let (done, running, queued, skipped, failed) = view.tally();
    let total = view.total();
    let settled = done + failed;
    let workers = view.workers;
    let running_command = view.running_command();
    let commits = view.commit_count;
    let policy = if view.stop_on_conflict {
        "stop on first conflict"
    } else {
        "continue past conflicts"
    };
    let elapsed_secs = view.started_at.elapsed().as_secs_f32();
    let eta_secs = view.eta.map(|d| d.as_secs_f32());
    let rows: Vec<(String, String, Color32, String, String, Option<RootId>)> = view
        .rows
        .iter()
        .map(|row| {
            let (state_text, color, detail) = match &row.state {
                RowState::Queued { slot } => (
                    format!("Queued — waiting for worker slot {slot}/{workers}"),
                    Palette::STATE_WARNING,
                    String::new(),
                ),
                RowState::Running => (
                    "Running".to_string(),
                    Palette::STATE_INFO,
                    running_command.clone(),
                ),
                RowState::Done => ("Done".to_string(), Palette::STATE_SUCCESS, String::new()),
                RowState::Skipped { reason } => {
                    (format!("Skipped ({reason})"), Palette::INK_3, String::new())
                }
                RowState::Failed { error } => (
                    "Failed".to_string(),
                    Palette::STATE_ERROR,
                    first_line(error),
                ),
            };
            (
                row.name.clone(),
                state_text,
                color,
                detail,
                row.duration.map(fmt_duration).unwrap_or_default(),
                match &row.state {
                    RowState::Failed { .. } => Some(row.root.clone()),
                    _ => None,
                },
            )
        })
        .collect();

    // Header: what is running where, the overall progress, elapsed/ETA.
    ui.horizontal(|ui| {
        ui.strong(format!(
            "{commits} commit{} · {policy}",
            if commits == 1 { "" } else { "s" }
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(format!(
                "elapsed {} · ETA {}",
                fmt_secs(elapsed_secs),
                eta_secs
                    .map(|s| format!("~{}", fmt_secs(s)))
                    .unwrap_or_else(|| "—".into())
            ));
        });
    });
    ui.horizontal(|ui| {
        let frac = if total == 0 {
            1.0
        } else {
            settled as f32 / total as f32
        };
        ui.add(
            ProgressBar::new(frac)
                .desired_height(18.0)
                .text(format!("{settled} of {total} done")),
        );
    });
    ui.separator();

    // Table: REPO / STATE / DETAIL / DURATION (+ Resolve on failed rows).
    let width = ui.available_width();
    let header_h = 22.0;
    let header_rect = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, header_h));
    ui.allocate_exact_size(Vec2::new(width, header_h), Sense::hover());
    let hp = ui.painter().clone();
    for (label, x) in [
        ("REPO", 0.0),
        ("STATE", 130.0),
        ("DETAIL", 400.0),
        ("DURATION", 700.0),
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

    const ROW_H: f32 = 26.0;
    for (name, state_text, color, detail, duration, resolve) in rows {
        ui.horizontal(|ui| {
            let left = ui.cursor().left();
            let cy = ui.cursor().top() + ROW_H / 2.0;
            let cells_w = if resolve.is_some() {
                width - 120.0
            } else {
                width
            };
            ui.allocate_exact_size(Vec2::new(cells_w, ROW_H), Sense::hover());
            let cell = |x: f32, text: String, color: Color32| {
                let galley = ui.painter().layout_no_wrap(
                    text,
                    FontId::new(12.0, FontFamily::Proportional),
                    color,
                );
                ui.painter().galley_with_override_text_color(
                    Pos2::new(left + x, cy - galley.size().y / 2.0),
                    galley,
                    color,
                );
            };
            cell(0.0, name.clone(), Palette::INK);
            cell(130.0, state_text, color);
            cell(400.0, detail, Palette::INK_2);
            cell(700.0, duration, Palette::INK_2);
            if let Some(root) = resolve
                && ui.button(format!("Resolve {name}")).clicked()
            {
                resolve_cherry_root(state, root);
            }
        });
    }

    ui.add_space(8.0);
    ui.separator();

    // Footer: live tallies + the run actions (Stop remaining / Close).
    ui.horizontal(|ui| {
        ui.label(format!(
            "{done} done · {running} running · {queued} queued · {skipped} skipped · {failed} failed"
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.ui.cherry_run = None;
            }
            let stop = ui.add_enabled(queued > 0, egui::Button::new("Stop remaining"));
            if stop.clicked()
                && let Some(view) = state.ui.cherry_run.as_ref()
            {
                view.control.stop();
            }
        });
    });
}

/// Resolve deep link for the cherry run monitor (issue 16): jump to the
/// held repo, and when its snapshot shows unresolved conflicts, straight
/// into the conflict resolver. Nothing is auto-resolved here.
fn resolve_cherry_root(state: &mut AppState, root: RootId) {
    let conflict = state
        .multi
        .roots
        .iter()
        .find(|r| r.id == root)
        .and_then(|r| {
            r.status
                .conflicted
                .first()
                .map(|p| (r.path.clone(), p.clone()))
        });
    state.cherry_resolve(root);
    if let Some((root_path, rel)) = conflict {
        conflicts::open_conflict_editor(state, &root_path, &rel);
    }
}
