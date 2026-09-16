//! Workspace tree sidebar (issue #05, screen 01): the left rail listing
//! every discovered repository root as a recursive project tree
//! (sidebar-project-tree issue 01).
//!
//! The model is pure presentation logic living in [`super::project_tree`]:
//! folder and repo nodes, the collapse rule, path labels, and the
//! re-collapsing live / smart-group filters. This module paints that tree,
//! the smart-group rows, pinned views, and the selection bar; the shell
//! owns the left-rail geometry.

use std::path::PathBuf;

use super::project_tree::DotState;

// --- Rendering ---------------------------------------------------------------

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::multi_selection::CheckState;
use super::project_tree::{self, ProjectNode, iter_repos};
use super::smart_groups::{self, GroupFilter};
use super::tree_selection::{self};
use super::widgets;
use crate::theme::Palette;
use turbogit_app::root_caches::Affected;
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::{AppState, Dialog, Toast};

/// Left rail width (screen 01); also used by the shell's geometry.
pub const SIDEBAR_WIDTH: f32 = 280.0;

const ROW_HEIGHT: f32 = 26.0;
const GROUP_HEIGHT: f32 = 24.0;
/// Horizontal indent per tree depth (sidebar-project-tree issue 03).
const INDENT: f32 = 14.0;
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
    let full_tree = project_tree::build_tree(&state.project_dir, &state.multi.roots, &|id| {
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

    // Both filters narrow the recursive tree and re-collapse the survivors
    // with the same path labels (sidebar-project-tree issue 01), so no view
    // ever shows a folder with a single child.
    let tree = smart_groups::filter_to(
        &state.project_dir,
        &full_tree,
        active_group_filter(state).as_ref(),
    );
    let tree = project_tree::filter_tree(&state.project_dir, &tree, &state.ui.sidebar_filter);

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
    let project_dir = state.project_dir.clone();
    ScrollArea::vertical().show(&mut list, |ui| {
        for node in &tree.nodes {
            let root_path = match node {
                ProjectNode::Folder(_) => project_dir.clone(),
                ProjectNode::Repo(r) => r.path.clone(),
            };
            render_node(ui, state, node, 0, &root_path, &project_dir);
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
        render_selection_bar(&mut bar, state, iter_repos(&full_tree).count());
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
fn render_smart_groups(ui: &mut Ui, state: &mut AppState, tree: &project_tree::ProjectTree) {
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

/// The semantic token a group's dot paints with (issue 02): diverged and
/// conflicts keep the error red, unpushed the success green, while the
/// unpulled and dirty counters use the reserved counter orange.
fn smart_group_color(group: smart_groups::SmartGroup) -> Color32 {
    match group {
        smart_groups::SmartGroup::Diverged => Palette::STATUS_DIVERGED,
        smart_groups::SmartGroup::Conflicted => Palette::STATE_ERROR,
        smart_groups::SmartGroup::Unpushed => Palette::STATE_SUCCESS,
        smart_groups::SmartGroup::Unpulled => Palette::COUNTER,
        smart_groups::SmartGroup::Dirty => Palette::COUNTER,
    }
}

/// One user-defined rule's row (issue #07): the same dot + label + count
/// anatomy as the built-ins, in the info-blue family, clickable to filter
/// the tree to the rule's members. Rules render even at count zero —
/// they are persisted configuration, not computed views.
fn render_rule_group_row(
    ui: &mut Ui,
    state: &mut AppState,
    tree: &project_tree::ProjectTree,
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

/// One folder or repo node of the recursive project tree. `node_path` is
/// the node's real path (collapse state keys by its relative path); `depth`
/// indents children so the tree mirrors the directory structure.
fn render_node(
    ui: &mut Ui,
    state: &mut AppState,
    node: &ProjectNode,
    depth: usize,
    node_path: &std::path::Path,
    project_dir: &std::path::Path,
) {
    match node {
        ProjectNode::Folder(f) => render_folder_row(ui, state, f, depth, node_path, project_dir),
        ProjectNode::Repo(r) => render_repo_node(ui, state, r, depth, project_dir),
    }
}

/// One folder row: a tri-state selection checkbox, then a clickable header
/// (chevron + folder + name + subtree dirty badge + repo count) and, when
/// expanded, its children at the next depth. Collapse state keys by the
/// folder's relative path (sidebar-project-tree issue 03).
fn render_folder_row(
    ui: &mut Ui,
    state: &mut AppState,
    folder: &project_tree::FolderNode,
    depth: usize,
    node_path: &std::path::Path,
    project_dir: &std::path::Path,
) {
    let key = relative_key(node_path, project_dir);
    let expanded = !state.ui.sidebar_collapsed.contains(&key);
    let header = group_header_rect(ui);
    let response = ui.interact(
        header,
        ui.auto_id_with(("sidebar_folder", &key)),
        Sense::click(),
    );
    if response.clicked() {
        if expanded {
            state.ui.sidebar_collapsed.insert(key.clone());
        } else {
            state.ui.sidebar_collapsed.remove(&key);
        }
        state.persist_ui();
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, folder.name.as_str()));
    // The checkbox interacts after the header so it sits above it and
    // takes the click (the rule-row precedent).
    let checkbox = tri_state_checkbox(
        ui,
        checkbox_rect(header),
        ("sel_folder", &key),
        format!("Select group {}", folder.name),
        tree_selection::folder_state(&state.ui.repo_selection, folder),
    );
    if checkbox.clicked() {
        tree_selection::toggle_folder(&mut state.ui.repo_selection, folder);
    }

    let painter = ui.painter().clone();
    if response.hovered() {
        painter.rect_filled(header, CornerRadius::same(4), Palette::SURFACE_2);
    }
    let cy = header.center().y;
    let indent = depth as f32 * INDENT;
    let chevron = if expanded {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icon_at(
        ui,
        chevron,
        Pos2::new(header.left() + 28.0 + indent, cy),
        12.0,
        Palette::INK_3,
    );
    icon_at(
        ui,
        Icon::FOLDER,
        Pos2::new(header.left() + 46.0 + indent, cy),
        14.0,
        if expanded {
            Palette::BRAND
        } else {
            Palette::INK_2
        },
    );
    let name_galley = ui.painter().layout_no_wrap(
        folder.name.clone(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            header.left() + 60.0 + indent,
            cy - name_galley.size().y / 2.0,
        ),
        name_galley,
        Palette::INK,
    );
    // Right cluster: dirty count badge (when any), then the repo count.
    let mut right = header.right() - 12.0;
    if folder.dirty > 0 {
        let galley = ui.painter().layout_no_wrap(
            folder.dirty.to_string(),
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
        folder.total.to_string(),
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
        for child in &folder.children {
            let child_path = match child {
                ProjectNode::Folder(c) => node_path.join(&c.name),
                ProjectNode::Repo(c) => c.path.clone(),
            };
            render_node(ui, state, child, depth + 1, &child_path, project_dir);
        }
    }
}

/// One repository row: tri-state selection checkbox, status dot + label,
/// right-aligned ahead/behind badges and branch label. Clicking the row
/// focuses the repo everywhere; clicking the checkbox toggles selection
/// independently of any nested repos. A repo with nested repos renders an
/// expander (expanded by default) whose state keys by the repo's relative
/// path.
fn render_repo_node(
    ui: &mut Ui,
    state: &mut AppState,
    repo: &project_tree::RepoNode,
    depth: usize,
    project_dir: &std::path::Path,
) {
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
        tree_selection::toggle_repo(&mut state.ui.repo_selection, repo);
    }

    let has_children = !repo.children.is_empty();
    let rel = relative_key(&repo.path, project_dir);
    // A repo-with-children is expanded by default; the expander chevron
    // toggles only the collapse, the row itself keeps focusing the repo.
    let expanded = !has_children || !state.ui.sidebar_collapsed.contains(&rel);
    let indent = depth as f32 * INDENT;
    if has_children {
        let chevron_rect = Rect::from_center_size(
            Pos2::new(row.left() + 28.0 + indent, row.center().y),
            Vec2::splat(26.0),
        );
        let chevron = ui.interact(
            chevron_rect,
            ui.auto_id_with(("repo_expand", &repo.id)),
            Sense::click(),
        );
        chevron.widget_info(|| {
            WidgetInfo::labeled(
                WidgetType::Button,
                true,
                format!("Contract repo {}", repo.name),
            )
        });
        if chevron.clicked() {
            if expanded {
                state.ui.sidebar_collapsed.insert(rel.clone());
            } else {
                state.ui.sidebar_collapsed.remove(&rel);
            }
            state.persist_ui();
        }
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

    // A row with an expander shifts the dot and label right of the chevron;
    // a plain row keeps today's anatomy.
    let (dot_x, name_x) = if has_children {
        (44.0, 58.0)
    } else {
        (30.0, 44.0)
    };
    let cy = row.center().y;
    if has_children {
        icon_at(
            ui,
            if expanded {
                Icon::CHEVRON_DOWN
            } else {
                Icon::CHEVRON_RIGHT
            },
            Pos2::new(row.left() + 28.0 + indent, cy),
            12.0,
            Palette::INK_3,
        );
    }
    // Status dot (right of the selection checkbox).
    painter.circle_filled(
        Pos2::new(row.left() + dot_x + indent, cy),
        3.5,
        dot_color(repo.dot),
    );
    // Repo label: the path label when promoted, the bare name otherwise.
    let name_galley = ui.painter().layout_no_wrap(
        repo.label.clone(),
        FontId::new(12.5, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            row.left() + name_x + indent,
            cy - name_galley.size().y / 2.0,
        ),
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

    if has_children && expanded {
        for child in &repo.children {
            let child_path = match child {
                ProjectNode::Folder(c) => repo.path.join(&c.name),
                ProjectNode::Repo(c) => c.path.clone(),
            };
            render_node(ui, state, child, depth + 1, &child_path, project_dir);
        }
    }
}

/// Reserve one full-width row inside the scroll area and return its rect.
fn group_header_rect(ui: &mut Ui) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, GROUP_HEIGHT), Sense::hover());
    rect
}

/// The node's relative path under the project directory, normalized to
/// forward slashes so collapse keys are stable across platforms (and match
/// the persisted `ui.ron` keys on any OS).
fn relative_key(path: &std::path::Path, project_dir: &std::path::Path) -> String {
    path.strip_prefix(project_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_group_color_reserves_orange_for_unpulled_and_dirty() {
        // Issue 02 (design doc §6-7): the reserved counter orange paints the
        // unpulled + dirty smart-group dots exactly; every other group keeps
        // its semantic token — orange is never a general accent here.
        for group in smart_groups::SmartGroup::BUILTINS {
            let is_counter = matches!(
                group,
                smart_groups::SmartGroup::Unpulled | smart_groups::SmartGroup::Dirty
            );
            assert_eq!(
                smart_group_color(*group) == Palette::COUNTER,
                is_counter,
                "{group:?} must use COUNTER exactly when it is a dirty/unpulled counter"
            );
        }
        assert_eq!(
            smart_group_color(smart_groups::SmartGroup::Unpulled),
            Palette::COUNTER,
            "unpulled dot is the counter orange"
        );
        assert_eq!(
            smart_group_color(smart_groups::SmartGroup::Dirty),
            Palette::COUNTER,
            "dirty dot is the counter orange"
        );
        assert_eq!(
            smart_group_color(smart_groups::SmartGroup::Diverged),
            Palette::STATUS_DIVERGED,
            "diverged dot keeps the error red"
        );
        assert_eq!(
            smart_group_color(smart_groups::SmartGroup::Unpushed),
            Palette::STATE_SUCCESS,
            "unpushed dot keeps the success green"
        );
        assert_eq!(
            smart_group_color(smart_groups::SmartGroup::Conflicted),
            Palette::STATE_ERROR,
            "has-conflicts dot keeps the error red"
        );
    }
}
