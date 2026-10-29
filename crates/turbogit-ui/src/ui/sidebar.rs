use std::path::PathBuf;

use super::project_tree::DotState;

use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};

use super::components;
use super::icons::{self, Icon};
use super::multi_selection::CheckState;
use super::project_tree::{self, ProjectNode, iter_repos};
use super::smart_groups::{self, GroupFilter};
use super::tree_selection::{self};
use super::widgets;
use crate::theme::{Palette, two_space_indent};
use turbogit_app::operation::Operation;
use turbogit_app::shell_writes;
use turbogit_app::smart_rules::SmartGroupRule;
use turbogit_app::state::{AppState, Dialog, Toast};
use turbogit_domain::model::{DisplayName, forward_slash_path};

pub const SIDEBAR_WIDTH: f32 = 280.0;

const RULE_BUTTONS_ZONE: f32 = 56.0;

fn paint_active_band(painter: &egui::Painter, row: Rect) {
    painter.rect_filled(
        row,
        CornerRadius::ZERO,
        components::row_fill(components::RowState::FocusSelected),
    );
    components::paint_rail(painter, row);
}
const SELECTION_BAR_HEIGHT: f32 = 36.0;

const EXPANDER_X: f32 = 28.0;
const FOLDER_ICON_X: f32 = 42.0;
const FOLDER_NAME_X: f32 = 54.0;
const REPO_NAME_X: f32 = 44.0;

const REPO_STATE_DOT_X: f32 = 37.5;
const REPO_STATE_DOT_R: f32 = 3.5;

fn checkbox_rect(row: Rect) -> Rect {
    Rect::from_center_size(
        Pos2::new(row.left() + 15.0, row.center().y),
        Vec2::splat(13.0),
    )
}

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
            painter.rect_filled(
                rect,
                CornerRadius::same(crate::theme::CHIP_RADIUS),
                Palette::BRAND,
            );
            icons::centered_icon(ui, Icon::CHECK, rect.center(), 10.0, Palette::BRAND_INK);
        }
        CheckState::Partial => {
            painter.rect_filled(
                rect,
                CornerRadius::same(crate::theme::CHIP_RADIUS),
                Palette::BRAND,
            );
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
            painter.rect_filled(rect, CornerRadius::same(crate::theme::CHIP_RADIUS), fill);
            painter.rect_stroke(
                rect,
                CornerRadius::same(crate::theme::CHIP_RADIUS),
                Stroke::new(1.0, Palette::LINE),
                egui::StrokeKind::Inside,
            );
        }
    }
    response
}

fn render_pinned_views(ui: &mut Ui, state: &mut AppState) {
    if state.ui.pinned_views.is_empty() {
        return;
    }
    inset_column(ui, |ui| {
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
    });
}

fn render_selection_bar(ui: &mut Ui, state: &mut AppState, total: usize) {
    let rect = ui.max_rect();
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
    widgets::edge_rule(
        ui.painter(),
        rect,
        widgets::Edge::Top,
        Palette::RULE_STRUCTURAL,
    );

    let cy = rect.center().y;
    let count = format!("{} selected / {}", state.ui.repo_selection.len(), total);
    let count_galley = painter.layout_no_wrap(
        count,
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
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

fn fetch_selection(state: &mut AppState) {
    let roots: Vec<_> = state.ui.repo_selection.iter().cloned().collect();
    if roots.is_empty() {
        return;
    }
    state.dispatch(Operation::Fetch { roots });
}

fn pull_selection(state: &mut AppState) {
    let paths: Vec<PathBuf> = state
        .ui
        .repo_selection
        .iter()
        .map(|id| id.0.to_path_buf())
        .collect();
    if paths.is_empty() {
        return;
    }
    state.dispatch(shell_writes::pull::selection(paths));
}

pub fn dot_color(dot: DotState) -> Color32 {
    dot.color()
}

fn count_chip_at(painter: &egui::Painter, right_edge: f32, cy: f32, text: &str) -> f32 {
    let geometry = widgets::COMPACT_CHIP_GEOMETRY;
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        crate::theme::data_font(crate::theme::TYPE_CHIP),
        widgets::COUNT_CHIP_COLORS.fg,
    );
    let size = geometry.size(&galley);
    geometry.paint(
        painter,
        geometry.rect_right(right_edge, cy, &galley),
        galley,
        widgets::COUNT_CHIP_COLORS.bg,
        widgets::COUNT_CHIP_COLORS.fg,
    );
    size.x
}

const BADGE_GAP: f32 = 6.0;
pub const RAIL_INSET: f32 = 12.0;

fn inset_column<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rail = ui.max_rect();
    let column = Rect::from_min_max(
        Pos2::new(ui.cursor().left() + RAIL_INSET, ui.cursor().top()),
        Pos2::new(rail.right() - RAIL_INSET, rail.bottom()),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(column));
    let out = add(&mut child);
    ui.advance_cursor_after_rect(child.min_rect());
    out
}

/// Render the workspace tree into `ui`, whose max rect is the shell's
/// reserved left rail (see `shell::render`).
pub fn show(ui: &mut Ui, state: &mut AppState) {
    let rect = ui.max_rect();
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::SIDEBAR);
    widgets::edge_rule(ui.painter(), rect, widgets::Edge::Right, Palette::LINE);

    let mut col = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    col.add_space(8.0);
    render_workspace_header(&mut col, state);
    col.add_space(10.0);
    inset_column(&mut col, |ui| {
        widgets::search_input(ui, "Filter repos / branches…", &mut state.ui.sidebar_filter);
    });
    col.add_space(12.0);

    let full_tree = project_tree::build_tree(&state.project_dir, &state.multi.roots, &|id| {
        state.caches.ahead_behind(id)
    });
    render_smart_groups(&mut col, state, &full_tree);
    col.add_space(12.0);
    render_pinned_views(&mut col, state);
    col.add_space(12.0);

    widgets::pane_header_inset(&mut col, "PROJECTS", None, RAIL_INSET, |_ui| {});

    let tree = smart_groups::filter_to(
        &state.project_dir,
        &full_tree,
        active_group_filter(state).as_ref(),
    );
    let tree = project_tree::filter_tree(&state.project_dir, &tree, &state.ui.sidebar_filter);

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
    ScrollArea::vertical()
        .id_salt("sidebar_project_tree")
        .show(&mut list, |ui| {
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

const WORKSPACE_SWITCH_HINT: &str = "Switch workspace";

fn render_workspace_header(ui: &mut Ui, state: &mut AppState) {
    let name = state.project_dir.display_name_or("<workspace>");
    let total = state.multi.roots.len().to_string();
    let rail_width = ui.available_width();
    let chevron = 12.0;
    let mut chevron_slot = None;
    let painted = ui.horizontal(|ui| {
        ui.add_space(12.0);
        icons::icon(ui, Icon::FOLDER_GIT, 14.0, Palette::BRAND);
        ui.add_space(6.0);
        ui.label(
            RichText::new(&name)
                .strong()
                .font(crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE))
                .color(Palette::INK),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            let count_galley = ui.painter().layout_no_wrap(
                total,
                crate::theme::data_font(crate::theme::TYPE_CHIP),
                widgets::COUNT_CHIP_COLORS.fg,
            );
            let count_w = widgets::COMPACT_CHIP_GEOMETRY.size(&count_galley).x;
            let (chip, _) = ui.allocate_exact_size(
                Vec2::new(count_w, widgets::COMPACT_CHIP_GEOMETRY.height),
                Sense::hover(),
            );
            widgets::COMPACT_CHIP_GEOMETRY.paint(
                ui.painter(),
                chip,
                count_galley,
                widgets::COUNT_CHIP_COLORS.bg,
                widgets::COUNT_CHIP_COLORS.fg,
            );
            let (slot, _) = ui.allocate_exact_size(Vec2::splat(chevron), Sense::hover());
            chevron_slot = Some(slot);
        });
    });
    // `ui.horizontal` shrinks to its content, so widen that rect back out to
    // the rail: the hit target is the whole band, not the glyph cluster.
    let content = painted.response.rect;
    let row = Rect::from_min_max(
        Pos2::new(content.left(), content.top()),
        Pos2::new(content.left() + rail_width, content.bottom()),
    );
    let response = ui
        .interact(row, ui.id().with("workspace_header"), Sense::click())
        .on_hover_text(WORKSPACE_SWITCH_HINT);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, WORKSPACE_SWITCH_HINT));
    let hovered = response.hovered();
    if hovered {
        // egui only sets the pointer for its own buttons; a `Sense::click()`
        // row has to ask for the hand itself.
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if let Some(slot) = chevron_slot {
        icons::paint_icon(
            ui.painter(),
            slot.min,
            chevron,
            Icon::CHEVRON_DOWN,
            if hovered {
                Palette::INK_2
            } else {
                Palette::INK_3
            },
        );
    }
    if response.clicked() {
        state.ui.workspace_picker_open = true;
        state.ui.workspace_picker_anchor =
            Some((row.left(), row.bottom() + crate::theme::ITEM_SPACING.y));
    }
}

fn render_smart_groups(ui: &mut Ui, state: &mut AppState, tree: &project_tree::ProjectTree) {
    widgets::pane_header_inset(ui, "SMART GROUPS", None, RAIL_INSET, |ui| {
        ui.add_space(RAIL_INSET);
        let plus = ui.add(egui::Button::new(RichText::new("+").color(Palette::INK_3)));
        plus.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "New rule"));
        if plus.clicked() {
            state.ui.smart_rule_editor_open = true;
            state.ui.smart_rule_editing = None;
            state.ui.smart_rule_draft = None;
        }
    });
    for entry in smart_groups::smart_groups(tree) {
        render_smart_group_row(ui, state, &entry);
    }
    for (ix, rule) in state.ui.smart_group_rules.clone().iter().enumerate() {
        render_rule_group_row(ui, state, tree, ix, rule);
    }
}

fn render_smart_group_row(
    ui: &mut Ui,
    state: &mut AppState,
    entry: &smart_groups::SmartGroupEntry,
) {
    let width = ui.available_width();
    let row = Rect::from_min_size(
        Pos2::new(ui.cursor().left(), ui.cursor().top()),
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
    );
    ui.allocate_exact_size(
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
        Sense::hover(),
    );
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
        paint_active_band(&painter, row);
    } else if response.hovered() {
        painter.rect_filled(
            row,
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            components::row_fill(components::RowState::Hover),
        );
    }

    let cy = row.center().y;
    painter.circle_filled(
        Pos2::new(row.left() + 24.0, cy),
        3.5,
        smart_group_color(entry.group),
    );
    let label_galley = ui.painter().layout_no_wrap(
        entry.group.label().to_string(),
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(row.left() + 38.0, cy - label_galley.size().y / 2.0),
        label_galley,
        Palette::INK,
    );
    count_chip_at(
        &painter,
        row.right() - RAIL_INSET,
        cy,
        &entry.count.to_string(),
    );
}

fn smart_group_color(group: smart_groups::SmartGroup) -> Color32 {
    dot_color(group.state())
}

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
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
    );
    ui.allocate_exact_size(
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
        Sense::hover(),
    );
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
        paint_active_band(&painter, row);
    } else if response.hovered() {
        painter.rect_filled(
            row,
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            components::row_fill(components::RowState::Hover),
        );
    }

    let cy = row.center().y;
    painter.circle_filled(Pos2::new(row.left() + 24.0, cy), 3.5, Palette::STATE_INFO);
    let label_galley = ui.painter().layout_no_wrap(
        rule.label.clone(),
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(row.left() + 38.0, cy - label_galley.size().y / 2.0),
        label_galley,
        Palette::INK,
    );
    let count = smart_groups::rule_count(tree, rule);
    count_chip_at(
        &painter,
        row.right() - RULE_BUTTONS_ZONE,
        cy,
        &count.to_string(),
    );

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

fn delete_rule(state: &mut AppState, ix: usize) {
    if let Some(rule) = state.ui.smart_group_rules.get(ix)
        && state.ui.sidebar_smart_group.as_deref() == Some(rule.label.as_str())
    {
        state.ui.sidebar_smart_group = None;
    }
    state.ui.smart_group_rules.remove(ix);
    state.save_durable_ui();
    state.ui.toast = Some(Toast::info("Smart group rule deleted"));
}

fn is_smart_group_active(state: &AppState, label: &str) -> bool {
    state.ui.sidebar_smart_group.as_deref() == Some(label)
}

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
        state.save_durable_ui();
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, folder.name.as_str()));
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
        painter.rect_filled(
            header,
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            Palette::SURFACE_2,
        );
    }
    let cy = header.center().y;
    let indent = depth as f32 * two_space_indent(ui);
    let chevron = if expanded {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icons::centered_icon(
        ui,
        chevron,
        Pos2::new(header.left() + EXPANDER_X + indent, cy),
        12.0,
        Palette::INK_3,
    );
    icons::centered_icon(
        ui,
        Icon::FOLDER,
        Pos2::new(header.left() + FOLDER_ICON_X + indent, cy),
        14.0,
        if expanded {
            Palette::BRAND
        } else {
            Palette::INK_2
        },
    );
    let name_galley = ui.painter().layout_no_wrap(
        folder.name.clone(),
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        Palette::INK,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            header.left() + FOLDER_NAME_X + indent,
            cy - name_galley.size().y / 2.0,
        ),
        name_galley,
        Palette::INK,
    );
    let mut right = header.right() - RAIL_INSET;
    if folder.dirty > 0 {
        let galley = ui.painter().layout_no_wrap(
            folder.dirty.to_string(),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            dot_color(DotState::Dirty),
        );
        right -= galley.size().x + BADGE_GAP;
        painter.galley_with_override_text_color(
            Pos2::new(right, cy - galley.size().y / 2.0),
            galley,
            dot_color(DotState::Dirty),
        );
    }
    count_chip_at(&painter, right - BADGE_GAP, cy, &folder.total.to_string());

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
        Vec2::new(width, crate::theme::GROUP_ROW_HEIGHT),
    );
    ui.allocate_exact_size(
        Vec2::new(width, crate::theme::GROUP_ROW_HEIGHT),
        Sense::hover(),
    );
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
    let expanded = !has_children || !state.ui.sidebar_collapsed.contains(&rel);
    let indent = depth as f32 * two_space_indent(ui);
    if has_children {
        let chevron_rect = Rect::from_center_size(
            Pos2::new(row.left() + EXPANDER_X + indent, row.center().y),
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
            state.save_durable_ui();
        }
    }

    let painter = ui.painter().with_clip_rect(row.intersect(ui.clip_rect()));
    if selected {
        paint_active_band(&painter, row);
    } else if response.hovered() {
        painter.rect_filled(
            row,
            CornerRadius::ZERO,
            components::row_fill(components::RowState::Hover),
        );
    }

    let cy = row.center().y;
    if has_children {
        icons::centered_icon(
            ui,
            if expanded {
                Icon::CHEVRON_DOWN
            } else {
                Icon::CHEVRON_RIGHT
            },
            Pos2::new(row.left() + EXPANDER_X + indent, cy),
            12.0,
            Palette::INK_3,
        );
    }
    painter.circle_filled(
        Pos2::new(row.left() + REPO_STATE_DOT_X, cy),
        REPO_STATE_DOT_R,
        dot_color(repo.dot),
    );
    let left = (row.left() + REPO_NAME_X + indent).min(row.right());
    let mut right = row.right() - RAIL_INSET;
    let badge_left = left + ((right - left).max(0.0) * 0.52).max(24.0);
    if repo.behind > 0 {
        let ink = dot_color(DotState::Unpulled);
        let galley = painter.layout_no_wrap(
            format!("↓{}", repo.behind),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            ink,
        );
        if right - galley.size().x - BADGE_GAP >= badge_left {
            right -= galley.size().x + BADGE_GAP;
            painter.galley_with_override_text_color(
                Pos2::new(right, cy - galley.size().y / 2.0),
                galley,
                ink,
            );
        }
    }
    if repo.ahead > 0 {
        let ink = dot_color(DotState::Unpushed);
        let galley = painter.layout_no_wrap(
            format!("↑{}", repo.ahead),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            ink,
        );
        if right - galley.size().x - BADGE_GAP >= badge_left {
            right -= galley.size().x + BADGE_GAP;
            painter.galley_with_override_text_color(
                Pos2::new(right, cy - galley.size().y / 2.0),
                galley,
                ink,
            );
        }
    }
    let branch = repo
        .branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    let available = (right - left - 8.0).max(0.0);
    let name_width = available * 0.52;
    let branch_width = (available - name_width - 8.0).max(0.0);
    paint_row_label(
        ui,
        row,
        &repo.label,
        left,
        name_width,
        crate::theme::TYPE_BODY,
        Palette::INK,
    );
    paint_row_label(
        ui,
        row,
        &branch,
        left + name_width + 8.0,
        branch_width,
        crate::theme::TYPE_CONTROL,
        Palette::INK_3,
    );
    response.on_hover_text(format!(
        "{}\n{branch}\n↑{} ↓{}",
        repo.path.display(),
        repo.ahead,
        repo.behind
    ));

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

#[allow(clippy::too_many_arguments)]
fn paint_row_label(ui: &Ui, row: Rect, text: &str, x: f32, width: f32, size: f32, ink: Color32) {
    let font = crate::theme::chrome_font(size);
    let label = super::components::middle_truncate_to_width(ui, text, &font, width);
    let galley = ui.painter().layout_no_wrap(label, font, ink);
    let column = Rect::from_min_size(
        Pos2::new(x, row.top()),
        Vec2::new(width.max(0.0), row.height()),
    );
    ui.painter()
        .with_clip_rect(row.intersect(column).intersect(ui.clip_rect()))
        .galley(
            Pos2::new(x, row.center().y - galley.size().y / 2.0),
            galley,
            ink,
        );
}

fn group_header_rect(ui: &mut Ui) -> egui::Rect {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
        Sense::hover(),
    );
    rect
}

/// The node's relative path under the project directory, normalized to
/// forward slashes so collapse keys are stable across platforms (and match
/// the persisted `ui.ron` keys on any OS).
///
/// Two jobs, and only the second one is shared: the `strip_prefix` is what
/// makes a key *relative* and what answers `""` for a path outside the
/// project, and [`forward_slash_path`] is only the separator rewrite. Folding
/// this into that helper would drop the strip and lose a user's collapsed tree.
fn relative_key(path: &std::path::Path, project_dir: &std::path::Path) -> String {
    path.strip_prefix(project_dir)
        .map(forward_slash_path)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_unicode_repo_rows_keep_labels_and_badges_in_separate_columns() {
        let project = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let state = AppState::launch_in(Some(project.path().into()), Some(config.path().into()));
        let repo = project_tree::RepoNode {
            id: turbogit_domain::model::RootId(project.path().join("repo").into()),
            name: "服務端-very-long-repository-équipe".into(),
            label: "服務端-very-long-repository-équipe".into(),
            path: project.path().join("repo"),
            branch: Some("feature/修復-équipe-very-long-branch-終点".into()),
            dot: DotState::Diverged,
            ahead: usize::MAX,
            behind: usize::MAX,
            conflicts: 0,
            dirty_count: 0,
            children: vec![],
        };
        let mut harness = egui_kittest::Harness::new_ui_state(
            move |ui, state| {
                crate::theme::configure_style(ui.ctx());
                render_repo_node(ui, state, &repo, 0, &repo.path);
            },
            state,
        );
        crate::theme::install_fonts(&harness.ctx);
        for width in [320.0, 208.0, 120.0, 80.0] {
            harness.set_size(egui::vec2(width, 100.0));
            harness.run();
            let mut text_rects = Vec::new();
            for shape in &harness.output().shapes {
                if let egui::Shape::Text(text) = &shape.shape {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    assert!(rect.left() >= 44.0, "label/badge crowds controls: {rect:?}");
                    assert!(rect.right() <= width, "text escapes row: {rect:?}");
                    assert!(shape.clip_rect.height() <= crate::theme::GROUP_ROW_HEIGHT);
                    for previous in &text_rects {
                        assert!(
                            !rect.intersects(*previous),
                            "labels overlap: {rect:?} / {previous:?}"
                        );
                    }
                    text_rects.push(rect);
                }
            }
            assert!(!text_rects.is_empty(), "keep the repository identifiable");
        }
    }

    #[test]
    fn smart_group_color_reserves_orange_for_unpulled_and_dirty() {
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
            DotState::Unpushed.color(),
            "unpushed dot shares the repo and badge green"
        );
        assert_eq!(
            smart_group_color(smart_groups::SmartGroup::Conflicted),
            Palette::STATE_ERROR,
            "has-conflicts dot keeps the error red"
        );
    }
}
