use crate::theme::Palette;
use crate::ui::widgets::{self, BadgeKind};
use crate::ui::{components, icons};
use chrono::{TimeZone, Utc};
use egui::{Align, Color32, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2};
use std::collections::HashSet;
use std::path::PathBuf;
use turbogit_app::PushTags;
use turbogit_app::operation::PushRequest;
use turbogit_app::push_session::{self, Focus, ListedCommit, PushRow};
use turbogit_app::state::{AppState, PushPreview};
use turbogit_app::sync_service;
use turbogit_domain::model::{ChangeStatus, CommitId, RootId};

const PANE_BODY_H: f32 = 197.0;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    let ctx = ui.ctx().clone();
    let mut open = true;
    // The window's height is capped to the screen. A body that grows past the
    // screen's bottom edge makes egui MOVE the window up, and a window that
    // moves reflows every row under it, so the cap keeps the dialog's own
    // rows where the user last saw them, and the report well scrolls inside
    // the dialog instead of pushing the footer off the screen.
    let max_height = ctx.input(|i| i.content_rect().height()) - 40.0;
    let generation = state.ui.dlg.push.generation;
    egui::Window::new("Push Commits")
        .open(&mut open)
        .default_width(720.0)
        .resizable(false)
        .default_height(32.0)
        .max_height(max_height)
        .show(&ctx, |ui| {
            state.ui.dlg.push.scope = Some(state.ui.dlg.repo_scope.clone());
            super::dialogs::repo_scope_band(ui, state);
            panes(ui, state, generation);
            consent_line(ui, state);
            options_band(ui, state);
            if let Some(preview) = state.ui.dlg.push.preview.clone() {
                dry_run_well(ui, &preview);
            }
            footer(ui, state);
        });
    if !open {
        close(state);
    }
}

fn close(state: &mut AppState) {
    state.ui.dialog = None;
}

fn panes(ui: &mut Ui, state: &mut AppState, generation: u64) {
    let row = ui.available_rect_before_wrap();
    let gap = ui.spacing().item_spacing.x;
    let usable = row.width() - gap;
    let left_rect = Rect::from_min_max(
        row.min,
        Pos2::new(row.min.x + usable * 0.55, row.min.y + PANE_BODY_H),
    );
    let right_rect = Rect::from_min_max(
        Pos2::new(left_rect.right() + gap, row.min.y),
        Pos2::new(row.right(), row.min.y + PANE_BODY_H),
    );
    // The row's height belongs to the parent, or the bands below would paint
    // over the panes.
    ui.allocate_space(Vec2::new(row.width(), PANE_BODY_H));

    let mut left = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    row_table(&mut left, state, generation);
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    changed_files_pane(&mut right, state, generation);

    let x = (left_rect.right() + right_rect.left()) / 2.0;
    widgets::edge_rule(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x, row.min.y),
            Pos2::new(x, row.min.y + PANE_BODY_H),
        ),
        widgets::Edge::Right,
        Palette::RULE_STRUCTURAL,
    );
}

fn row_table(ui: &mut Ui, state: &mut AppState, generation: u64) {
    let rows = state.ui.dlg.push.rows.clone();
    let focus = state.ui.dlg.push.focus.clone();
    widgets::group_title(ui, "REPOSITORIES");
    let body_h = ui.available_height();
    let mut consented: Option<usize> = None;
    let mut expanded: Option<usize> = None;
    let mut next_focus: Option<Focus> = None;
    ScrollArea::vertical()
        .id_salt(("push_rows", generation))
        .max_height(body_h)
        .show(ui, |ui| {
            if rows.is_empty() {
                widgets::empty_state(ui, "No repositories registered.");
                return;
            }
            // The all-roots focus target: a subject for the right pane, never a
            // row that consents or a node the payload reads.
            let over_all = focus.as_ref().is_none_or(|f| *f == Focus::AllRoots);
            if ui.selectable_label(over_all, "All repositories").clicked() {
                next_focus = Some(Focus::AllRoots);
            }
            for (index, row) in rows.iter().enumerate() {
                let decision = repo_row(ui, state, row, index, generation, focus.as_ref());
                if decision.consent {
                    consented = Some(index);
                }
                if decision.expand {
                    expanded = Some(index);
                }
                if let Some(focus) = decision.focus {
                    next_focus = Some(focus);
                }
            }
        });

    let session = &mut state.ui.dlg.push;
    if let Some(index) = consented {
        session.rows[index].toggle_consent();
    }
    if let Some(index) = expanded {
        session.rows[index].expanded = !session.rows[index].expanded;
    }
    if let Some(focus) = next_focus {
        session.focus = Some(focus);
    }
}

#[derive(Default)]
struct RowDecision {
    consent: bool,
    expand: bool,
    focus: Option<Focus>,
}

fn repo_row(
    ui: &mut Ui,
    state: &AppState,
    row: &PushRow,
    index: usize,
    generation: u64,
    focus: Option<&Focus>,
) -> RowDecision {
    let mut decision = RowDecision::default();
    let width = ui.available_width();
    let (band, host) =
        ui.allocate_exact_size(Vec2::new(width, components::BRANCH_ROW_H), Sense::hover());
    let focused = matches!(focus, Some(Focus::Root(id)) if *id == row.id);
    // `contains_pointer` rather than `hovered`: the row's own labels are
    // registered after this rect and win the hover tie, which would otherwise
    // make the band's highlight flicker as the pointer crosses the text.
    components::row_shell(
        ui,
        band,
        if focused {
            components::RowState::FocusSelected
        } else {
            components::RowState::from_flags(false, host.contains_pointer())
        },
        components::RowShell::Railed,
    );

    let mut line = ui.new_child(
        UiBuilder::new()
            .max_rect(band)
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The repository name is the checkbox's own text rather than a separate
    // label, so the consent control reaches the accessibility tree and a
    // `get_by_role_and_label(CheckBox, name)` query. An empty-labelled checkbox
    // is unaddressable by both.
    let name_text = {
        let ink = if row.consented {
            Palette::INK
        } else {
            Palette::INK_3
        };
        let text = RichText::new(row.name.clone()).color(ink);
        if row.consented { text.strong() } else { text }
    };
    let mut consented = row.consented;
    if line
        .checkbox(&mut consented, name_text)
        .on_hover_text("Push this repository")
        .changed()
    {
        decision.consent = true;
    }
    let chevron = if row.expanded {
        icons::Icon::CHEVRON_DOWN
    } else {
        icons::Icon::CHEVRON_RIGHT
    };
    let (chevron_rect, _) =
        line.allocate_exact_size(Vec2::splat(components::CLICK_TARGET_MIN), Sense::hover());
    let expand_id = line.id().with(("push_expand", generation, index));
    let expand_label = format!(
        "{} {}",
        if row.expanded { "Collapse" } else { "Expand" },
        row.name
    );
    let disclosure = widgets::ghost_icon_button(
        &mut line,
        chevron_rect,
        expand_id,
        &expand_label,
        !row.commits.is_empty(),
        |ui, rect, _state| {
            icons::centered_icon(
                ui,
                chevron,
                rect.center(),
                components::KIT_ICON,
                Palette::INK_3,
            );
        },
    );
    if disclosure.clicked() {
        decision.expand = true;
    }
    // The band's click target begins after the row's own controls and is
    // registered *before* the labels, so the labels win the hit-test tie over
    // the text they sit on while the band still covers the empty space beyond
    // them. egui gives the click to the later of two widgets at one layer.
    let focus_from = line.cursor().min.x.clamp(band.left(), band.right());
    let focus_rect = Rect::from_x_y_ranges(focus_from..=band.right(), band.y_range());
    if line
        .interact(
            focus_rect,
            egui::Id::new(("push_focus", generation, index)),
            Sense::click(),
        )
        .clicked()
    {
        decision.focus = Some(Focus::Root(row.id.clone()));
    }
    icons::icon(
        &mut line,
        icons::Icon::FOLDER_GIT,
        components::KIT_ICON_LARGE,
        Palette::INK_2,
    );
    // The target line is the row's own focus affordance: it carries the
    // accessible label a `query_by_text` needs, which a bare `interact` rect
    // does not register.
    let target_text = RichText::new(push_session::target_line(
        row.local.as_deref(),
        row.remote.value().unwrap_or_default(),
        row.branch.value().unwrap_or_default(),
    ))
    .small()
    .color(Palette::INK_2);
    if line
        .add(egui::Label::new(target_text).sense(Sense::click()))
        .clicked()
    {
        decision.focus = Some(Focus::Root(row.id.clone()));
    }
    if let Some(root) = state.multi.by_id(&row.id)
        && let Some(target) = sync_service::push_target_state(
            root,
            row.remote.value().unwrap_or_default(),
            row.branch.value().unwrap_or_default(),
            row.set_upstream,
        )
    {
        widgets::badge(&mut line, target.label(), BadgeKind::Neutral);
    }
    widgets::count_chip(&mut line, &row.listed_total.to_string());
    if let Some(obstacle) = row.obstacle {
        line.label(
            RichText::new(obstacle.label())
                .small()
                .color(Palette::STATE_WARNING),
        );
    }
    if let Some(error) = row.error.as_deref() {
        line.label(RichText::new(error).small().color(Palette::STATE_ERROR));
    }

    if row.expanded {
        ui.indent(("push_row_commits", generation, index), |ui| {
            for commit in &row.commits {
                let shown = commit_line(
                    ui,
                    commit,
                    matches!(focus, Some(Focus::Commit(_, sha)) if sha == &commit.id),
                );
                if shown {
                    decision.focus = Some(Focus::Commit(row.id.clone(), commit.id.clone()));
                }
            }
        });
    }
    decision
}

fn commit_line(ui: &mut Ui, commit: &ListedCommit, focused: bool) -> bool {
    let detail = format!(
        "{}\n{} · {}\n{}",
        commit.subject,
        commit.author,
        fmt_date(commit.time),
        commit.id
    );
    let short = widgets::short_commit_ref(&commit.id);
    let body = {
        let text = RichText::new(format!(
            "{}{}",
            commit.subject,
            if commit.author_differs { "*" } else { "" }
        ))
        .color(if focused {
            Palette::INK
        } else {
            Palette::INK_2
        });
        if focused { text.strong() } else { text }
    };
    ui.horizontal(|ui| {
        widgets::hash_chip(ui, &short, &detail);
        let clicked = ui
            .add(egui::Label::new(body).sense(Sense::click()))
            .on_hover_text(&detail)
            .clicked();
        ui.label(
            RichText::new(format!("{} · {}", commit.author, rel_time(commit.time)))
                .small()
                .weak(),
        );
        clicked
    })
    .inner
}

fn changed_files_pane(ui: &mut Ui, state: &mut AppState, generation: u64) {
    let rows = state.ui.dlg.push.rows.clone();
    let focus = state.ui.dlg.push.focus.clone();
    let wanted = wanted_commits(&rows, focus.as_ref());
    ui.horizontal(|ui| {
        widgets::group_title(ui, "CHANGED FILES");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(focus_name(&rows, focus.as_ref()))
                    .small()
                    .weak(),
            );
        });
    });
    let body_h = ui.available_height();
    ScrollArea::vertical()
        .id_salt(("push_changed_files", generation))
        .max_height(body_h)
        .show(ui, |ui| {
            let mut seen: HashSet<PathBuf> = HashSet::new();
            let mut shown = 0;
            for (root, commit) in &wanted {
                for change in state.ensure_files(root, commit) {
                    if !seen.insert(change.path.clone()) {
                        continue;
                    }
                    shown += 1;
                    ui.horizontal(|ui| {
                        components::state_mark(
                            ui,
                            change.status.short(),
                            status_ink(change.status),
                        );
                        ui.label(change.path.display().to_string());
                    });
                }
            }
            if shown == 0 {
                widgets::empty_state(ui, "No changed files.");
            }
        });
}

fn wanted_commits(rows: &[PushRow], focus: Option<&Focus>) -> Vec<(RootId, CommitId)> {
    let mut wanted = Vec::new();
    for row in rows {
        let mine = match focus {
            None | Some(Focus::AllRoots) => true,
            Some(Focus::Root(id)) => *id == row.id,
            Some(Focus::Commit(id, _)) => *id == row.id,
        };
        if !mine {
            continue;
        }
        for commit in &row.commits {
            if let Some(Focus::Commit(_, sha)) = focus
                && sha != &commit.id
            {
                continue;
            }
            wanted.push((row.id.clone(), commit.id.clone()));
        }
    }
    wanted
}

fn focus_name(rows: &[PushRow], focus: Option<&Focus>) -> String {
    match focus {
        None | Some(Focus::AllRoots) => "All roots".to_string(),
        Some(Focus::Root(id)) => rows
            .iter()
            .find(|row| &row.id == id)
            .map(|row| row.name.clone())
            .unwrap_or_else(|| id.name()),
        Some(Focus::Commit(_, sha)) => widgets::short_commit_ref(sha),
    }
}

fn status_ink(status: ChangeStatus) -> Color32 {
    match status {
        ChangeStatus::Added => Palette::STATUS_ADDED,
        ChangeStatus::Deleted => Palette::STATUS_DELETED,
        ChangeStatus::Unversioned => Palette::STATUS_UNVERSIONED,
        _ => Palette::STATUS_MODIFIED,
    }
}

fn consent_line(ui: &mut Ui, state: &AppState) {
    let session = &state.ui.dlg.push;
    let repositories = session.rows.iter().filter(|row| row.consented).count();
    let commits: usize = session
        .payload()
        .targets
        .iter()
        .filter_map(|target| session.row(&target.root))
        .map(|row| row.listed_total)
        .sum();
    ui.label(
        RichText::new(format!(
            "{} · {}",
            components::counted(repositories, "repository", "repositories"),
            components::counted(commits, "commit", "commits")
        ))
        .small()
        .weak(),
    );
}

/// The batch's three git flags. `Force push` stays a checkbox rather than the
/// reference's item on a `Push ▾` dropdown, which is the follow-up this surface
/// owes; protection is refused per repository at the engine and never disables
/// plain `Push`.
fn options_band(ui: &mut Ui, state: &mut AppState) {
    widgets::group_title(ui, "OPTIONS");
    let mut run_hooks = !state.ui.dlg.push.no_verify;
    if ui
        .checkbox(&mut run_hooks, "Run Git hooks")
        .on_hover_text("Unchecking passes --no-verify to git push.")
        .changed()
    {
        state.ui.dlg.push.no_verify = !run_hooks;
    }

    const TAG_ORDER: [PushTags; 3] = [PushTags::None, PushTags::All, PushTags::Follow];
    let labels: Vec<&str> = TAG_ORDER.iter().map(|mode| mode.label()).collect();
    let chosen = TAG_ORDER
        .iter()
        .position(|mode| *mode == state.ui.dlg.push.tags)
        .unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label("Push tags:");
        if let Some(picked) = widgets::segmented_control(ui, &labels, chosen) {
            state.ui.dlg.push.tags = TAG_ORDER[picked.min(TAG_ORDER.len() - 1)];
        }
    });

    let mut force = state.ui.dlg.push.force;
    if ui
        .checkbox(&mut force, "Force push (--force-with-lease)")
        .changed()
    {
        state.ui.dlg.push.force = force;
    }
    if force {
        let protected: Vec<String> = state
            .ui
            .dlg
            .push
            .rows
            .iter()
            .filter(|row| row.consented)
            .filter_map(|row| row.branch.value().map(str::to_string))
            .filter(|branch| sync_service::is_protected(&state.settings, branch))
            .collect();
        if protected.is_empty() {
            ui.colored_label(
                Palette::STATE_WARNING,
                "Force push rewrites the remote branch (--force-with-lease).",
            );
        } else {
            widgets::inline_error(
                ui,
                format!(
                    "{} protected — the push refuses force for that repository.",
                    protected.join(", ")
                ),
            );
        }
    }
}

fn footer(ui: &mut Ui, state: &mut AppState) {
    let session = &state.ui.dlg.push;
    // Consent is necessary but not sufficient: a consented row whose obstacle
    // blocks the push never enters the payload, so an all-blocked dialog would
    // otherwise dispatch an empty batch and report it a success.
    let nothing_to_push = session.payload().is_empty();
    let stale = !session
        .stale_rows(&|id| state.caches.ahead_behind(id).map(|(ahead, _)| ahead))
        .is_empty();
    // `Nothing to update` answers the empty-scope defect — an unticked dialog
    // used to push nothing and report success — and is not protected-branch
    // handling, which never disables this button.
    let dimmed = nothing_to_push || stale;
    let label = if dimmed { "Nothing to update" } else { "Push" };
    widgets::dialog_footer(ui, |ui| {
        let push = ui
            .scope(|ui| {
                if dimmed {
                    ui.disable();
                }
                widgets::compact_primary_button(ui, label)
            })
            .inner;
        if push.clicked() {
            execute_push(state);
            close(state);
        }
        if widgets::compact_button(ui, "Preview dry-run").clicked() {
            run_preview(state);
        }
        if widgets::compact_button(ui, "Cancel").clicked() {
            close(state);
        }
    });
}

fn dry_run_well(ui: &mut Ui, preview: &PushPreview) {
    widgets::note(ui, None, |ui| {
        ui.label(
            RichText::new(format!(
                "{} commits → {} remotes · {} refs · {} rejected",
                preview.commits, preview.remotes, preview.refs, preview.rejected
            ))
            .small()
            .strong(),
        );
        preview_reports(ui, preview);
    });
}

fn preview_reports(ui: &mut Ui, preview: &PushPreview) {
    let has_ok = preview.reports.iter().any(|(_, report)| report.is_ok());
    let has_err = preview.reports.iter().any(|(_, report)| report.is_err());
    if has_ok {
        ui.label("Dry-run report (verbatim):");
    }
    if has_err {
        ui.colored_label(Palette::STATE_ERROR, "Push rejected by git:");
    }
    ScrollArea::vertical()
        .id_salt("push_preview_reports")
        .max_height(160.0)
        .show(ui, |ui| {
            for (name, report) in &preview.reports {
                match report {
                    Ok(text) => {
                        ui.label(RichText::new(format!("[{name}]")).small().strong());
                        ui.label(RichText::new(text).monospace().small());
                    }
                    Err(stderr) => {
                        ui.colored_label(Palette::STATE_ERROR, format!("[{name}] rejected:"));
                        ui.label(
                            RichText::new(stderr)
                                .monospace()
                                .small()
                                .color(Palette::STATE_ERROR),
                        );
                    }
                }
            }
        });
}

fn run_preview(state: &mut AppState) {
    let plan = state.ui.dlg.push.payload();
    let targets = plan.targets.clone();
    let results = state.push_target_dry_run(&targets, plan.force);
    let summary = sync_service::summarize_dry_runs(&results);
    let commits: usize = targets
        .iter()
        .filter_map(|target| state.ui.dlg.push.row(&target.root))
        .map(|row| row.listed_total)
        .sum();
    let reports: Vec<(String, Result<String, String>)> = targets
        .iter()
        .zip(results)
        .map(|(target, (_, report))| (target.root.name(), report))
        .collect();
    state.ui.dlg.push.preview = Some(PushPreview {
        commits,
        remotes: summary.remotes,
        refs: summary.refs,
        rejected: summary.rejected,
        reports,
    });
}

fn execute_push(state: &mut AppState) {
    let plan = state.ui.dlg.push.payload();
    let mut request = PushRequest::batch(plan.targets.clone(), &state.settings);
    request.force = plan.force;
    request.tags = plan.tags;
    request.no_verify = plan.no_verify;
    request.skipped = plan.blocked.clone();
    state.dispatch_push(request, None);
}

fn rel_time(epoch_secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - epoch_secs).max(0);
    if delta < 60 {
        "just now".into()
    } else if delta < 3600 {
        format!("{delta}m ago")
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

fn fmt_date(epoch: i64) -> String {
    match Utc.timestamp_opt(epoch, 0) {
        chrono::LocalResult::Single(moment) => {
            let local: chrono::DateTime<chrono::Local> = chrono::DateTime::from(moment);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}
