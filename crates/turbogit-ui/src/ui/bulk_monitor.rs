//! Cascade run monitor modal (issue 10, screen 03): the live view of a
//! fleet run — one row per repo (Done / Skipped / Running with the current
//! git command / Failed / Queued with its worker slot), per-row durations,
//! an overall progress header with elapsed time and ETA, live footer
//! tallies, and the Stop remaining / Retry skipped / Resolve actions.
//!
//! The monitor is plain data — `turbogit_app::bulk_run_view::BulkRunView`
//! advanced by the event pump — rendered here; it never derives run state
//! from git itself.

use egui::{Align, Align2, Color32, Layout, Pos2, ProgressBar, Rect, Sense, Ui, Vec2};
use turbogit_app::state::AppState;
use turbogit_domain::model::{RootId, subject_of_message};
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::RowState;

use crate::theme::Palette;
use crate::ui::conflicts;

/// One row of the running-operation table, where both copies of this literal
/// said 26.0 (conformance issue 13). Local rather than
/// [`crate::theme::GROUP_ROW_HEIGHT`], which happens to be 26 too but names the
/// changes tree's group band, not a monitor row.
const ROW_H: f32 = 26.0;

#[derive(Clone, Copy)]
struct Tally {
    done: usize,
    running: usize,
    queued: usize,
    skipped: usize,
    failed: usize,
}

impl Tally {
    fn of((done, running, queued, skipped, failed): (usize, usize, usize, usize, usize)) -> Self {
        Self {
            done,
            running,
            queued,
            skipped,
            failed,
        }
    }

    /// The progress bar's numerator. **Done plus failed**: a failed repository
    /// is settled, so an all-failing run is complete.
    fn settled(self) -> usize {
        self.done + self.failed
    }
}

/// One rendered run row; `resolve` is set for failed rows only.
struct RunRow {
    name: String,
    state_text: String,
    color: Color32,
    detail: String,
    duration: String,
    resolve: Option<RootId>,
}

/// The run view's rows as a snapshot, so the frame can hand `&mut AppState` to
/// the footer's verbs.
fn run_rows(
    rows: &[turbogit_services::bulk_run::RunRow],
    workers: usize,
    running_command: &str,
) -> Vec<RunRow> {
    rows.iter()
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
                    running_command.to_string(),
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
            RunRow {
                name: row.name.clone(),
                state_text,
                color,
                detail,
                duration: row.duration.map(fmt_duration).unwrap_or_default(),
                resolve: match &row.state {
                    RowState::Failed { .. } => Some(row.root.clone()),
                    _ => None,
                },
            }
        })
        .collect()
}

/// The progress bar both monitors' headers paint. An empty run is **complete**,
/// not stalled, so the fraction is 1 rather than `0 / 0`.
fn progress(settled: usize, total: usize) -> ProgressBar {
    let frac = if total == 0 {
        1.0
    } else {
        settled as f32 / total as f32
    };
    ProgressBar::new(frac)
        .desired_height(18.0)
        .text(format!("{settled} of {total} done"))
}

/// The header's right-aligned elapsed / ETA label. The em dash is the **unknown
/// ETA**, not zero.
fn elapsed_eta(elapsed_secs: f32, eta_secs: Option<f32>) -> String {
    format!(
        "elapsed {} · ETA {}",
        fmt_secs(elapsed_secs),
        eta_secs
            .map(|s| format!("~{}", fmt_secs(s)))
            .unwrap_or_else(|| "—".into())
    )
}

/// The run table: the `REPO / STATE / DETAIL / DURATION` header row, then one
/// row per repository, `Resolve <name>` on the failed ones.
fn run_table(
    ui: &mut Ui,
    rows: Vec<RunRow>,
    state: &mut AppState,
    on_resolve: &mut dyn FnMut(&mut AppState, RootId),
) {
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
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
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

    for row in rows {
        let RunRow {
            name,
            state_text,
            color,
            detail,
            duration,
            resolve,
        } = row;
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
                    crate::theme::chrome_font(crate::theme::TYPE_BODY),
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
                on_resolve(state, root);
            }
        });
    }
}

/// The live tally sentence both monitors' footers print.
fn run_tallies(ui: &mut Ui, tally: Tally) {
    ui.label(format!(
        "{} done · {} running · {} queued · {} skipped · {} failed",
        tally.done, tally.running, tally.queued, tally.skipped, tally.failed
    ));
}

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
    let Some(view) = state.ui.bulk_run.as_ref() else {
        return;
    };
    let tally = Tally::of(view.tally());
    let total = view.total();
    let running_command = view.running_command();
    let rows = run_rows(&view.rows, view.workers, &running_command);
    let elapsed_secs = view.started_at.elapsed().as_secs_f32();
    let eta_secs = view.eta.map(|d| d.as_secs_f32());

    // Keep the custom-command label visible even when every Running event is
    // drained before the first frame.
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
        ui.add(progress(tally.settled(), total));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(elapsed_eta(elapsed_secs, eta_secs));
        });
    });
    ui.separator();

    // Table: REPO / STATE / DETAIL / DURATION (+ Resolve on failed rows).
    run_table(ui, rows, state, &mut resolve_root);

    ui.add_space(8.0);
    ui.separator();

    // Footer: live tallies + the run actions.
    ui.horizontal(|ui| {
        run_tallies(ui, tally);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.ui.bulk_run = None;
            }
            // **The one action the cherry footer does not have.** A skipped
            // repository is retryable here because a cascade is idempotent.
            let retry = ui.add_enabled(
                tally.skipped > 0,
                egui::Button::new(format!("Retry {} skipped", tally.skipped)),
            );
            if retry.clicked() {
                state.bulk_retry_skipped();
            }
            let stop = ui.add_enabled(tally.queued > 0, egui::Button::new("Stop remaining"));
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

/// The row's one-line failure text: the first line of the error, never a whole
/// git transcript.
fn first_line(s: &str) -> String {
    subject_of_message(s).to_string()
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
    let tally = Tally::of(view.tally());
    let total = view.total();
    let running_command = view.running_command();
    let rows = run_rows(&view.rows, view.workers, &running_command);
    let elapsed_secs = view.started_at.elapsed().as_secs_f32();
    let eta_secs = view.eta.map(|d| d.as_secs_f32());
    let commits = view.commit_count;
    let policy = if view.stop_on_conflict {
        "stop on first conflict"
    } else {
        "continue past conflicts"
    };

    // Header: the `{commits} · {policy}` line is the cherry run's own identity.
    ui.horizontal(|ui| {
        ui.strong(format!(
            "{commits} commit{} · {policy}",
            if commits == 1 { "" } else { "s" }
        ));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.strong(elapsed_eta(elapsed_secs, eta_secs));
        });
    });
    ui.horizontal(|ui| {
        ui.add(progress(tally.settled(), total));
    });
    ui.separator();

    // Table: REPO / STATE / DETAIL / DURATION (+ Resolve on failed rows).
    run_table(ui, rows, state, &mut resolve_cherry_root);

    ui.add_space(8.0);
    ui.separator();

    // Footer: live tallies + the run actions (Stop remaining / Close).
    ui.horizontal(|ui| {
        run_tallies(ui, tally);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.ui.cherry_run = None;
            }
            let stop = ui.add_enabled(tally.queued > 0, egui::Button::new("Stop remaining"));
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
