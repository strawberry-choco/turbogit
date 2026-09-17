//! Multi-repo selection (issue #08, screen 04): tri-state checkbox
//! selection over the workspace tree plus the selection summary.
//!
//! Pure presentation logic (sidebar / smart_groups precedent): the
//! checked set is plain data — a `HashSet<RootId>` living in
//! [`turbogit_app::state::UiState`] — and every rule here takes the tree
//! plus that set, so the app crate never names UI types and this module
//! never owns state. The selection rules themselves live in
//! [`super::tree_selection`] over the recursive project tree
//! (sidebar-project-tree issue 02); this module owns the summary row
//! types and the rendering half ([`super::sidebar`] paints the checkboxes
//! and the bottom selection bar, [`super::shell`] the central summary
//! surface).

/// The three visual states of a group's checkbox, computed from its
/// descendant repos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// No descendant repo is checked.
    Unchecked,
    /// Some — but not all — descendant repos are checked.
    Partial,
    /// Every descendant repo is checked.
    Checked,
}

// --- Selection summary (issue #08, screen 04) --------------------------------

/// A selected repo's last commit: the subject line and a compact relative
/// age ("14m", "3h", "2d").
#[derive(Debug, Clone)]
pub struct LastCommit {
    pub subject: String,
    pub age: String,
}

/// One row of the selection summary's per-repo table.
#[derive(Debug, Clone)]
pub struct SelectionRow {
    pub name: String,
    pub branch: Option<String>,
    /// Outgoing commits vs upstream (the ↑ column).
    pub ahead: usize,
    /// Incoming commits vs upstream (the ↓ column).
    pub behind: usize,
    /// Modified + unversioned paths.
    pub dirty: usize,
    /// `None` when the root's log is not cached yet — the table shows a
    /// dash and the caller fetches the log lazily.
    pub last_commit: Option<LastCommit>,
}

/// The aggregate stats header plus per-repo table of the selection surface.
#[derive(Debug, Clone, Default)]
pub struct SelectionSummary {
    /// Selected repo count (the "REPOS" stat).
    pub repos: usize,
    /// Total outgoing commits across the selection ("AHEAD").
    pub ahead: usize,
    /// Total incoming commits across the selection ("BEHIND").
    pub behind: usize,
    /// Total dirty (modified + unversioned) files across the selection
    /// ("DIRTY FILES").
    pub dirty_files: usize,
    /// One row per selected repo, in tree order.
    pub rows: Vec<SelectionRow>,
}

/// Compact relative age (screen 04's "14m" / "1h" / "3h" style) from a
/// delta in seconds.
pub fn age_label(delta_secs: i64) -> String {
    if delta_secs < 60 {
        "<1m".to_string()
    } else if delta_secs < 3600 {
        format!("{}m", delta_secs / 60)
    } else if delta_secs < 86_400 {
        format!("{}h", delta_secs / 3600)
    } else {
        format!("{}d", delta_secs / 86_400)
    }
}

// --- Rendering ----------------------------------------------------------------

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Sense, Stroke, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::project_tree;
use super::tree_selection;
use super::widgets;
use crate::theme::Palette;
use turbogit_app::bulk_history::RepoOutcome;
use turbogit_app::state::AppState;
use turbogit_services::bulk_ops::BulkOp;

/// Table column offsets of the per-repo table (from the surface's left
/// content edge), matching screen 04's column rhythm.
const COL_BRANCH: f32 = 220.0;
const COL_SYNC: f32 = 380.0;
const COL_DIRTY: f32 = 470.0;
const COL_COMMIT: f32 = 545.0;

/// The central "Multi-repo selection" surface (screen 04): a header with
/// the Pin-as-view affordance, the aggregate stats row, and the per-repo
/// table. The shell routes here instead of the active tool window while
/// the selection is non-empty.
pub fn show_summary(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), Palette::BG);
    let mut col = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    let inset = Rect::from_min_max(
        Pos2::new(rect.left() + 24.0, rect.top() + 16.0),
        Pos2::new(rect.right() - 24.0, rect.bottom()),
    );
    let mut body = col.new_child(
        UiBuilder::new()
            .max_rect(inset)
            .layout(Layout::top_down(Align::Min)),
    );

    // Header: title + breadcrumb on the left, "Pin as view" on the right.
    let project = state
        .project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<workspace>")
        .to_string();
    let header_h = 32.0;
    let header = Rect::from_min_size(
        body.cursor().left_top(),
        Vec2::new(body.available_width(), header_h),
    );
    body.allocate_exact_size(Vec2::new(header.width(), header_h), Sense::hover());
    let hp = body.painter().clone();
    let hcy = header.center().y;
    icon_at(
        &mut body,
        Icon::LAYERS,
        Pos2::new(header.left() + 8.0, hcy),
        15.0,
        Palette::BRAND,
    );
    let title = hp.layout_no_wrap(
        "Multi-repo selection".to_string(),
        FontId::new(16.0, FontFamily::Proportional),
        Palette::INK,
    );
    hp.galley_with_override_text_color(
        Pos2::new(header.left() + 24.0, hcy - title.size().y / 2.0),
        title.clone(),
        Palette::INK,
    );
    let crumb = hp.layout_no_wrap(
        project,
        FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional),
        Palette::INK_3,
    );
    hp.galley_with_override_text_color(
        Pos2::new(
            header.left() + 24.0 + title.size().x + 12.0,
            hcy - crumb.size().y / 2.0,
        ),
        crumb,
        Palette::INK_3,
    );
    let mut header_right = body.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(header.left(), header.top()),
                Pos2::new(header.right(), header.bottom()),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    let pin = widgets::ghost_button(&mut header_right, None, "Pin as view");
    pin.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Pin as view"));
    if pin.clicked() {
        state.pin_selection();
    }

    body.add_space(10.0);

    // Lazy log fill: selected roots whose log is not cached fetch through
    // the same worker path the Log tab uses (the table shows a dash until
    // the cache fills).
    let tree = project_tree::build_tree(&state.project_dir, &state.multi.roots, &|id| {
        state.caches.ahead_behind(id)
    });
    let missing: Vec<turbogit_domain::model::RootId> =
        tree_selection::selected_repos(&tree, &state.ui.repo_selection)
            .iter()
            .filter(|r| state.caches.log(&r.id).is_none())
            .map(|r| r.id.clone())
            .collect();
    for id in missing {
        state.fetch_log(id);
    }
    let now = chrono::Local::now().timestamp();
    let summary = tree_selection::selection_summary(
        &tree,
        &state.ui.repo_selection,
        &|id| {
            state
                .caches
                .log(id)
                .and_then(|commits| commits.first())
                .map(|c| (c.message.lines().next().unwrap_or("").to_string(), c.time))
        },
        now,
    );

    // Aggregate stats row (screen 04): four label-over-number columns.
    let stats = [
        ("REPOS", summary.repos.to_string(), Palette::INK),
        ("AHEAD", summary.ahead.to_string(), Palette::STATE_SUCCESS),
        ("BEHIND", summary.behind.to_string(), Palette::STATE_INFO),
        (
            "DIRTY FILES",
            summary.dirty_files.to_string(),
            Palette::STATE_WARNING,
        ),
    ];
    let stats_rect = Rect::from_min_size(
        body.cursor().left_top(),
        Vec2::new(body.available_width(), 52.0),
    );
    body.allocate_exact_size(Vec2::new(stats_rect.width(), 52.0), Sense::hover());
    let sp = body.painter().clone();
    for (i, (label, value, color)) in stats.iter().enumerate() {
        let x = stats_rect.left() + i as f32 * 150.0;
        let label_galley = sp.layout_no_wrap(
            label.to_string(),
            FontId::new(crate::theme::TYPE_CONTROL, FontFamily::Proportional),
            Palette::INK_3,
        );
        sp.galley_with_override_text_color(
            Pos2::new(x, stats_rect.top() + 4.0),
            label_galley,
            Palette::INK_3,
        );
        let value_galley = sp.layout_no_wrap(
            value.clone(),
            FontId::new(crate::theme::TYPE_STATISTIC, FontFamily::Proportional),
            *color,
        );
        sp.galley_with_override_text_color(
            Pos2::new(x, stats_rect.top() + 20.0),
            value_galley,
            *color,
        );
    }

    body.add_space(12.0);

    // Per-repo table: header row, then one row per selected repo.
    let width = body.available_width();
    let header_rect = Rect::from_min_size(body.cursor().left_top(), Vec2::new(width, 24.0));
    body.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
    let thp = body.painter().clone();
    for (label, x) in [
        ("REPO", 0.0),
        ("BRANCH", COL_BRANCH),
        ("SYNC", COL_SYNC),
        ("DIRTY", COL_DIRTY),
        ("LAST COMMIT", COL_COMMIT),
    ] {
        let galley = thp.layout_no_wrap(
            label.to_string(),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::INK_3,
        );
        thp.galley_with_override_text_color(
            Pos2::new(
                header_rect.left() + x,
                header_rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::INK_3,
        );
    }
    thp.line_segment(
        [
            Pos2::new(header_rect.left(), header_rect.bottom()),
            Pos2::new(header_rect.right(), header_rect.bottom()),
        ],
        Stroke::new(1.0, Palette::LINE_SUBTLE),
    );

    const ROW_H: f32 = 34.0;
    let row_h = ROW_H;
    for row in &summary.rows {
        let r = Rect::from_min_size(body.cursor().left_top(), Vec2::new(width, row_h));
        body.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
        let response = body.interact(r, body.auto_id_with(("sel_row", &row.name)), Sense::click());
        if response.clicked()
            && let Some(id) = find_repo_by_label(&tree, &row.name).map(|r| r.id.clone())
        {
            state.selected_root = Some(id);
        }
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Focus {}", row.name))
        });
        let hovered = response.hovered();
        if hovered {
            body.painter()
                .rect_filled(r, CornerRadius::same(4), Palette::SURFACE_2);
        }
        let cy = r.center().y;
        let cell = |x: f32, text: String, color: Color32| {
            let galley = body.painter().layout_no_wrap(
                text,
                FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional),
                color,
            );
            body.painter().galley_with_override_text_color(
                Pos2::new(r.left() + x, cy - galley.size().y / 2.0),
                galley,
                color,
            );
        };
        cell(0.0, row.name.clone(), Palette::INK);
        let branch = row.branch.clone().unwrap_or_else(|| "<detached>".into());
        cell(COL_BRANCH, branch, Palette::INK_2);
        cell(
            COL_SYNC,
            format!("↑{} ↓{}", row.ahead, row.behind),
            Palette::INK_2,
        );
        let dirty_color = if row.dirty > 0 {
            Palette::STATE_WARNING
        } else {
            Palette::INK_3
        };
        cell(COL_DIRTY, row.dirty.to_string(), dirty_color);
        match &row.last_commit {
            Some(lc) => cell(
                COL_COMMIT,
                format!("{} · {}", lc.subject, lc.age),
                Palette::INK_2,
            ),
            None => cell(COL_COMMIT, "—".to_string(), Palette::INK_3),
        }
    }

    // Operations grid (issue 09, screen 04): the fleet sync operations.
    // Choosing any of them opens the preflight matrix modal first —
    // nothing runs straight from the grid.
    body.add_space(20.0);
    let ops_width = body.available_width();
    let title_rect = Rect::from_min_size(body.cursor().left_top(), Vec2::new(ops_width, 18.0));
    body.allocate_exact_size(Vec2::new(ops_width, 18.0), Sense::hover());
    let tp = body.painter().clone();
    let title = tp.layout_no_wrap(
        "OPERATIONS".to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    tp.galley_with_override_text_color(
        Pos2::new(title_rect.left(), title_rect.top() + 2.0),
        title,
        Palette::INK_3,
    );
    let hint = tp.layout_no_wrap(
        format!("· RUNS ON ALL {}", summary.repos),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    tp.galley_with_override_text_color(
        Pos2::new(title_rect.left() + 88.0, title_rect.top() + 2.0),
        hint,
        Palette::INK_3,
    );
    let hint_right = tp.layout_no_wrap(
        "every op shows a preflight matrix first".to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    tp.galley_with_override_text_color(
        Pos2::new(
            title_rect.right() - hint_right.size().x,
            title_rect.top() + 2.0,
        ),
        hint_right,
        Palette::INK_3,
    );

    // Two rows of four grid cells, per the mockup. The four fleet sync
    // operations, the cascade branch op (issue 11), and the custom command
    // (issue 13) are wired; the remaining mockup actions (tag release,
    // cherry-pick across) belong to their own issues and render as inert
    // controls — scope gaps are recorded, never hidden (CONTEXT.md "Inert
    // control").
    const OP_COLS: usize = 4;
    let cell_gap = 12.0;
    let cell_w = (ops_width - cell_gap * (OP_COLS - 1) as f32) / OP_COLS as f32;
    let cell_h = 44.0;
    let wired: [[Option<BulkOp>; OP_COLS]; 2] = [
        [
            Some(BulkOp::FetchAll),
            Some(BulkOp::PullAll),
            Some(BulkOp::PushAll),
            // The flagship cascade op (issue 11): create & checkout branch.
            Some(BulkOp::CreateBranch),
        ],
        [
            Some(BulkOp::StashAll),
            None,
            None,
            // The user-typed fleet command (issue 13).
            Some(BulkOp::Custom),
        ],
    ];
    let inert_labels = [
        [None, None, None, None],
        [None, Some("Tag release"), Some("Cherry-pick across"), None],
    ];
    for (r, row_ops) in wired.iter().enumerate() {
        let row_y = body.cursor().top() + if r == 0 { 8.0 } else { cell_h + cell_gap };
        let row_rect = Rect::from_min_size(
            Pos2::new(body.cursor().left(), row_y),
            Vec2::new(ops_width, cell_h),
        );
        body.allocate_exact_size(Vec2::new(ops_width, cell_h), Sense::hover());
        let mut row_ui = body.new_child(
            UiBuilder::new()
                .max_rect(row_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        for (c, op) in row_ops.iter().enumerate() {
            let cell_rect = Rect::from_min_size(
                Pos2::new(
                    row_rect.left() + c as f32 * (cell_w + cell_gap),
                    row_rect.top(),
                ),
                Vec2::new(cell_w, cell_h),
            );
            let mut cell = row_ui.new_child(
                UiBuilder::new()
                    .max_rect(cell_rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            let (label, action) = match (op, inert_labels[r][c]) {
                (Some(op), _) => (op.label(), Some(*op)),
                (None, Some(label)) => (label, None),
                (None, None) => continue,
            };
            let btn = widgets::ghost_button(&mut cell, None, label);
            btn.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.to_string()));
            if let Some(op) = action
                && btn.clicked()
            {
                state.open_bulk_preflight(op);
            }
        }
    }

    // Recent bulk operations (issue 12, screen 04): one row per completed
    // run — time, op, repo count, outcome summary — with per-row Details
    // drill-down and Rollback where reversible.
    body.add_space(20.0);
    let hist_width = body.available_width();
    let header_rect = Rect::from_min_size(body.cursor().left_top(), Vec2::new(hist_width, 18.0));
    body.allocate_exact_size(Vec2::new(hist_width, 18.0), Sense::hover());
    let hp = body.painter().clone();
    let title = hp.layout_no_wrap(
        "RECENT BULK OPERATIONS".to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    hp.galley_with_override_text_color(
        Pos2::new(header_rect.left(), header_rect.top() + 2.0),
        title,
        Palette::INK_3,
    );
    let hint_right = hp.layout_no_wrap(
        "per-row drill-down · rollback where reversible".to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    hp.galley_with_override_text_color(
        Pos2::new(
            header_rect.right() - hint_right.size().x,
            header_rect.top() + 2.0,
        ),
        hint_right,
        Palette::INK_3,
    );

    for rec in state.ui.bulk_history.clone() {
        body.add_space(8.0);
        let op_label = if rec.op == BulkOp::CreateBranch && !rec.branch.is_empty() {
            format!("Create branch {}", rec.branch)
        } else {
            rec.op.label().to_string()
        };
        let row_rect = Rect::from_min_size(body.cursor().left_top(), Vec2::new(hist_width, 26.0));
        body.allocate_exact_size(Vec2::new(hist_width, 26.0), Sense::hover());
        let rp = body.painter().clone();
        let cy = row_rect.center().y;
        let cell = |x: f32, text: String, color: Color32, right: bool| {
            let galley = rp.layout_no_wrap(
                text,
                FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional),
                color,
            );
            let x = if right {
                row_rect.right() - galley.size().x
            } else {
                row_rect.left() + x
            };
            rp.galley_with_override_text_color(
                Pos2::new(x, cy - galley.size().y / 2.0),
                galley,
                color,
            )
        };
        cell(
            0.0,
            turbogit_app::bulk_history::format_time(rec.at),
            Palette::INK_2,
            false,
        );
        cell(
            80.0,
            format!("{} · {} repos", op_label, rec.repos.len()),
            Palette::INK,
            false,
        );
        cell(400.0, rec.summary(), Palette::INK_2, false);

        // Details / Rollback on the right, per the mockup.
        let actions = Rect::from_min_size(
            Pos2::new(row_rect.right() - 180.0, row_rect.top()),
            Vec2::new(180.0, 26.0),
        );
        let mut actions_ui = body.new_child(
            UiBuilder::new()
                .max_rect(actions)
                .layout(Layout::right_to_left(Align::Center)),
        );
        if rec.reversible() && !rec.rolled_back {
            let rollback_label = format!("Rollback {op_label}");
            let btn = widgets::ghost_button(&mut actions_ui, None, "Rollback");
            btn.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, true, rollback_label.clone())
            });
            if btn.clicked() {
                state.bulk_rollback(rec.id);
            }
        }
        let details_label = format!("Details {op_label}");
        let btn = widgets::ghost_button(&mut actions_ui, None, "Details");
        btn.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, details_label.clone()));
        if btn.clicked() {
            state.ui.bulk_history_open = if state.ui.bulk_history_open == Some(rec.id) {
                None
            } else {
                Some(rec.id)
            };
        }

        // Details drill-down: one outcome row per repo.
        if state.ui.bulk_history_open == Some(rec.id) {
            for repo in &rec.repos {
                let detail_rect =
                    Rect::from_min_size(body.cursor().left_top(), Vec2::new(hist_width, 20.0));
                body.allocate_exact_size(Vec2::new(hist_width, 20.0), Sense::hover());
                let dp = body.painter().clone();
                let cy = detail_rect.center().y;
                let outcome = match &repo.outcome {
                    RepoOutcome::Done => "done".to_string(),
                    RepoOutcome::Failed { error } => format!("failed ({error})"),
                    RepoOutcome::Skipped { reason } => format!("skipped ({reason})"),
                };
                let galley = dp.layout_no_wrap(
                    format!("{} · {}", repo.name, outcome),
                    FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional),
                    Palette::INK_2,
                );
                dp.galley_with_override_text_color(
                    Pos2::new(detail_rect.left() + 80.0, cy - galley.size().y / 2.0),
                    galley,
                    Palette::INK_2,
                );
            }
        }
    }
}

/// Paint one icon primitive centered at `origin` without disturbing layout
/// (mirrors `sidebar::icon_at`).
fn icon_at(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let mut child =
        ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(center, Vec2::splat(size))));
    icons::icon(&mut child, icon, size, color);
}

/// The repo whose display label equals `label`, walking the recursive tree
/// in pre-order (the selection summary's row names are those labels).
fn find_repo_by_label<'a>(
    tree: &'a project_tree::ProjectTree,
    label: &str,
) -> Option<&'a project_tree::RepoNode> {
    fn walk<'a>(
        nodes: &'a [project_tree::ProjectNode],
        label: &str,
    ) -> Option<&'a project_tree::RepoNode> {
        for node in nodes {
            match node {
                project_tree::ProjectNode::Repo(r) => {
                    if r.label == label {
                        return Some(r);
                    }
                    if let Some(found) = walk(&r.children, label) {
                        return Some(found);
                    }
                }
                project_tree::ProjectNode::Folder(f) => {
                    if let Some(found) = walk(&f.children, label) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    walk(&tree.nodes, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_label_formats_deltas_compactly() {
        assert_eq!(age_label(30), "<1m");
        assert_eq!(age_label(14 * 60), "14m");
        assert_eq!(age_label(3 * 3600 + 40 * 60), "3h");
        assert_eq!(age_label(2 * 86_400 + 5 * 3600), "2d");
    }
}
