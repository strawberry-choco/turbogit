use crate::theme::Palette;
use crate::ui::changes_selection::{self, ClickKind, RowSlot};
use crate::ui::column_table::{self, PaneTable};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use crate::ui::widgets::PaneColumn;
use crate::ui::widgets::TabItem;
use egui::containers::menu::{MenuConfig, SubMenuButton};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Key, Layout, Margin, PopupCloseBehavior,
    Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::path::{Path, PathBuf};
use turbogit_app::root_caches::Affected;
use turbogit_app::shell_writes;
use turbogit_app::state::{AppState, CommitSubTab, Dialog, PendingConfirm, Toast};
use turbogit_domain::model::{Change, ChangeRef, ChangeStatus, DisplayName, Root, RootId};

pub const COMMIT_PANEL_WIDTH: f32 = 340.0;

const CHANGES: &str = "Changes";

pub const MERGE_CONFLICTS: &str = "Merge conflicts";

struct Bucket<'a> {
    name: &'static str,
    root: &'a Root,
    changes: Vec<&'a Change>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StageState {
    None,
    Partial,
    All,
}

impl StageState {
    fn of(c: &Change) -> Self {
        match (c.staged, c.unstaged) {
            (true, false) => Self::All,
            (true, true) => Self::Partial,
            _ => Self::None,
        }
    }
}

enum RowAction {
    ToggleStage {
        root: RootId,
        path: PathBuf,
    },
    ToggleGroup(turbogit_domain::model::RootId),
    Select {
        key: PathBuf,
        root: RootId,
        path: PathBuf,
        kind: ClickKind,
    },
}

fn change_buckets(state: &AppState) -> Vec<Bucket<'_>> {
    let mut out = Vec::new();
    for root in &state.multi.roots {
        let mut files = Vec::new();
        let mut conflicts = Vec::new();
        for c in &root.status.changes {
            match c.status {
                ChangeStatus::Conflicted => conflicts.push(c),
                ChangeStatus::Ignored => {}
                _ => files.push(c),
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
        for (name, changes) in [(CHANGES, files), (MERGE_CONFLICTS, conflicts)] {
            if !changes.is_empty() {
                out.push(Bucket {
                    name,
                    root,
                    changes,
                });
            }
        }
    }
    out
}

fn apply_actions(state: &mut AppState, actions: Vec<RowAction>, order: &[RowSlot]) {
    for action in actions {
        match action {
            RowAction::ToggleStage { root, path } => set_file_staged(state, &root, &path),
            RowAction::Select {
                key,
                root,
                path,
                kind,
            } => {
                changes_selection::apply(
                    &mut state.ui.selected,
                    &mut state.ui.selection_anchor,
                    order,
                    &key,
                    kind,
                );
                state.ui.preview_change = Some(ChangeRef { root, path });
            }
            RowAction::ToggleGroup(id) => {
                if state.ui.changes_expanded.contains(&id) {
                    state.ui.changes_expanded.remove(&id);
                } else {
                    state.ui.changes_expanded.insert(id);
                }
            }
        }
    }
}

fn set_file_staged(state: &mut AppState, root: &RootId, path: &Path) {
    let Some(change) = state.multi.by_id(root).and_then(|r| r.resolve_change(path)) else {
        return;
    };
    let stage = StageState::of(change) != StageState::All;
    state.dispatch(shell_writes::commit::set_staged(root, path, stage));
}

pub fn toggle_previewed_row_checkbox(state: &mut AppState) {
    let Some(change) = state.ui.preview_change.clone() else {
        return;
    };
    if state
        .multi
        .by_id(&change.root)
        .and_then(|r| r.resolve_change(&change.path))
        .is_some_and(|c| c.status == ChangeStatus::Conflicted)
    {
        return;
    }
    set_file_staged(state, &change.root, &change.path);
}

fn selected_changes(state: &AppState) -> Vec<Change> {
    let mut out = Vec::new();
    if let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
    {
        for c in &root.status.changes {
            if state.ui.selected.contains(&root.canonical_key(c)) {
                out.push(c.clone());
            }
        }
    }
    out
}

pub fn show(ui: &mut Ui, state: &mut AppState) {
    if let Some(root_id) = state.selected_root.clone() {
        state.ensure_hunk_stats(&root_id);
    }
    if let Some(root_id) = state.ui.preview_change.as_ref().map(|c| c.root.clone()) {
        state.ensure_hunk_stats(&root_id);
    }
    let active = state.ui.commit_subtab;
    widgets::tab_view(
        ui,
        state,
        &SUB_TAB_ITEMS,
        active,
        |state, tab| state.ui.commit_subtab = tab,
        |_, _| {},
        |ui, state| match state.ui.commit_subtab {
            CommitSubTab::LocalChanges => local_changes_body(ui, state),
            CommitSubTab::Shelf => shelf_pane(ui, state),
            CommitSubTab::Stash => stash_pane(ui, state),
        },
    );

    crate::ui::conflicts::render(ui, state);

    if let Some(err) = &state.last_error {
        ui.separator();
        widgets::inline_error(ui, format!("⚠ {err}"));
    }
}

const SUB_TAB_ITEMS: [TabItem<'static, CommitSubTab>; 3] = [
    TabItem {
        id: CommitSubTab::LocalChanges,
        title: "Local Changes",
        icon: None,
        end_icon: None,
        badge: None,
    },
    TabItem {
        id: CommitSubTab::Shelf,
        title: "Shelf",
        icon: None,
        end_icon: None,
        badge: None,
    },
    TabItem {
        id: CommitSubTab::Stash,
        title: "Stash",
        icon: None,
        end_icon: None,
        badge: None,
    },
];

fn file_filter_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let resp = widgets::search_input(ui, "Filter files", &mut state.ui.file_filter);
        if state.ui.focus_file_filter {
            resp.request_focus();
            state.ui.focus_file_filter = false;
        }
        // egui surrenders focus on bare Escape; pair that transition with
        // clearing the query so Esc means "filter off".
        if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            state.ui.file_filter.clear();
        }
    });
}

fn filter_buckets<'a>(state: &AppState, mut buckets: Vec<Bucket<'a>>) -> (Vec<Bucket<'a>>, String) {
    let query = state.ui.file_filter.trim().to_lowercase();
    if query.is_empty() {
        return (buckets, String::new());
    }
    for bucket in &mut buckets {
        bucket
            .changes
            .retain(|c| widgets::filter_matches(&c.path.display().to_string(), &query));
    }
    buckets.retain(|b| !b.changes.is_empty());
    let shown = state.ui.file_filter.trim();
    (buckets, format!("No files match '{shown}'."))
}

fn local_changes_body(ui: &mut Ui, state: &mut AppState) {
    two_zone(ui, state, changelist_pane, preview_and_editor_pane);
}

fn two_zone(
    ui: &mut Ui,
    state: &mut AppState,
    left: impl FnOnce(&mut Ui, &mut AppState),
    right: impl FnOnce(&mut Ui, &mut AppState),
) {
    let avail = ui.available_rect_before_wrap();
    let left_rect = Rect::from_min_max(
        avail.min,
        Pos2::new(avail.min.x + COMMIT_PANEL_WIDTH, avail.max.y),
    );
    let right_rect = Rect::from_min_max(
        Pos2::new(left_rect.max.x + crate::theme::PANEL_PADDING, avail.min.y),
        avail.max,
    );
    let mut left_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    left(&mut left_ui, state);
    let mut right_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    right(&mut right_ui, state);
    let max_height = left_ui.min_size().y.max(right_ui.min_size().y);
    ui.advance_cursor_after_rect(Rect::from_min_size(
        avail.min,
        Vec2::new(avail.width(), max_height),
    ));
}

const SHELF_COLUMNS: [PaneColumn; 4] = [
    PaneColumn::start("SHELF", 8.0),
    PaneColumn::start("FILES", 320.0),
    PaneColumn::start("SHELVED", 392.0),
    PaneColumn::end("ACTIONS", 0.0),
];

const STASH_COLUMNS: [PaneColumn; 3] = [
    PaneColumn::start("STASH", 8.0),
    PaneColumn::start("MESSAGE", 112.0),
    PaneColumn::end("ACTIONS", 0.0),
];

fn shelf_pane(ui: &mut Ui, state: &mut AppState) {
    column_table::column_table_pane(
        ui,
        state,
        PaneTable {
            title: "SHELVES",
            columns: &SHELF_COLUMNS,
            loading: "No shelf store for this project yet.",
            empty: "Nothing is shelved in this repository. Use “Shelve…” to park the selected \
                    changes and take the working tree back to clean.",
            cached: |state, id| Some(state.shelves_for(id)),
            actions: &mut |ui, state| {
                if widgets::ghost_button(ui, None, "Shelve\u{2026}")
                    .on_hover_text("Shelve selected changes")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::Shelve);
                }
                if widgets::ghost_button(ui, None, "Import Patches\u{2026}")
                    .on_hover_text("Park a patch file on this repository's shelf")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::ImportPatches);
                }
                if widgets::ghost_button(ui, None, "Unshelve\u{2026}")
                    .on_hover_text("Unshelve a shelf from this repository")
                    .clicked()
                {
                    state.open_unshelve_dialog();
                }
            },
            row: &mut shelf_row,
        },
    );
}

fn stash_pane(ui: &mut Ui, state: &mut AppState) {
    let cold = state
        .selected_root
        .clone()
        .filter(|id| state.caches.stashes(id).is_none());
    if let Some(id) = cold {
        state.fetch_stashes(id);
    }
    column_table::column_table_pane(
        ui,
        state,
        PaneTable {
            title: "STASHES",
            columns: &STASH_COLUMNS,
            loading: "Loading stashes\u{2026}",
            empty: "Nothing is stashed in this repository. Use “Stash…” to park every uncommitted \
                    change, in or out of this application.",
            cached: |state, id| state.caches.stashes(id).map(|rows| rows.to_vec()),
            actions: &mut |ui, state| {
                if widgets::ghost_button(ui, None, "Stash\u{2026}")
                    .on_hover_text("Stash all changes")
                    .clicked()
                {
                    state.ui.dialog = Some(Dialog::Stash);
                }
            },
            row: &mut stash_row,
        },
    );
}

fn shelf_row(ui: &mut Ui, state: &mut AppState, shelf: &turbogit_domain::model::Shelf) {
    let root = shelf.root.clone();
    let name = shelf.name.clone();
    let files = components::counted(shelf.changes.len(), "file", "files");
    let shelved = shelf.created_at.format("%Y-%m-%d %H:%M").to_string();
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &SHELF_COLUMNS, 0, |ui| {
            ui.label(RichText::new(&shelf.name).color(Palette::INK));
        });
        widgets::column_cell(ui, &SHELF_COLUMNS, 1, |ui| {
            ui.label(RichText::new(files).color(Palette::INK_2));
        });
        widgets::column_cell(ui, &SHELF_COLUMNS, 2, |ui| {
            ui.label(RichText::new(shelved).color(Palette::INK_2));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if components::kit_button(ui, components::KitButton::Quiet, "Delete Shelf").clicked() {
                state.ui.confirm = Some(PendingConfirm::DeleteShelf {
                    root: root.clone(),
                    name: name.clone(),
                });
            }
            if components::kit_button(ui, components::KitButton::Danger, "Unshelve and Remove")
                .clicked()
            {
                state.unshelve(root.clone(), name.clone(), true);
            }
            if components::kit_button(ui, components::KitButton::Secondary, "Unshelve").clicked() {
                state.unshelve(root.clone(), name.clone(), false);
            }
        });
    });
}

fn stash_row(ui: &mut Ui, state: &mut AppState, stash: &turbogit_domain::model::Stash) {
    let root = stash.root.clone();
    let index = stash.index;
    let stash_ref = turbogit_app::shelve_stash::stash_ref(index);
    ui.horizontal(|ui| {
        widgets::column_cell(ui, &STASH_COLUMNS, 0, |ui| {
            ui.label(RichText::new(&stash_ref).color(Palette::INK));
        });
        widgets::column_cell(ui, &STASH_COLUMNS, 1, |ui| {
            ui.label(RichText::new(&stash.message).color(Palette::INK_2));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if components::kit_button(ui, components::KitButton::Quiet, "Delete Stash").clicked() {
                state.ui.confirm = Some(PendingConfirm::DeleteStash {
                    root: root.clone(),
                    index,
                });
            }
            if components::kit_button(ui, components::KitButton::Danger, "Pop Stash (removes)")
                .clicked()
            {
                state.dispatch(shell_writes::stash::pop_at(&root, &stash_ref, index));
            }
            if components::kit_button(ui, components::KitButton::Secondary, "Apply Stash (keeps)")
                .clicked()
            {
                state.dispatch(shell_writes::stash::apply_at(&root, &stash_ref, index));
            }
        });
    });
}

fn changelist_pane(ui: &mut Ui, state: &mut AppState) {
    let conflicted = state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .is_some_and(|r| !r.status.conflicted.is_empty());
    if conflicted {
        widgets::card(ui, widgets::CardFrame::default(), |ui| {
            changes_card_body(ui, state, false);
        });
        ui.add_space(crate::theme::PANEL_PADDING - crate::theme::ITEM_SPACING.y);
        widgets::card(ui, widgets::CardFrame::default(), |ui| {
            commit_card_body(ui, state);
        });
        return;
    }

    let avail = ui.available_rect_before_wrap();
    let dock_height_id = egui::Id::new("commit_card_dock_height");
    let dock_height = ui
        .ctx()
        .memory(|m| m.data.get_temp::<f32>(dock_height_id))
        .unwrap_or(0.0);
    let dock_rect = Rect::from_min_max(
        Pos2::new(avail.min.x, (avail.max.y - dock_height).max(avail.min.y)),
        avail.max,
    );
    let changes_rect = Rect::from_min_max(
        avail.min,
        Pos2::new(
            avail.max.x,
            (dock_rect.min.y - crate::theme::PANEL_PADDING).max(avail.min.y),
        ),
    );
    let mut changes_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(changes_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    widgets::card(&mut changes_ui, widgets::CardFrame::default(), |ui| {
        changes_card_body(ui, state, true);
    });

    let mut dock_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(dock_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    let commit_card = widgets::card(&mut dock_ui, widgets::CardFrame::default(), |ui| {
        commit_card_body(ui, state);
    })
    .response
    .rect;
    ui.ctx()
        .memory_mut(|m| m.data.insert_temp(dock_height_id, commit_card.height()));

    ui.advance_cursor_after_rect(Rect::from_min_size(
        avail.min,
        Vec2::new(avail.width(), avail.height()),
    ));
}

fn commit_card_body(ui: &mut Ui, state: &mut AppState) {
    commit_message_box(ui, state);
    recent_messages_row(ui, state);
    ui.separator();
    commit_action_row(ui, state);
}

fn changes_card_body(ui: &mut Ui, state: &mut AppState, fills: bool) {
    changes_card_header(ui, state);

    let Some(root_id) = state.selected_root.clone() else {
        ui.colored_label(Color32::GRAY, "Select a repository to see changes.");
        return;
    };
    if state.multi.by_id(&root_id).is_none() {
        return;
    }

    if state.ui.changes_focus != state.selected_root {
        state.ui.changes_focus = state.selected_root.clone();
        state.ui.changes_expanded.clear();
    }

    let (buckets, no_match) = filter_buckets(state, change_buckets(state));
    let empty_text = if no_match.is_empty() {
        "Nothing to commit, working tree is clean."
    } else {
        &no_match
    };
    let mut actions = Vec::new();
    let mut order = Vec::new();
    changes_tree(
        ui,
        state,
        &buckets,
        empty_text,
        fills,
        &mut actions,
        &mut order,
    );
    apply_actions(state, actions, &order);
}

fn changes_tree(
    ui: &mut Ui,
    state: &AppState,
    buckets: &[Bucket<'_>],
    empty_text: &str,
    fills: bool,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    let remaining = ui.available_rect_before_wrap();
    let max_h = remaining.height().max(crate::theme::FILE_ROW_HEIGHT);
    egui::ScrollArea::vertical()
        .id_salt("changes_tree")
        .auto_shrink(!fills)
        .max_height(max_h)
        .show(ui, |ui| {
            let mut painted = false;
            for root in &state.multi.roots {
                let root_buckets: Vec<&Bucket> =
                    buckets.iter().filter(|b| b.root.id == root.id).collect();
                if root_buckets.is_empty() {
                    continue;
                }
                painted = true;
                repo_group(ui, state, root, &root_buckets, actions, order);
            }
            if !painted {
                widgets::empty_state(ui, empty_text);
            }
        });
}

struct GroupHeader<'a> {
    label: &'a str,
    name: &'a str,
    meta: Option<&'a str>,
    count: usize,
    expanded: Option<bool>,
    focused: bool,
    lead: Option<Icon>,
}

/// Paint one group header: the section band, the shared row's hover, the
/// chevron, the name, its metadata, and the file count as a count chip.
///
/// **The band is [`Palette::SECTION_BG`] at [`CornerRadius::ZERO`], not a
/// card.** A group is scaffolding on the changes card; a card inside a card
/// would put a floating border and a second surface around a list the card
/// already contains, and the square corners are what say so at a glance. The
/// row state paints *over* the band rather than replacing it, so a hovered or
/// focused group is still visibly on it.
///
/// Hover is read from [`Ui::rect_contains_pointer`] rather than the response:
/// the count chip below is a later-registered widget over the same rect, and
/// asking the response would make the band flicker off whenever the pointer
/// crossed the number.
fn group_header_row(ui: &mut Ui, id: egui::Id, header: GroupHeader<'_>) -> egui::Response {
    let width = ui.available_width();
    let row = Rect::from_min_size(
        ui.cursor().left_top(),
        Vec2::new(width, crate::theme::GROUP_ROW_HEIGHT),
    );
    ui.allocate_exact_size(row.size(), Sense::hover());

    let collapsible = header.expanded.is_some();
    let response = ui.interact(
        row,
        id,
        if collapsible {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        WidgetInfo::labeled(
            if collapsible {
                WidgetType::Button
            } else {
                WidgetType::Label
            },
            true,
            header.label,
        )
    });

    let painter = ui.painter().clone();
    painter.rect_filled(row, CornerRadius::ZERO, Palette::SECTION_BG);
    let state = if header.focused {
        components::RowState::FocusSelected
    } else if ui.rect_contains_pointer(row) {
        components::RowState::Hover
    } else {
        components::RowState::Default
    };
    widgets::paint_row(ui, row, state);

    let cy = row.center().y;
    if let Some(open) = header.expanded {
        icons::centered_icon(
            ui,
            if open {
                Icon::CHEVRON_DOWN
            } else {
                Icon::CHEVRON_RIGHT
            },
            Pos2::new(row.left() + GROUP_CHEVRON_X, cy),
            12.0,
            Palette::INK_3,
        );
    }
    if let Some(glyph) = header.lead {
        icons::centered_icon(
            ui,
            glyph,
            Pos2::new(row.left() + GROUP_ICON_X, cy),
            14.0,
            if header.focused {
                Palette::BRAND
            } else {
                Palette::INK_2
            },
        );
    }

    let name_x = row.left() + GROUP_NAME_X;
    let name_galley = painter.layout_no_wrap(
        header.name.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_GROUP_NAME),
        Palette::INK,
    );
    let name_w = name_galley.size().x;
    painter.galley_with_override_text_color(
        Pos2::new(name_x, cy - name_galley.size().y / 2.0),
        name_galley,
        Palette::INK,
    );
    if let Some(meta) = header.meta {
        let meta_galley = painter.layout_no_wrap(
            meta.to_owned(),
            crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
            Palette::INK_3,
        );
        painter.galley_with_override_text_color(
            Pos2::new(name_x + name_w + GROUP_GAP, cy - meta_galley.size().y / 2.0),
            meta_galley,
            Palette::INK_3,
        );
    }

    let trailing = Rect::from_min_max(
        Pos2::new(row.right() - GROUP_COUNT_SLOT, row.top()),
        Pos2::new(row.right() - GROUP_EDGE, row.bottom()),
    );
    let mut chip_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(trailing)
            .layout(Layout::right_to_left(Align::Center)),
    );
    chip_ui.spacing_mut().item_spacing.x = 0.0;
    widgets::count_chip(&mut chip_ui, &header.count.to_string());

    response
}

fn repo_group(
    ui: &mut Ui,
    state: &AppState,
    root: &Root,
    buckets: &[&Bucket<'_>],
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    let focused = state.selected_root.as_ref() == Some(&root.id);
    let expanded = focused || state.ui.changes_expanded.contains(&root.id);
    let name = root.id.name();
    let branch = root
        .current_branch
        .clone()
        .unwrap_or_else(|| "<detached>".to_owned());
    let count: usize = buckets.iter().map(|b| b.changes.len()).sum();

    let response = group_header_row(
        ui,
        ui.auto_id_with(("repo_group", &root.id)),
        GroupHeader {
            label: &format!("Toggle {name}"),
            name: &name,
            meta: Some(&branch),
            count,
            expanded: Some(expanded),
            focused,
            lead: Some(Icon::FOLDER_GIT),
        },
    );
    if response.clicked() && !focused {
        actions.push(RowAction::ToggleGroup(root.id.clone()));
    }

    if expanded {
        ui.indent(ui.id().with(("repo_group_body", &root.id)), |ui| {
            for bucket in buckets {
                group_section(ui, state, bucket, actions, order);
            }
        });
    }
}

fn indent_guide(ui: &Ui, block: Rect) {
    let x = block.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP / 2.0;
    ui.painter().rect_filled(
        Rect::from_min_max(
            Pos2::new(x, block.top()),
            Pos2::new(x + 1.0, block.bottom()),
        ),
        CornerRadius::ZERO,
        Palette::LINE_SUBTLE,
    );
}

fn group_section(
    ui: &mut Ui,
    state: &AppState,
    bucket: &Bucket<'_>,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    if bucket.name == MERGE_CONFLICTS {
        let count = bucket.changes.len();
        group_header_row(
            ui,
            ui.auto_id_with(("group_header", &bucket.root.id, bucket.name)),
            GroupHeader {
                label: &format!("{} ({count})", bucket.name),
                name: bucket.name,
                meta: None,
                count,
                expanded: None,
                focused: false,
                lead: None,
            },
        );
    }
    let body = ui.indent(
        ui.id()
            .with(("group_section", &bucket.root.id, bucket.name)),
        |ui| {
            for c in &bucket.changes {
                change_row(ui, state, bucket.root, c, actions, order);
            }
        },
    );
    indent_guide(ui, body.response.rect);
}

pub(crate) fn active_subtab_files(state: &AppState) -> Vec<ChangeRef> {
    let buckets = match state.ui.commit_subtab {
        CommitSubTab::LocalChanges => change_buckets(state),
        CommitSubTab::Shelf | CommitSubTab::Stash => return Vec::new(),
    };
    buckets
        .iter()
        .flat_map(|b| {
            b.changes
                .iter()
                .map(|c| ChangeRef::new(b.root.id.clone(), c.path.clone()))
        })
        .collect()
}

fn status_glyph(status: ChangeStatus) -> (Icon, Color32) {
    match status {
        ChangeStatus::Modified => (Icon::FILE, Palette::STATE_WARNING),
        ChangeStatus::Added => (Icon::FILE_PLUS, Palette::STATE_SUCCESS),
        ChangeStatus::Deleted => (Icon::FILE_MINUS, Palette::STATE_ERROR),
        ChangeStatus::Renamed => (Icon::ARROW_RIGHT_LEFT, Palette::BRAND),
        ChangeStatus::Copied => (Icon::FILES, Palette::STATE_SUCCESS),
        ChangeStatus::Unversioned => (Icon::PLUS_CIRCLE, Palette::INK_2),
        ChangeStatus::Ignored => (Icon::EYE_OFF, Palette::INK_3),
        ChangeStatus::Conflicted => (Icon::FILE_WARNING, Palette::STATE_ERROR),
    }
}

fn badge_letter(c: &Change) -> &'static str {
    match c.status {
        ChangeStatus::Conflicted | ChangeStatus::Copied => "C",
        ChangeStatus::Deleted => "D",
        ChangeStatus::Added | ChangeStatus::Unversioned if !c.unstaged => "N",
        _ => "M",
    }
}

fn status_color(c: &Change) -> Color32 {
    match c.status {
        ChangeStatus::Deleted => Palette::STATUS_DELETED,
        ChangeStatus::Renamed => Palette::BRAND,
        ChangeStatus::Ignored => Palette::INK_3,
        ChangeStatus::Conflicted => Palette::STATE_ERROR,
        _ => match badge_letter(c) {
            "N" => Palette::STATUS_ADDED,
            _ => Palette::STATUS_MODIFIED,
        },
    }
}

fn row_display(path: &Path) -> (String, Option<String>) {
    let name = path.display_name_lossy_or(&path.display().to_string());
    let location = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| format!("{}/", p.display()));
    (name, location)
}

fn stage_label(path: &Path, stage: StageState) -> String {
    let name = path.display();
    match stage {
        StageState::None => format!("Stage {name} (unstaged)"),
        StageState::Partial => format!("Stage all of {name} (partially staged)"),
        StageState::All => format!("Unstage {name} (staged)"),
    }
}

fn paint_checkbox(ui: &Ui, rect: Rect, stage: StageState, resp: &egui::Response) {
    let radius = CornerRadius::same(crate::theme::CHIP_RADIUS);
    let border = if stage != StageState::None || resp.hovered() {
        Palette::INK
    } else {
        Palette::INK_2
    };
    ui.painter()
        .rect_filled(rect, radius, Palette::RAISED_ON_CARD);
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    match stage {
        StageState::None => {}
        StageState::All => {
            let p1 = Pos2::new(rect.left() + 3.5, rect.center().y - 1.0);
            let p2 = Pos2::new(rect.left() + 6.0, rect.center().y + 2.0);
            let p3 = Pos2::new(rect.left() + 11.5, rect.center().y - 3.5);
            let check = Stroke::new(1.6, Palette::INK);
            ui.painter().line_segment([p1, p2], check);
            ui.painter().line_segment([p2, p3], check);
        }
        StageState::Partial => {
            let half = rect.center().y;
            ui.painter().line_segment(
                [
                    Pos2::new(rect.left() + 3.5, half),
                    Pos2::new(rect.right() - 3.5, half),
                ],
                Stroke::new(1.6, Palette::INK),
            );
        }
    }
}

const ROW_PAD_X: f32 = 6.0;
const ROW_CHECKBOX: f32 = 16.0;
const ROW_GAP: f32 = 6.0;
const ROW_EDGE: f32 = 12.0;
const RENAME_ARROW_W: f32 = 24.0;
const MIN_NAME_COLUMN: f32 = 96.0;

const GROUP_CHEVRON_X: f32 = 14.0;
const GROUP_ICON_X: f32 = 32.0;
const GROUP_NAME_X: f32 = 46.0;
const GROUP_GAP: f32 = 8.0;
const GROUP_EDGE: f32 = 12.0;
const GROUP_COUNT_SLOT: f32 = 44.0;

fn row_click_kind(ui: &Ui) -> ClickKind {
    let modifiers = ui.ctx().input(|i| i.modifiers);
    changes_selection::click_kind(modifiers.shift, modifiers.command)
}

#[allow(clippy::too_many_lines)]
fn change_row(
    ui: &mut Ui,
    state: &AppState,
    root: &Root,
    c: &Change,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    let key = root.canonical_key(c);
    if c.status == ChangeStatus::Conflicted {
        conflict_row(ui, state, &root.id, c, &key, actions, order);
        return;
    }

    let selected = state.ui.selected.contains(&key);
    let stage = StageState::of(c);
    let tint = status_color(c);
    let (name, location) = row_display(&c.path);
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional);
    let painter = ui.painter().clone();

    let letter_galley = painter.layout_no_wrap(badge_letter(c).to_owned(), font.clone(), tint);
    let (letter_w, letter_h) = letter_galley.size().into();
    let left = ui.cursor().left_top();
    let content_right = left.x + ui.available_width() - ROW_EDGE;
    let name_x = left.x + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP + letter_w + ROW_GAP;
    let loc_galley = location
        .as_ref()
        .map(|loc| painter.layout_no_wrap(loc.clone(), font.clone(), Palette::INK_3));
    let loc_w = loc_galley.as_ref().map_or(0.0, |g| g.size().x);
    let loc_h = loc_galley.as_ref().map_or(0.0, |g| g.size().y);
    let loc_gap = if loc_galley.is_some() { ROW_GAP } else { 0.0 };
    let full_column = content_right - name_x;
    let loc_below = loc_galley.is_some() && full_column - loc_w - loc_gap < MIN_NAME_COLUMN;
    let name_column = if loc_below {
        full_column
    } else {
        full_column - loc_w - loc_gap
    }
    .max(1.0);
    let name_galley = painter.layout(name.clone(), font.clone(), Palette::INK, name_column);

    let orig_galley = c
        .orig_path
        .as_ref()
        .map(|p| painter.layout_no_wrap(p.display().to_string(), font.clone(), Palette::INK_3));
    let rename_below = match (&orig_galley, name_galley.rows.len()) {
        (Some(orig), 1) => {
            name_x + name_galley.size().x + ROW_GAP + RENAME_ARROW_W + orig.size().x
                > name_x + name_column
        }
        (Some(_), _) => true,
        (None, _) => false,
    };

    let line_h = name_galley
        .rows
        .first()
        .map_or(name_galley.size().y, |row| row.size.y);
    let name_h = name_galley.size().y;
    let name_lines = name_galley.rows.len().max(1);
    let below_lines = usize::from(rename_below) + usize::from(loc_below);
    let content_h = name_h + below_lines as f32 * line_h;
    let row_h = crate::theme::FILE_ROW_HEIGHT + (name_lines - 1 + below_lines) as f32 * line_h;
    let row = Rect::from_min_size(left, Vec2::new(ui.available_width(), row_h));
    ui.allocate_exact_size(row.size(), Sense::hover());

    components::row_shell(
        ui,
        row,
        components::RowState::from_flags(selected, ui.rect_contains_pointer(row)),
        components::RowShell::Railed,
    );

    let block_top = row.top() + (row_h - content_h) / 2.0;
    let cy = block_top + line_h / 2.0;

    let cb = ROW_CHECKBOX;
    let cb_rect = Rect::from_min_max(
        Pos2::new(row.left() + ROW_PAD_X, cy - cb / 2.0),
        Pos2::new(row.left() + ROW_PAD_X + cb, cy + cb / 2.0),
    );
    let cb_resp = ui.interact(
        cb_rect,
        ui.auto_id_with(("row_checkbox", &key)),
        Sense::click(),
    );
    let cb_label = stage_label(&c.path, stage);
    cb_resp.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, cb_label.clone()));
    if cb_resp.clicked() {
        actions.push(RowAction::ToggleStage {
            root: root.id.clone(),
            path: c.path.clone(),
        });
    }
    paint_checkbox(ui, cb_rect, stage, &cb_resp);

    let letter_x = row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP;
    painter.galley_with_override_text_color(
        Pos2::new(letter_x, cy - letter_h / 2.0),
        letter_galley,
        tint,
    );

    painter.galley_with_override_text_color(
        Pos2::new(name_x, block_top),
        name_galley.clone(),
        Palette::INK,
    );

    let rename_top = block_top + name_h;
    let loc_top = rename_top + if rename_below { line_h } else { 0.0 };
    if let Some(orig) = &orig_galley {
        let orig_h = orig.size().y;
        let (arrow_cy, orig_x, orig_y) = if rename_below {
            (
                rename_top + line_h / 2.0,
                name_x + RENAME_ARROW_W,
                rename_top + (line_h - orig_h) / 2.0,
            )
        } else {
            (
                cy,
                name_x + name_galley.size().x + ROW_GAP + RENAME_ARROW_W,
                cy - orig_h / 2.0,
            )
        };
        icons::centered_icon(
            ui,
            Icon::ARROW_RIGHT,
            Pos2::new(orig_x - RENAME_ARROW_W / 2.0, arrow_cy),
            12.0,
            Palette::INK_3,
        );
        painter.galley_with_override_text_color(
            Pos2::new(orig_x, orig_y),
            orig.clone(),
            Palette::INK_3,
        );
    }

    if let Some(loc) = &loc_galley {
        let loc_y = if loc_below {
            loc_top + (line_h - loc_h) / 2.0
        } else {
            cy - loc_h / 2.0
        };
        painter.galley_with_override_text_color(
            Pos2::new(content_right - loc_w, loc_y),
            loc.clone(),
            Palette::INK_3,
        );
    }

    let body_rect = Rect::from_min_max(
        Pos2::new(row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP, row.top()),
        row.max,
    );
    let body = ui.interact(
        body_rect,
        ui.auto_id_with(("row_body", &key)),
        Sense::click(),
    );
    body.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    order.push(RowSlot {
        key: key.clone(),
        selectable: true,
    });
    if body.clicked() {
        actions.push(RowAction::Select {
            key,
            root: root.id.clone(),
            path: c.path.clone(),
            kind: row_click_kind(ui),
        });
    }
}

fn conflict_row(
    ui: &mut Ui,
    state: &AppState,
    root: &RootId,
    c: &Change,
    key: &PathBuf,
    actions: &mut Vec<RowAction>,
    order: &mut Vec<RowSlot>,
) {
    let previewing =
        state.ui.preview_change.as_ref() == Some(&ChangeRef::new(root.clone(), c.path.clone()));
    let (glyph, glyph_tint) = status_glyph(c.status);
    let label = format!("{} {}", badge_letter(c), c.path.display());
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Proportional);
    let painter = ui.painter().clone();

    let left = ui.cursor().left_top();
    let row = Rect::from_min_size(
        left,
        Vec2::new(ui.available_width(), crate::theme::FILE_ROW_HEIGHT),
    );
    ui.allocate_exact_size(row.size(), Sense::hover());

    components::row_shell(
        ui,
        row,
        components::RowState::from_flags(previewing, ui.rect_contains_pointer(row)),
        components::RowShell::Railed,
    );

    let cy = row.center().y;
    let letter_x = row.left() + ROW_PAD_X + ROW_CHECKBOX + ROW_GAP;
    let letter_galley =
        painter.layout_no_wrap(badge_letter(c).to_owned(), font.clone(), glyph_tint);
    let (letter_w, letter_h) = letter_galley.size().into();
    painter.galley_with_override_text_color(
        Pos2::new(letter_x, cy - letter_h / 2.0),
        letter_galley,
        glyph_tint,
    );
    let galley = painter.layout_no_wrap(c.path.display().to_string(), font.clone(), Palette::INK);
    painter.galley_with_override_text_color(
        Pos2::new(letter_x + letter_w + ROW_GAP, cy - galley.size().y / 2.0),
        galley,
        Palette::INK,
    );
    icons::centered_icon(
        ui,
        glyph,
        Pos2::new(row.right() - ROW_EDGE, cy),
        14.0,
        glyph_tint,
    );

    let response = ui.interact(row, ui.auto_id_with(("conflict_row", key)), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.clone()));
    order.push(RowSlot {
        key: key.clone(),
        selectable: false,
    });
    if response.clicked() {
        actions.push(RowAction::Select {
            key: key.clone(),
            root: root.clone(),
            path: c.path.clone(),
            kind: row_click_kind(ui),
        });
    }
}

fn recent_messages_row(ui: &mut Ui, state: &mut AppState) {
    if state.ui.recent_messages.is_empty() {
        return;
    }
    let messages: Vec<String> = state.ui.recent_messages.iter().take(6).cloned().collect();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Recent:").color(Palette::INK_3));
        for m in messages {
            let short = if m.chars().count() > 22 {
                m.chars().take(22).collect::<String>()
            } else {
                m.clone()
            };
            if meta_button(ui, &short).clicked() {
                state.ui.commit_message = m;
            }
        }
    });
}

const CHANGES_PANE_TITLE: &str = "CHANGES";

/// The changes-header control that cannot do anything yet, and why.
///
/// ADR-0010's rule is that a flow which has not shipped stays *reachable* and
/// stays honestly inert, and this codebase's rendering of "reachable but cannot
/// act" is a **disabled menu row**: painted, dimmed to 45 % ink, `is_disabled()`
/// in the accessibility tree, carrying its reason on hover. So the entry here
/// is `enabled: false` and has no `clicked` arm anywhere near it, pressing it
/// dispatches no operation, opens no dialog, and calls no git.
///
/// That is the whole point of the shape: an enabled-looking control that cannot
/// do anything *and cannot be read* is noise, and the previous version of this
/// header was five of them. The scope gap is recorded in this table rather than
/// hidden, and it is the same gap the spec keeps out of scope.
const INERT_HEADER_ACTIONS: [(Icon, &str, &str); 1] = [(
    Icon::LAYERS,
    "Group by",
    "No grouping surface yet (ADR-0010)",
)];

fn changes_card_header(ui: &mut Ui, state: &mut AppState) {
    let total = state
        .multi
        .roots
        .iter()
        .flat_map(|r| &r.status.changes)
        .filter(|c| !matches!(c.status, ChangeStatus::Ignored))
        .count();
    let count = total.to_string();

    widgets::pane_header(ui, CHANGES_PANE_TITLE, Some(&count), |ui| {
        let overflow = widgets::icon_button(ui, Icon::MORE_HORIZONTAL);
        overflow.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, "Changes header overflow")
        });
        changes_header_overflow(&overflow);

        let refresh = widgets::icon_button(ui, Icon::REFRESH_CW);
        refresh.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Refresh changes"));
        if refresh.clicked() {
            state.refresh(Affected::All);
        }

        let rollback = widgets::icon_button(ui, Icon::UNDO);
        rollback.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Rollback"));
        if rollback.clicked() {
            let ch = selected_changes(state);
            if ch.is_empty() {
                state.ui.toast = Some(Toast::warning("Select files to discard."));
            } else {
                state.ui.confirm = Some(PendingConfirm::Discard { changes: ch });
            }
        }

        let all_expanded = state
            .multi
            .roots
            .iter()
            .filter(|r| state.selected_root.as_ref() != Some(&r.id))
            .all(|r| state.ui.changes_expanded.contains(&r.id));
        let (icon, label) = if all_expanded {
            (Icon::CHEVRON_UP, "Collapse all groups")
        } else {
            (Icon::CHEVRON_DOWN, "Expand all groups")
        };
        let expand_all = widgets::icon_button(ui, icon);
        expand_all.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
        if expand_all.clicked() {
            if all_expanded {
                state.ui.changes_expanded.clear();
            } else {
                for r in &state.multi.roots {
                    if state.selected_root.as_ref() != Some(&r.id) {
                        state.ui.changes_expanded.insert(r.id.clone());
                    }
                }
            }
        }
    });

    file_filter_row(ui, state);
}

fn changes_header_overflow(trigger: &egui::Response) {
    use egui::Popup;
    Popup::menu(trigger).show(|menu| {
        menu.spacing_mut().item_spacing.y = 0.0;
        menu.set_min_width(200.0);
        menu.set_max_width(224.0);
        for (icon, label, reason) in INERT_HEADER_ACTIONS {
            widgets::menu_item(
                menu,
                widgets::MenuItemProps {
                    icon,
                    label,
                    data: None,
                    shortcut: None,
                    state: widgets::MenuItemState::disabled(reason),
                    kind: widgets::MenuItemKind::Default,
                },
            );
        }
    });
}

fn preview_and_editor_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::card(ui, widgets::CardFrame::default(), |ui| {
        let preview = state.ui.preview_change.clone();
        preview_header(ui, state, preview.as_ref());
        match preview {
            Some(change) => {
                let root = change.root.clone();
                crate::ui::diff::render_diff(ui, state, &root, &None, &None, &Some(change));
            }
            None => {
                ui.colored_label(
                    Color32::GRAY,
                    "Select a changed file to preview its unified diff.",
                );
            }
        }
    });
}

fn preview_header(ui: &mut Ui, state: &mut AppState, change: Option<&ChangeRef>) {
    let Some(change) = change else {
        return;
    };
    widgets::card_header(ui, |ui| {
        let width = ui.available_width();
        // A fixed-height centred band, not a plain horizontal: a one-pass
        // layout centres a first, shorter item against the row height it
        // *starts* with, so the path hung high over the 28 pt chevrons that
        // grew the row after it.
        ui.allocate_ui_with_layout(
            Vec2::new(width, widgets::ICON_BUTTON_SIZE),
            Layout::left_to_right(Align::Center),
            |ui| {
                let path_text = change.path.display().to_string();
                let path_resp = ui.label(
                    RichText::new(&path_text)
                        .font(crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE))
                        .color(Palette::INK),
                );
                path_resp.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Label, true, format!("Previewing {path_text}"))
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    change_nav(ui, state, change);
                });
            },
        );
    });
}

fn change_nav(ui: &mut Ui, state: &mut AppState, change: &ChangeRef) {
    let files = active_subtab_files(state);
    let current = files.iter().position(|c| c == change);
    let prev_enabled = current.is_some_and(|i| i > 0);
    let next_enabled = current.is_some_and(|i| i + 1 < files.len());
    let next = widgets::icon_button_enabled(ui, Icon::CHEVRON_RIGHT, next_enabled);
    next.widget_info(|| WidgetInfo::labeled(WidgetType::Button, next_enabled, "Next change"));
    if next_enabled && next.clicked() {
        state.ui.preview_change = current.and_then(|i| files.get(i + 1)).cloned();
    }
    let prev = widgets::icon_button_enabled(ui, Icon::CHEVRON_LEFT, prev_enabled);
    prev.widget_info(|| WidgetInfo::labeled(WidgetType::Button, prev_enabled, "Previous change"));
    if prev_enabled && prev.clicked() {
        state.ui.preview_change = current.and_then(|i| files.get(i - 1)).cloned();
    }
}

const MESSAGE_HEADING: &str = "Commit message:";

const MESSAGE_WELL_LABEL: &str = "Commit message";
const MESSAGE_WELL_HINT: &str = "Description";

const MESSAGE_WELL_ROWS: usize = 4;

const MESSAGE_WELL_PAD: i8 = 8;

pub const MESSAGE_WELL_HEIGHT: f32 = 76.0;

const META_BUTTON_PAD_X: f32 = 8.0;

const COMMIT_CONTROL_H: f32 = 32.0;

const COMMIT_CHEVRON_W: f32 = 28.0;

const COMMIT_RULE_W: f32 = 1.0;

const COMMIT_LABEL: &str = "Commit";

struct CommitHalves {
    label: egui::Response,
    chevron: egui::Response,
}

fn commit_message_box(ui: &mut Ui, state: &mut AppState) {
    ui.label(RichText::new(MESSAGE_HEADING).color(Palette::INK_2));
    message_well(ui, &mut state.ui.commit_message);
    commit_meta_row(ui, state);
}

fn message_well(ui: &mut Ui, buf: &mut String) -> egui::Response {
    let out = egui::Frame::new()
        .fill(Palette::RAISED_ON_CARD)
        .corner_radius(CornerRadius::same(crate::theme::CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(MESSAGE_WELL_PAD, MESSAGE_WELL_PAD))
        .show(ui, |ui| {
            let resp = egui::TextEdit::multiline(buf)
                .hint_text(MESSAGE_WELL_HINT)
                .desired_width(f32::INFINITY)
                .desired_rows(MESSAGE_WELL_ROWS)
                .text_color(Palette::INK)
                .frame(egui::Frame::new())
                .show(ui);
            resp.response.widget_info(|| {
                WidgetInfo::labeled(WidgetType::TextEdit, true, MESSAGE_WELL_LABEL)
            });
            resp.response
        });
    if out.inner.has_focus() {
        ui.painter().rect_stroke(
            out.response.rect.expand(1.0),
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
    out.response
}

fn commit_meta_row(ui: &mut Ui, state: &mut AppState) {
    let row_h = components::KIT_BUTTON_H;
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(Vec2::new(width, row_h), Sense::hover());
    let painter = ui.painter().clone();
    let font = crate::theme::chrome_font(crate::theme::TYPE_CONTROL);
    let amend_label = painter.layout_no_wrap(AMEND_LABEL.to_owned(), font, Palette::INK_2);
    let amend_w = ROW_CHECKBOX + ROW_GAP + amend_label.size().x;
    amend_checkbox_at(
        ui,
        Rect::from_min_max(
            Pos2::new(row.right() - amend_w, row.top()),
            Pos2::new(row.right(), row.bottom()),
        ),
        &mut state.ui.amend,
    );
    let kebab_rect = Rect::from_min_size(
        Pos2::new(row.left(), row.center().y - META_KEBAB_SIZE / 2.0),
        Vec2::splat(META_KEBAB_SIZE),
    );
    commit_options_kebab_at(ui, kebab_rect, state);
}

const META_KEBAB_SIZE: f32 = 24.0;

fn commit_options_kebab_at(ui: &mut Ui, rect: Rect, state: &mut AppState) {
    use egui::Popup;
    let response = ui.interact(rect, ui.auto_id_with("commit_options"), Sense::click());
    let widget_state = widgets::WidgetState::from_response(true, &response);
    let painter = ui.painter().clone();
    let fill = match widget_state {
        widgets::WidgetState::Idle => Palette::RAISED_ON_CARD,
        widgets::WidgetState::Hovered => {
            widgets::mix(Palette::RAISED_ON_CARD, Color32::WHITE, 0.10)
        }
        _ => widgets::mix(Palette::RAISED_ON_CARD, Color32::WHITE, 0.20),
    };
    painter.rect_filled(rect, CornerRadius::same(crate::theme::CONTROL_RADIUS), fill);
    let ink = match widget_state {
        widgets::WidgetState::Idle => Palette::INK_2,
        _ => Palette::INK,
    };
    icons::centered_icon(ui, Icon::MORE_HORIZONTAL, rect.center(), 14.0, ink);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, COMMIT_OPTIONS_LABEL));
    widgets::focus_ring(ui, &response);

    let repos: Vec<(RootId, String)> = state
        .multi
        .roots
        .iter()
        .filter(|r| r.status.changes.iter().any(|c| c.staged))
        .map(|r| (r.id.clone(), r.path.display_name_or("?")))
        .collect();

    Popup::menu(&response).show(|menu| {
        menu.checkbox(&mut state.ui.commit_no_verify, COMMIT_NO_VERIFY_LABEL)
            .on_hover_text(COMMIT_NO_VERIFY_HINT);
        menu.separator();
        SubMenuButton::new(COMMIT_MULTI_LABEL)
            .config(MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside))
            .ui(menu, |ui| {
                if repos.is_empty() {
                    ui.label(COMMIT_MULTI_EMPTY);
                    return;
                }
                for (id, name) in &repos {
                    let mut checked = !state.ui.commit_repo_unchecks.contains(id);
                    if ui.checkbox(&mut checked, name).changed() {
                        if checked {
                            state.ui.commit_repo_unchecks.remove(id);
                        } else {
                            state.ui.commit_repo_unchecks.insert(id.clone());
                        }
                    }
                }
            });
    });
}

const COMMIT_OPTIONS_LABEL: &str = "Commit card options";
const COMMIT_NO_VERIFY_LABEL: &str = "Do not run git commit hooks";
const COMMIT_NO_VERIFY_HINT: &str =
    "Commits with --no-verify. Unchecks itself after a successful commit.";
const COMMIT_MULTI_LABEL: &str = "Commit on multiple repos";
const COMMIT_MULTI_EMPTY: &str = "No repos have staged changes";

fn amend_checkbox_at(ui: &mut Ui, rect: Rect, amend: &mut bool) -> egui::Response {
    let painter = ui.painter().clone();
    let galley = painter.layout_no_wrap(
        AMEND_LABEL.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Palette::INK_2,
    );
    let box_rect = Rect::from_center_size(
        Pos2::new(rect.left() + ROW_CHECKBOX / 2.0, rect.center().y),
        Vec2::splat(ROW_CHECKBOX),
    );
    let response = ui.interact(rect, ui.auto_id_with("commit_amend"), Sense::click());
    let mut checked = *amend;
    if response.clicked() {
        checked = !checked;
        *amend = checked;
    }
    paint_checkbox(
        ui,
        box_rect,
        if checked {
            StageState::All
        } else {
            StageState::None
        },
        &response,
    );
    painter.galley_with_override_text_color(
        Pos2::new(
            box_rect.right() + ROW_GAP,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        Palette::INK_2,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Checkbox, true, AMEND_LABEL));
    widgets::focus_ring(ui, &response);
    response
}

const AMEND_LABEL: &str = "Amend";

fn meta_button_width(ui: &Ui, label: &str) -> f32 {
    2.0 * META_BUTTON_PAD_X
        + ui.painter()
            .layout_no_wrap(
                label.to_owned(),
                crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
                Color32::WHITE,
            )
            .size()
            .x
}

fn meta_button_at(ui: &mut Ui, rect: Rect, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Color32::WHITE,
    );
    let response = ui.interact(
        rect,
        ui.auto_id_with(("meta_button", label)),
        Sense::click(),
    );
    let state = widgets::WidgetState::from_response(true, &response);
    let painter = ui.painter().clone();
    let fill = match state {
        widgets::WidgetState::Idle => Palette::RAISED_ON_CARD,
        widgets::WidgetState::Hovered => {
            widgets::mix(Palette::RAISED_ON_CARD, Color32::WHITE, 0.10)
        }
        _ => widgets::mix(Palette::RAISED_ON_CARD, Color32::WHITE, 0.20),
    };
    painter.rect_filled(rect, CornerRadius::same(crate::theme::CONTROL_RADIUS), fill);
    let ink = match state {
        widgets::WidgetState::Idle => Palette::INK_2,
        _ => Palette::INK,
    };
    painter.galley_with_override_text_color(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    widgets::focus_ring(ui, &response);
    response
}

fn meta_button(ui: &mut Ui, label: &str) -> egui::Response {
    let width = meta_button_width(ui, label);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width, components::KIT_BUTTON_H), Sense::hover());
    meta_button_at(ui, rect, label)
}

fn commit_action_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        let can_commit = !state.ui.commit_message.trim().is_empty() && state.has_commit_target();
        commit_control(ui, state, can_commit);
        ui.add_space(10.0);
        if widgets::ghost_button(ui, None, "Shelve\u{2026}")
            .on_hover_text("Shelve selected changes")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Shelve);
        }
        if widgets::ghost_button(ui, None, "Stash\u{2026}")
            .on_hover_text("Stash all changes")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Stash);
        }
    });
}

/// **The one Commit control: a content-width split button with the chevron
/// inside it.**
///
/// The previous version was a brand `Commit` button with a *separate* chevron
/// button beside it, which is exactly why it read as two carets glued together
/// with a gap. This is one painted button: two fills that meet across a single
/// 1 px rule, no gap, and one brand run. What the chevron half opens is
/// unchanged, the alternatives menu still opens from the chevron.
///
/// - **Content-width.** The width is its own label's width plus the chevron's
///   own square, and nothing stretches it. The card's inner row is 316 pt and
///   this control is roughly a quarter of it; a full-width brand bar would be a
///   different control, not a bigger Commit.
/// - **The rule is on the button, not around it.** One hairline between the
///   halves, and no stroke anywhere near the button's own edge, a bordered
///   button is an R2 failure.
/// - **The halves are two hit regions inside one control.** That is what makes
///   "the menu opens from the chevron" addressable, and what lets each half take
///   its own hover state; it is not two buttons, because there is no gap, no
///   second border and no second edge to read as a seam.
/// - **The fill and ink decisions are the shared primary's**, through
///   [`widgets::ButtonVariant::Primary`]; only the geometry is local, because
///   the shared primary has no split variant.
///
/// The one Commit control, or why it is not one right now.
///
/// A refused control reports its reason on hover. It has to go through
/// `on_disabled_hover_text` rather than `on_hover_text`: a `Ui` that is
/// disabled reports no hover for the widgets inside it, so the ordinary call
/// would be exactly the one control in this window that cannot explain itself.
fn commit_control(ui: &mut Ui, state: &mut AppState, can_commit: bool) {
    let mut halves = widgets::disabled_child_scope(ui, can_commit, commit_control_paint);
    if !can_commit {
        halves.label = halves
            .label
            .on_disabled_hover_text(commit_blocked_reason(state));
    }
    if can_commit && halves.label.clicked() {
        do_commit(state, false);
    }
    if can_commit {
        commit_split_menu(&halves.chevron, state);
    }
}

fn commit_blocked_reason(state: &AppState) -> &'static str {
    if state.ui.commit_message.trim().is_empty() {
        "Enter a commit message to commit"
    } else {
        "Stage at least one file — Commit commits the index and nothing else"
    }
}

fn commit_control_paint(ui: &mut Ui) -> CommitHalves {
    let enabled = ui.is_enabled();
    let pad_x = crate::theme::BUTTON_PADDING.x;
    let galley = ui.painter().layout_no_wrap(
        COMMIT_LABEL.to_owned(),
        crate::theme::chrome_font(crate::theme::TYPE_CONTROL),
        Color32::WHITE,
    );
    let label_w = 2.0 * pad_x + galley.size().x;
    let (button, _) = ui.allocate_exact_size(
        Vec2::new(label_w + COMMIT_CHEVRON_W, COMMIT_CONTROL_H),
        Sense::hover(),
    );

    let rule_x = button.left() + label_w;
    let left = Rect::from_min_max(button.min, Pos2::new(rule_x, button.max.y));
    let right = Rect::from_min_max(Pos2::new(rule_x + COMMIT_RULE_W, button.min.y), button.max);

    let label = ui.interact(left, ui.auto_id_with("commit_label_half"), Sense::click());
    let chevron = ui.interact(
        right,
        ui.auto_id_with("commit_chevron_half"),
        Sense::click(),
    );
    label.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, "Commit changes"));
    chevron
        .widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, "Commit split options"));

    let radius = crate::theme::CONTROL_RADIUS;
    let painter = ui.painter().clone();
    let fill_of = |half: &egui::Response| {
        let state = widgets::WidgetState::from_response(enabled, half);
        widgets::ButtonVariant::Primary.fill(state)
    };
    painter.rect_filled(
        left,
        CornerRadius {
            nw: radius,
            ne: 0,
            se: 0,
            sw: radius,
        },
        fill_of(&label),
    );
    painter.rect_filled(
        right,
        CornerRadius {
            nw: 0,
            ne: radius,
            se: radius,
            sw: 0,
        },
        fill_of(&chevron),
    );
    painter.rect_stroke(
        Rect::from_min_max(
            Pos2::new(rule_x, button.top()),
            Pos2::new(rule_x + COMMIT_RULE_W, button.bottom()),
        ),
        CornerRadius::ZERO,
        Stroke::new(
            COMMIT_RULE_W,
            widgets::mix(Palette::BRAND, Palette::BRAND_INK, 0.35),
        ),
        StrokeKind::Inside,
    );

    let ink = widgets::ButtonVariant::Primary.text(if enabled {
        widgets::WidgetState::Idle
    } else {
        widgets::WidgetState::Disabled
    });
    painter.galley_with_override_text_color(
        Pos2::new(
            left.center().x - galley.size().x / 2.0,
            left.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    icons::centered_icon(ui, Icon::CHEVRON_DOWN, right.center(), 14.0, ink);

    widgets::focus_ring(ui, &label);
    widgets::focus_ring(ui, &chevron);
    CommitHalves { label, chevron }
}

fn commit_split_menu(chevron: &egui::Response, state: &mut AppState) {
    use egui::Popup;
    Popup::menu(chevron).show(|menu| {
        if menu
            .button("Commit and Push...")
            .on_hover_text("Commit then open the push dialog")
            .clicked()
        {
            do_commit(state, true);
            menu.close();
        }
    });
}

fn do_commit(state: &mut AppState, and_push: bool) {
    let msg = state.ui.commit_message.clone();
    let amend = state.ui.amend;
    let no_verify = state.ui.commit_no_verify;
    if !state.ui.recent_messages.contains(&msg) {
        state.ui.recent_messages.insert(0, msg.clone());
        state.ui.recent_messages.truncate(12);
    }
    if and_push {
        if let Some(root) = state.selected_root.clone() {
            state.dispatch_commit(&root, &msg, amend, no_verify, true);
        }
    } else {
        let targets = state.commit_targets();
        match targets.len() {
            0 => {}
            1 => {
                let root = targets[0].id.clone();
                state.dispatch_commit(&root, &msg, amend, no_verify, false);
            }
            _ => state.run_commit_across_roots(targets, &msg, amend),
        }
    }
    state.ui.commit_message.clear();
    state.save_durable_ui();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_state_is_derived_from_the_staged_and_unstaged_pair() {
        let of = |staged, unstaged| {
            StageState::of(&Change {
                path: PathBuf::from("a.txt"),
                status: ChangeStatus::Modified,
                chunks: vec![],
                staged,
                unstaged,
                orig_path: None,
            })
        };
        assert_eq!(of(true, false), StageState::All);
        assert_eq!(of(true, true), StageState::Partial);
        assert_eq!(of(false, true), StageState::None);
        assert_eq!(of(false, false), StageState::None);
    }

    #[test]
    fn row_display_splits_filename_from_dim_location() {
        assert_eq!(
            row_display(Path::new("base.txt")),
            ("base.txt".to_owned(), None)
        );
        assert_eq!(
            row_display(Path::new("src/app.rs")),
            ("app.rs".to_owned(), Some("src/".to_owned()))
        );
        assert_eq!(
            row_display(Path::new("a/b/c.txt")),
            ("c.txt".to_owned(), Some("a/b/".to_owned()))
        );
    }
}
