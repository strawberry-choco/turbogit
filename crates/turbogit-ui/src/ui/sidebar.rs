//! Workspace tree sidebar (issue #05, screen 01): the left rail listing
//! every discovered repository root as a tree of projects → repos.
//!
//! The tree model below is pure presentation logic (hunk_nav precedent):
//! it groups roots by their first path component under the project
//! directory, computes aggregate counts, applies the live filter, and maps
//! each root's git state onto one [`DotState`]. The rendering half lives
//! in [`super::shell`], which owns the left-rail geometry.

use std::path::{Path, PathBuf};

use turbogit_domain::model::{Root, RootId};

/// One repository row of the tree.
#[derive(Debug, Clone)]
pub struct SidebarRepo {
    pub id: RootId,
    /// Repo row label: the root path's file name.
    pub name: String,
    /// Full path (the filter matches against it).
    pub path: PathBuf,
    /// Current branch label, `None` when detached.
    pub branch: Option<String>,
    /// The row's status dot (CONTEXT.md: clean / dirty / conflict / diverged).
    pub dot: DotState,
    /// Outgoing commits vs upstream (↑ badge).
    pub ahead: usize,
    /// Incoming commits vs upstream (↓ badge).
    pub behind: usize,
    /// Conflicted paths (the smart-group conflict predicate, issue #06).
    pub conflicts: usize,
    /// Modified + unversioned paths (the smart-group dirty predicate).
    pub dirty_count: usize,
}

/// One collapsible project group: roots sharing the first path component
/// under the project directory, with aggregate counts.
#[derive(Debug, Clone)]
pub struct SidebarGroup {
    pub name: String,
    pub repos: Vec<SidebarRepo>,
}

impl SidebarGroup {
    /// Repos in the group (the group-header count).
    pub fn total(&self) -> usize {
        self.repos.len()
    }

    /// Repos carrying uncommitted work (the group's dirty badge).
    pub fn dirty(&self) -> usize {
        self.repos
            .iter()
            .filter(|r| matches!(r.dot, DotState::Dirty | DotState::Conflict))
            .count()
    }
}

/// The full tree: groups in name order plus the top-level repo total.
#[derive(Debug, Clone, Default)]
pub struct SidebarTree {
    pub groups: Vec<SidebarGroup>,
    /// Total repos across every group (the workspace-header badge).
    pub total: usize,
}

/// The four row-level states a repo's status dot can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotState {
    Clean,
    Dirty,
    Conflict,
    Diverged,
}

/// Apply the sidebar filter live: a repo survives when the query matches
/// its name, its full path, or its current branch (case-insensitive); a
/// group survives when any of its repos does. The empty query returns the
/// tree unchanged. Counts recompute from the surviving rows.
pub fn filter_tree(tree: &SidebarTree, query: &str) -> SidebarTree {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return tree.clone();
    }
    let mut groups = Vec::new();
    let mut total = 0;
    for group in &tree.groups {
        // A group-name match keeps the whole group; otherwise only the
        // rows matching on name / path / branch survive.
        let keep_all = group.name.to_lowercase().contains(&q);
        let repos: Vec<SidebarRepo> = group
            .repos
            .iter()
            .filter(|r| {
                keep_all
                    || r.name.to_lowercase().contains(&q)
                    || r.path.to_string_lossy().to_lowercase().contains(&q)
                    || r.branch
                        .as_deref()
                        .is_some_and(|b| b.to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        if !repos.is_empty() {
            total += repos.len();
            groups.push(SidebarGroup {
                name: group.name.clone(),
                repos,
            });
        }
    }
    SidebarTree { groups, total }
}

// --- Rendering ---------------------------------------------------------------

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::multi_selection::{self, CheckState};
use super::smart_groups::{self, GroupFilter, filter_to};
use super::widgets;
use crate::theme::Palette;
use turbogit_app::root_caches::Affected;
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::{AppState, Dialog, Toast};

/// Left rail width (screen 01); also used by the shell's geometry.
pub const SIDEBAR_WIDTH: f32 = 280.0;

const ROW_HEIGHT: f32 = 26.0;
const GROUP_HEIGHT: f32 = 24.0;
/// Right zone of a rule row reserved for its edit/delete affordances;
/// the row's click target shrinks by the same amount.
const RULE_BUTTONS_ZONE: f32 = 56.0;
/// Height of the bottom selection bar (issue #08), shown only while a
/// selection is live.
const SELECTION_BAR_HEIGHT: f32 = 36.0;

/// Paint one icon primitive centered at `origin` without disturbing layout
/// (mirrors `shell::paint_icon_centered`).
fn icon_at(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let mut child =
        ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(center, Vec2::splat(size))));
    icons::icon(&mut child, icon, size, color);
}

/// The square a row's selection checkbox occupies (screen 04's leading
/// checkbox column).
fn checkbox_rect(row: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(row.left() + 15.0, row.center().y),
        Vec2::splat(13.0),
    )
}

/// Tri-state selection checkbox (issue #08, screen 04): checked = brand
/// fill + check, partial = brand fill + dash, unchecked = outline. Owns
/// its interact id so the click never reaches the row behind it.
fn tri_state_checkbox(
    ui: &mut Ui,
    rect: Rect,
    id: impl egui::AsIdSalt,
    label: String,
    state: CheckState,
) -> egui::Response {
    let response = ui.interact(rect, ui.auto_id_with(id), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, label.clone()));
    let painter = ui.painter().clone();
    match state {
        CheckState::Checked => {
            painter.rect_filled(rect, CornerRadius::same(3), Palette::BRAND);
            icon_at(ui, Icon::CHECK, rect.center(), 10.0, Palette::BRAND_INK);
        }
        CheckState::Partial => {
            painter.rect_filled(rect, CornerRadius::same(3), Palette::BRAND);
            painter.line_segment(
                [
                    Pos2::new(rect.left() + 3.5, rect.center().y),
                    Pos2::new(rect.right() - 3.5, rect.center().y),
                ],
                Stroke::new(2.0, Palette::BRAND_INK),
            );
        }
        CheckState::Unchecked => {
            let fill = if response.hovered() {
                Palette::SURFACE_2
            } else {
                Palette::BG
            };
            painter.rect_filled(rect, CornerRadius::same(3), fill);
            painter.rect_stroke(
                rect,
                CornerRadius::same(3),
                Stroke::new(1.0, Palette::LINE),
                egui::StrokeKind::Inside,
            );
        }
    }
    response
}

/// One chip per pinned view (issue #08): clicking restores the saved
/// selection filtered to the roots still registered.
fn render_pinned_views(ui: &mut Ui, state: &mut AppState) {
    if state.ui.pinned_views.is_empty() {
        return;
    }
    widgets::group_title(ui, "PINNED VIEWS");
    ui.add_space(4.0);
    let width = ui.available_width();
    let row = Rect::from_min_size(ui.cursor().left_top(), Vec2::new(width, 26.0));
    ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    let mut chips = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let views = state.ui.pinned_views.clone();
    for view in &views {
        let chip = widgets::compact_button(&mut chips, &view.name);
        chip.widget_info(|| {
            WidgetInfo::labeled(
                WidgetType::Button,
                true,
                format!("Restore view {}", view.name),
            )
        });
        if chip.clicked() {
            let registered: Vec<_> = state.multi.roots.iter().map(|r| r.id.clone()).collect();
            state.ui.repo_selection =
                turbogit_app::pinned_views::selection_from_view(view, &registered);
        }
    }
    ui.add_space(8.0);
}

/// The bottom selection bar (screen 04): "N selected / total" on the left,
/// the quick Fetch / Pull / Branch… actions and Clear on the right. Rendered
/// only while a selection is live (the caller reserves the height).
fn render_selection_bar(ui: &mut Ui, state: &mut AppState, total: usize) {
    let rect = ui.max_rect();
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(0), Palette::SURFACE);
    painter.line_segment(
        [
            Pos2::new(rect.left(), rect.top() + 0.5),
            Pos2::new(rect.right(), rect.top() + 0.5),
        ],
        Stroke::new(1.0, Palette::LINE_SUBTLE),
    );

    let cy = rect.center().y;
    let count = format!("{} selected / {}", state.ui.repo_selection.len(), total);
    let count_galley = painter.layout_no_wrap(
        count,
        FontId::new(12.0, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(rect.left() + 12.0, cy - count_galley.size().y / 2.0),
        count_galley,
        Palette::INK,
    );

    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.left(), rect.top()),
                Pos2::new(rect.right() - 12.0, rect.bottom()),
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    let clear = widgets::compact_button(&mut right, "Clear");
    clear.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Clear selection"));
    if clear.clicked() {
        state.ui.repo_selection.clear();
    }
    let branch = widgets::compact_button(&mut right, "Branch…");
    branch
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "New branch for selection"));
    if branch.clicked() {
        state.ui.dialog = Some(Dialog::NewBranch);
    }
    let pull = widgets::compact_button(&mut right, "Pull");
    pull.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Pull selection"));
    if pull.clicked() {
        pull_selection(state);
    }
    let fetch = widgets::compact_button(&mut right, "Fetch");
    fetch.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Fetch selection"));
    if fetch.clicked() {
        fetch_selection(state);
    }
}

/// `git fetch` on every selected root, one worker op (issue #08).
fn fetch_selection(state: &mut AppState) {
    let paths: Vec<PathBuf> = state
        .ui
        .repo_selection
        .iter()
        .map(|id| id.0.to_path_buf())
        .collect();
    let n = paths.len();
    state.run_git(format!("Fetch · {n} repos"), Affected::All, move |v| {
        for p in &paths {
            v.fetch(p, None)?;
        }
        Ok(())
    });
}

/// `git pull` on every selected root, one worker op (issue #08).
fn pull_selection(state: &mut AppState) {
    let paths: Vec<PathBuf> = state
        .ui
        .repo_selection
        .iter()
        .map(|id| id.0.to_path_buf())
        .collect();
    let n = paths.len();
    state.run_git(format!("Pull · {n} repos"), Affected::All, move |v| {
        for p in &paths {
            v.pull(p, false)?;
        }
        Ok(())
    });
}

/// The STATE-family token a row's dot paints with. Conflict shares the
/// error red with diverged (the domain STATUS_* aliases); the row's badges
/// and the rest of the shell disambiguate.
pub fn dot_color(dot: DotState) -> Color32 {
    match dot {
        DotState::Clean => Palette::STATUS_CLEAN,
        DotState::Dirty => Palette::STATUS_DIRTY,
        DotState::Diverged => Palette::STATUS_DIVERGED,
        DotState::Conflict => Palette::STATE_ERROR,
    }
}

/// Render the workspace tree into `ui`, whose max rect is the shell's
/// reserved left rail (see `shell::render`).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, CornerRadius::same(0), Palette::BG);
    ui.painter().line_segment(
        [
            Pos2::new(rect.right() - 0.5, rect.top()),
            Pos2::new(rect.right() - 0.5, rect.bottom()),
        ],
        Stroke::new(1.0, Palette::LINE),
    );

    let mut col = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    col.add_space(8.0);
    render_workspace_header(&mut col, state);
    col.add_space(10.0);
    widgets::search_input(
        &mut col,
        "Filter repos / branches…",
        &mut state.ui.sidebar_filter,
    );
    col.add_space(12.0);

    // Snapshot the tree so the rows borrow nothing while clicks mutate
    // `selected_root` / collapsed state.
    let full_tree = build_tree(&state.project_dir, &state.multi.roots, &|id| {
        state.caches.ahead_behind(id)
    });
    render_smart_groups(&mut col, state, &full_tree);
    col.add_space(12.0);
    // Pinned views (issue #08): always reachable, including with an empty
    // selection — recall is the only way back into a saved view.
    render_pinned_views(&mut col, state);
    col.add_space(12.0);

    col.label(
        RichText::new("PROJECTS")
            .strong()
            .font(FontId::new(11.0, FontFamily::Proportional))
            .color(Palette::INK_3),
    );

    let tree = filter_to(&full_tree, active_group_filter(state).as_ref());
    let tree = filter_tree(&tree, &state.ui.sidebar_filter);

    // The bottom selection bar (issue #08) claims a strip of the rail while
    // a selection is live; the tree scrolls in the space above it.
    let bar_h = if state.ui.repo_selection.is_empty() {
        0.0
    } else {
        SELECTION_BAR_HEIGHT
    };
    let rest = Rect::from_min_max(
        col.cursor().left_top(),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let list_rect = Rect::from_min_max(rest.min, Pos2::new(rest.max.x, rest.max.y - bar_h));
    let mut list = col.new_child(
        UiBuilder::new()
            .max_rect(list_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    ScrollArea::vertical().show(&mut list, |ui| {
        for group in &tree.groups {
            render_group(ui, state, &tree, group);
        }
    });
    if bar_h > 0.0 {
        let bar_rect = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - bar_h),
            Pos2::new(rect.right(), rect.bottom()),
        );
        let mut bar = col.new_child(
            UiBuilder::new()
                .max_rect(bar_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        render_selection_bar(&mut bar, state, full_tree.total);
    }
}

/// Workspace header row: folder icon + project basename + right-aligned
/// total repo count badge (screen 01's "37"). The workspace picker itself
/// belongs to issue #34 — the row is inert in v1.
fn render_workspace_header(ui: &mut Ui, state: &mut AppState) {
    let name = state
        .project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<workspace>")
        .to_string();
    let total = state.multi.roots.len().to_string();
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        icons::icon(ui, Icon::FOLDER_GIT, 14.0, Palette::BRAND);
        ui.add_space(6.0);
        ui.label(
            RichText::new(&name)
                .strong()
                .font(FontId::new(13.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new(total)
                    .font(FontId::new(11.0, FontFamily::Proportional))
                    .color(Palette::INK_2),
            );
            icons::icon(ui, Icon::CHEVRON_DOWN, 12.0, Palette::INK_3);
        });
    });
}

/// The SMART GROUPS section (issue #06, screens 01/04; user rules are
/// issue #07): the section title with the "+ New rule" trigger, then one
/// dot + label + count row per non-empty built-in group followed by one
/// row per user-defined rule. Built-in membership is recomputed from live
/// repo state each frame, so it follows refreshes without manual action;
/// user rules always render (they are persisted configuration — hiding a
/// zero-member rule would strand its editor), with their live count.
fn render_smart_groups(ui: &mut Ui, state: &mut AppState, tree: &SidebarTree) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(
            RichText::new("SMART GROUPS")
                .strong()
                .font(FontId::new(11.0, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            let plus = ui.add(egui::Button::new(RichText::new("+").color(Palette::INK_3)));
            plus.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "New rule"));
            if plus.clicked() {
                state.ui.smart_rule_editor_open = true;
                state.ui.smart_rule_editing = None;
                state.ui.smart_rule_draft = None;
            }
        });
    });
    for entry in smart_groups::smart_groups(tree) {
        render_smart_group_row(ui, state, &entry);
    }
    for (ix, rule) in state.ui.smart_group_rules.clone().iter().enumerate() {
        render_rule_group_row(ui, state, tree, ix, rule);
    }
}

/// One smart-group row: colored dot + label, right-aligned member count.
fn render_smart_group_row(
    ui: &mut Ui,
    state: &mut AppState,
    entry: &smart_groups::SmartGroupEntry,
) {
    let width = ui.available_width();
    let row = Rect::from_min_size(
        Pos2::new(ui.cursor().left(), ui.cursor().top()),
        Vec2::new(width, GROUP_HEIGHT),
    );
    ui.allocate_exact_size(Vec2::new(width, GROUP_HEIGHT), Sense::hover());
    let response = ui.interact(
        row,
        ui.auto_id_with(("smart_group", entry.group)),
        Sense::click(),
    );
    if response.clicked() {
        toggle_smart_group(state, entry.group);
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, entry.group.label()));

    let painter = ui.painter().clone();
    if is_smart_group_active(state, entry.group.label()) {
        painter.rect_filled(row, CornerRadius::same(0), Palette::selection_bg());
        painter.line_segment(
            [
                Pos2::new(row.left() + 1.0, row.top()),
                Pos2::new(row.left() + 1.0, row.bottom()),
            ],
            Stroke::new(2.0, Palette::BRAND),
        );
    } else if response.hovered() {
        painter.rect_filled(row, CornerRadius::same(4), Palette::SURFACE_2);
    }

    let cy = row.center().y;
    painter.circle_filled(
        Pos2::new(row.left() + 24.0, cy),
        3.5,
        smart_group_color(entry.group),
    );
    let label_galley = ui.painter().layout_no_wrap(
        entry.group.label().to_string(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(row.left() + 38.0, cy - label_galley.size().y / 2.0),
        label_galley,
        Palette::INK,
    );
    let count_galley = painter.layout_no_wrap(
        entry.count.to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_2,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            row.right() - 12.0 - count_galley.size().x,
            cy - count_galley.size().y / 2.0,
        ),
        count_galley,
        Palette::INK_2,
    );
}

/// The semantic token a group's dot paints with (matching the repo rows:
/// diverged and conflicts share the error red family, unpushed the success
/// green, dirty the warning amber).
fn smart_group_color(group: smart_groups::SmartGroup) -> Color32 {
    match group {
        smart_groups::SmartGroup::Diverged => Palette::STATUS_DIVERGED,
        smart_groups::SmartGroup::Conflicted => Palette::STATE_ERROR,
        smart_groups::SmartGroup::Unpushed => Palette::STATE_SUCCESS,
        smart_groups::SmartGroup::Dirty => Palette::STATUS_DIRTY,
    }
}

/// One user-defined rule's row (issue #07): the same dot + label + count
/// anatomy as the built-ins, in the info-blue family, clickable to filter
/// the tree to the rule's members. Rules render even at count zero —
/// they are persisted configuration, not computed views.
fn render_rule_group_row(
    ui: &mut Ui,
    state: &mut AppState,
    tree: &SidebarTree,
    ix: usize,
    rule: &SmartGroupRule,
) {
    let width = ui.available_width();
    let row = Rect::from_min_size(
        Pos2::new(ui.cursor().left(), ui.cursor().top()),
        Vec2::new(width, GROUP_HEIGHT),
    );
    ui.allocate_exact_size(Vec2::new(width, GROUP_HEIGHT), Sense::hover());
    // The click target excludes the affordances zone so the pencil/trash
    // buttons never fight the row's own filter toggle.
    let clickable = Rect::from_min_max(
        row.min,
        Pos2::new(
            (row.right() - RULE_BUTTONS_ZONE).max(row.left()),
            row.bottom(),
        ),
    );
    let response = ui.interact(
        clickable,
        ui.auto_id_with(("smart_rule", ix)),
        Sense::click(),
    );
    if response.clicked() {
        toggle_group_filter(state, &rule.label);
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &rule.label));

    let painter = ui.painter().clone();
    if is_smart_group_active(state, &rule.label) {
        painter.rect_filled(row, CornerRadius::same(0), Palette::selection_bg());
        painter.line_segment(
            [
                Pos2::new(row.left() + 1.0, row.top()),
                Pos2::new(row.left() + 1.0, row.bottom()),
            ],
            Stroke::new(2.0, Palette::BRAND),
        );
    } else if response.hovered() {
        painter.rect_filled(row, CornerRadius::same(4), Palette::SURFACE_2);
    }

    let cy = row.center().y;
    painter.circle_filled(Pos2::new(row.left() + 24.0, cy), 3.5, Palette::STATE_INFO);
    let label_galley = ui.painter().layout_no_wrap(
        rule.label.clone(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(row.left() + 38.0, cy - label_galley.size().y / 2.0),
        label_galley,
        Palette::INK,
    );
    let count = smart_groups::rule_count(tree, rule);
    let count_galley = painter.layout_no_wrap(
        count.to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_2,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            row.right() - RULE_BUTTONS_ZONE - count_galley.size().x,
            cy - count_galley.size().y / 2.0,
        ),
        count_galley,
        Palette::INK_2,
    );

    // Edit/delete affordances (issue #07), right-aligned in the reserved
    // zone. Registered after the row's interact so they take the click.
    let buttons = Rect::from_min_max(
        Pos2::new(row.right() - RULE_BUTTONS_ZONE, row.top()),
        Pos2::new(row.right(), row.bottom()),
    );
    let mut zone = ui.new_child(
        UiBuilder::new()
            .max_rect(buttons)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let delete = widgets::icon_button(&mut zone, Icon::TRASH_2);
    delete.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("Delete rule {}", rule.label),
        )
    });
    if delete.clicked() {
        delete_rule(state, ix);
    }
    let edit = widgets::icon_button(&mut zone, Icon::PENCIL);
    edit.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("Edit rule {}", rule.label),
        )
    });
    if edit.clicked() {
        state.ui.smart_rule_editor_open = true;
        state.ui.smart_rule_editing = Some(ix);
        state.ui.smart_rule_draft = Some(rule.clone());
    }
}

/// Remove a rule (issue #07): drop it from live state + the workspace's
/// `ui.ron`, and clear the active filter if it pointed at the rule.
fn delete_rule(state: &mut AppState, ix: usize) {
    if let Some(rule) = state.ui.smart_group_rules.get(ix)
        && state.ui.sidebar_smart_group.as_deref() == Some(rule.label.as_str())
    {
        state.ui.sidebar_smart_group = None;
    }
    state.ui.smart_group_rules.remove(ix);
    state.persist_ui();
    state.ui.toast = Some(Toast::info("Smart group rule deleted"));
}

/// The active smart-group filter, if a group row is toggled on: the
/// stored label resolves to a built-in predicate or a user-defined rule
/// (issue #07); unknown labels — e.g. from a stale session — deactivate.
fn is_smart_group_active(state: &AppState, label: &str) -> bool {
    state.ui.sidebar_smart_group.as_deref() == Some(label)
}

/// Resolve the stored active-group label back to its filter.
fn active_group_filter(state: &AppState) -> Option<GroupFilter> {
    let label = state.ui.sidebar_smart_group.as_deref()?;
    if let Some(group) = smart_groups::SmartGroup::BUILTINS
        .iter()
        .copied()
        .find(|g| g.label() == label)
    {
        return Some(GroupFilter::BuiltIn(group));
    }
    state
        .ui
        .smart_group_rules
        .iter()
        .find(|r| r.label == label)
        .map(|r| GroupFilter::Custom(r.clone()))
}

/// Clicking a group filters the tree to its members; clicking it again
/// (or another group) clears / replaces the filter.
fn toggle_smart_group(state: &mut AppState, group: smart_groups::SmartGroup) {
    toggle_group_filter(state, group.label());
}

fn toggle_group_filter(state: &mut AppState, label: &str) {
    if state.ui.sidebar_smart_group.as_deref() == Some(label) {
        state.ui.sidebar_smart_group = None;
    } else {
        state.ui.sidebar_smart_group = Some(label.to_string());
    }
}

/// One project group: a tri-state selection checkbox, then a clickable
/// header (chevron + folder + name + dirty badge + repo count) and, when
/// expanded, one row per repo.
fn render_group(ui: &mut Ui, state: &mut AppState, tree: &SidebarTree, group: &SidebarGroup) {
    let expanded = !state.ui.sidebar_collapsed.contains(&group.name);
    let header = group_header_rect(ui, group.name.clone());
    let response = ui.interact(
        header,
        ui.auto_id_with(("sidebar_group", &group.name)),
        Sense::click(),
    );
    if response.clicked() {
        if expanded {
            state.ui.sidebar_collapsed.insert(group.name.clone());
        } else {
            state.ui.sidebar_collapsed.remove(&group.name);
        }
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, group.name.as_str()));
    // The checkbox interacts after the header so it sits above it and
    // takes the click (the rule-row precedent).
    let checkbox = tri_state_checkbox(
        ui,
        checkbox_rect(header),
        ("sel_group", &group.name),
        format!("Select group {}", group.name),
        multi_selection::group_state(tree, &state.ui.repo_selection, &group.name),
    );
    if checkbox.clicked() {
        multi_selection::toggle_group(&mut state.ui.repo_selection, tree, &group.name);
    }

    let painter = ui.painter().clone();
    if response.hovered() {
        painter.rect_filled(header, CornerRadius::same(4), Palette::SURFACE_2);
    }
    let cy = header.center().y;
    let chevron = if expanded {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icon_at(
        ui,
        chevron,
        Pos2::new(header.left() + 28.0, cy),
        12.0,
        Palette::INK_3,
    );
    icon_at(
        ui,
        Icon::FOLDER,
        Pos2::new(header.left() + 46.0, cy),
        14.0,
        if expanded {
            Palette::BRAND
        } else {
            Palette::INK_2
        },
    );
    let name_galley = ui.painter().layout_no_wrap(
        group.name.clone(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(header.left() + 60.0, cy - name_galley.size().y / 2.0),
        name_galley,
        Palette::INK,
    );
    // Right cluster: dirty count badge (when any), then the repo count.
    let mut right = header.right() - 12.0;
    if group.dirty() > 0 {
        let galley = ui.painter().layout_no_wrap(
            group.dirty().to_string(),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::STATE_WARNING,
        );
        right -= galley.size().x + 10.0;
        painter.galley_with_override_text_color(
            Pos2::new(right, cy - galley.size().y / 2.0),
            galley,
            Palette::STATE_WARNING,
        );
    }
    let count_galley = ui.painter().layout_no_wrap(
        group.total().to_string(),
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    right -= count_galley.size().x;
    painter.galley_with_override_text_color(
        Pos2::new(right, cy - count_galley.size().y / 2.0),
        count_galley,
        Palette::INK_3,
    );

    if expanded {
        for repo in &group.repos {
            render_repo_row(ui, state, repo);
        }
    }
}

/// Reserve one full-width row inside the scroll area and return its rect.
fn group_header_rect(ui: &mut Ui, name: String) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, GROUP_HEIGHT), Sense::hover());
    let _ = name;
    rect
}

/// One repository row: tri-state selection checkbox, status dot + name,
/// right-aligned ahead/behind badges and branch label. Clicking the row
/// focuses the repo everywhere; clicking the checkbox toggles selection.
fn render_repo_row(ui: &mut Ui, state: &mut AppState, repo: &SidebarRepo) {
    let width = ui.available_width();
    let row = Rect::from_min_size(
        Pos2::new(ui.cursor().left(), ui.cursor().top()),
        Vec2::new(width, ROW_HEIGHT),
    );
    ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::hover());
    let selected = state.selected_root.as_ref() == Some(&repo.id);
    let response = ui.interact(
        row,
        ui.auto_id_with(("sidebar_repo", &repo.id)),
        Sense::click(),
    );
    if response.clicked() {
        state.selected_root = Some(repo.id.clone());
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, repo.name.as_str()));
    // The checkbox interacts after the row so it sits above it and takes
    // the click (the rule-row precedent).
    let checkbox = tri_state_checkbox(
        ui,
        checkbox_rect(row),
        ("sel_repo", &repo.id),
        format!("Select repo {}", repo.name),
        if state.ui.repo_selection.contains(&repo.id) {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        },
    );
    if checkbox.clicked() {
        multi_selection::toggle_repo(&mut state.ui.repo_selection, repo);
    }

    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(row, CornerRadius::same(0), Palette::selection_bg());
        painter.line_segment(
            [
                Pos2::new(row.left() + 1.0, row.top()),
                Pos2::new(row.left() + 1.0, row.bottom()),
            ],
            Stroke::new(2.0, Palette::BRAND),
        );
    } else if response.hovered() {
        painter.rect_filled(row, CornerRadius::same(0), Palette::SURFACE_2);
    }

    let cy = row.center().y;
    // Status dot (right of the selection checkbox).
    painter.circle_filled(Pos2::new(row.left() + 30.0, cy), 3.5, dot_color(repo.dot));
    // Repo name.
    let name_galley = ui.painter().layout_no_wrap(
        repo.name.clone(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(row.left() + 44.0, cy - name_galley.size().y / 2.0),
        name_galley,
        Palette::INK,
    );
    // Right cluster: ahead/behind badges, then the branch label.
    let mut right = row.right() - 12.0;
    if repo.behind > 0 {
        let galley = painter.layout_no_wrap(
            format!("↓{}", repo.behind),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::STATE_WARNING,
        );
        right -= galley.size().x + 6.0;
        painter.galley_with_override_text_color(
            Pos2::new(right, cy - galley.size().y / 2.0),
            galley,
            Palette::STATE_WARNING,
        );
    }
    if repo.ahead > 0 {
        let galley = painter.layout_no_wrap(
            format!("↑{}", repo.ahead),
            FontId::new(11.0, FontFamily::Proportional),
            Palette::STATE_SUCCESS,
        );
        right -= galley.size().x + 6.0;
        painter.galley_with_override_text_color(
            Pos2::new(right, cy - galley.size().y / 2.0),
            galley,
            Palette::STATE_SUCCESS,
        );
    }
    let branch = repo
        .branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    let branch_galley = painter.layout_no_wrap(
        branch,
        FontId::new(11.0, FontFamily::Proportional),
        Palette::INK_3,
    );
    right -= branch_galley.size().x + 8.0;
    painter.galley_with_override_text_color(
        Pos2::new(right, cy - branch_galley.size().y / 2.0),
        branch_galley,
        Palette::INK_3,
    );
}

/// Build the workspace tree from the registered roots. `ahead_behind`
/// reads the root caches (may be absent for some roots → `(0, 0)`).
pub fn build_tree(
    project_dir: &Path,
    roots: &[Root],
    ahead_behind: &dyn Fn(&RootId) -> Option<(usize, usize)>,
) -> SidebarTree {
    // Group key: the root's first path component relative to the project
    // directory; a root at the project dir itself groups under the
    // project's own basename (single-repo projects still render a group).
    let mut groups: Vec<SidebarGroup> = Vec::new();
    for root in roots {
        let group_name = match root.path.strip_prefix(project_dir) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .unwrap_or_else(|| basename(project_dir)),
            _ => basename(project_dir),
        };
        let (ahead, behind) = ahead_behind(&root.id).unwrap_or((0, 0));
        let repo = SidebarRepo {
            id: root.id.clone(),
            name: basename(&root.path),
            path: root.path.clone(),
            branch: root.current_branch.clone(),
            dot: dot_state(root, ahead, behind),
            ahead,
            behind,
            conflicts: root.status.conflicted.len(),
            dirty_count: root.status.modified() + root.status.unversioned(),
        };
        match groups.iter_mut().find(|g| g.name == group_name) {
            Some(g) => g.repos.push(repo),
            None => groups.push(SidebarGroup {
                name: group_name,
                repos: vec![repo],
            }),
        }
    }
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    for g in &mut groups {
        g.repos.sort_by(|a, b| a.name.cmp(&b.name));
    }
    let total = roots.len();
    SidebarTree { groups, total }
}

/// File-name label of a path (the `<repo>` fallback matches the shell's).
fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<repo>")
        .to_string()
}

/// One row's dot state (issue #05): conflict wins, then divergence from
/// upstream, then uncommitted work, else clean.
fn dot_state(root: &Root, ahead: usize, behind: usize) -> DotState {
    if !root.status.conflicted.is_empty() {
        DotState::Conflict
    } else if ahead + behind > 0 {
        DotState::Diverged
    } else if root.status.modified() + root.status.unversioned() > 0 {
        DotState::Dirty
    } else {
        DotState::Clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_domain::model::RootStatus;

    fn root(path: &str, branch: Option<&str>) -> Root {
        Root {
            id: RootId(PathBuf::from(path).into()),
            path: PathBuf::from(path),
            remotes: vec![],
            branches: vec![],
            current_branch: branch.map(str::to_string),
            head: None,
            status: RootStatus::default(),
        }
    }

    #[test]
    fn tree_groups_roots_by_first_path_component_with_counts() {
        let project = Path::new("/w");
        let roots = vec![
            root("/w/frontend/app", Some("main")),
            root("/w/frontend/ui", Some("main")),
            root("/w/oss/lib", Some("dev")),
        ];
        let tree = build_tree(project, &roots, &|_| Some((0, 0)));

        assert_eq!(tree.total, 3, "workspace total counts every root");
        let group_names: Vec<&str> = tree.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(group_names, ["frontend", "oss"], "groups sort by name");
        let frontend = &tree.groups[0];
        assert_eq!(frontend.total(), 2);
        let repo_names: Vec<&str> = frontend.repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(repo_names, ["app", "ui"], "repos sort by name in group");
        assert_eq!(frontend.repos[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn root_at_project_dir_groups_under_project_basename() {
        let project = Path::new("/w/mono");
        let roots = vec![root("/w/mono", Some("main"))];
        let tree = build_tree(project, &roots, &|_| None);

        assert_eq!(tree.total, 1);
        assert_eq!(tree.groups.len(), 1);
        assert_eq!(tree.groups[0].name, "mono");
        assert_eq!(tree.groups[0].repos[0].name, "mono");
    }

    #[test]
    fn filter_matches_name_path_and_branch_preserving_groups() {
        let project = Path::new("/w");
        let roots = vec![
            root("/w/frontend/app", Some("main")),
            root("/w/frontend/ui", Some("release/2")),
            root("/w/oss/lib", Some("dev")),
        ];
        let tree = build_tree(project, &roots, &|_| Some((0, 0)));

        // Empty query: the tree comes back unchanged.
        let unfiltered = filter_tree(&tree, "");
        assert_eq!(unfiltered.total, 3);
        assert_eq!(unfiltered.groups.len(), 2);

        // By repo name.
        let by_name = filter_tree(&tree, "lib");
        assert_eq!(by_name.groups.len(), 1, "empty groups are dropped");
        assert_eq!(by_name.groups[0].name, "oss");
        assert_eq!(by_name.groups[0].repos.len(), 1);
        assert_eq!(by_name.total, 1, "top-level total narrows live");

        // By full path.
        let by_path = filter_tree(&tree, "/w/frontend/ui");
        assert_eq!(by_path.total, 1);
        assert_eq!(by_path.groups[0].repos[0].name, "ui");

        // By current branch, case-insensitively.
        let by_branch = filter_tree(&tree, "RELEASE");
        assert_eq!(by_branch.total, 1);
        assert_eq!(by_branch.groups[0].repos[0].name, "ui");

        // No match: every group drops out.
        assert!(filter_tree(&tree, "nothing-matches").groups.is_empty());
    }

    #[test]
    fn filter_matches_group_name_keeping_whole_group() {
        let project = Path::new("/w");
        let roots = vec![
            root("/w/frontend/app", Some("main")),
            root("/w/frontend/ui", Some("main")),
            root("/w/oss/lib", Some("dev")),
        ];
        let tree = build_tree(project, &roots, &|_| Some((0, 0)));

        // A group-name match keeps all of the group's repos so the group
        // structure stays navigable.
        let by_group = filter_tree(&tree, "frontend");
        assert_eq!(by_group.total, 2);
        assert_eq!(by_group.groups[0].repos.len(), 2);
    }

    #[test]
    fn dot_state_precedence_conflict_then_diverged_then_dirty_then_clean() {
        let project = Path::new("/w");
        let mut conflicted = root("/w/a", Some("main"));
        conflicted.status.conflicted = vec![PathBuf::from("f.txt")];
        let mut dirty = root("/w/b", Some("main"));
        dirty.status.changes = vec![turbogit_domain::model::Change {
            path: PathBuf::from("g.txt"),
            status: turbogit_domain::model::ChangeStatus::Modified,
            chunks: vec![],
            staged: false,
            unstaged: false,
            orig_path: None,
        }];
        let clean = root("/w/c", Some("main"));
        let diverged = root("/w/d", Some("main"));
        let roots = vec![conflicted, dirty, clean, diverged];

        // The diverged root is recognized purely from its ahead/behind
        // counts (the cache reader's job); the others report (0, 0).
        let tree = build_tree(project, &roots, &|id| {
            if id.0.as_os_str() == "/w/d" {
                Some((2, 1))
            } else {
                Some((0, 0))
            }
        });
        let dot_of = |name: &str| {
            tree.groups
                .iter()
                .flat_map(|g| &g.repos)
                .find(|r| r.name == name)
                .unwrap()
                .dot
        };
        assert_eq!(dot_of("a"), DotState::Conflict, "conflict outranks all");
        assert_eq!(dot_of("d"), DotState::Diverged, "diverged outranks dirty");
        assert_eq!(dot_of("b"), DotState::Dirty);
        assert_eq!(dot_of("c"), DotState::Clean);
    }

    #[test]
    fn group_dirty_count_conflicts_count_as_dirty() {
        let project = Path::new("/w");
        let mut conflicted = root("/w/g/a", Some("main"));
        conflicted.status.conflicted = vec![PathBuf::from("f.txt")];
        let clean = root("/w/g/b", Some("main"));
        let tree = build_tree(project, &[conflicted, clean], &|_| Some((0, 0)));

        let group = &tree.groups[0];
        assert_eq!(group.total(), 2);
        assert_eq!(group.dirty(), 1, "conflicted roots count as dirty");
    }

    #[test]
    fn ahead_behind_land_on_repo_rows_as_badge_counts() {
        let project = Path::new("/w");
        let roots = vec![root("/w/app", Some("main"))];
        let tree = build_tree(project, &roots, &|_| Some((3, 2)));

        let repo = &tree.groups[0].repos[0];
        assert_eq!((repo.ahead, repo.behind), (3, 2));
    }
}
