use crate::theme::{Palette, TYPE_CONTROL};
use crate::ui::branch_tree_view::{self, TreeEvent, TreeGroup, TreeProps};
use crate::ui::branches::fetch_scope;
use crate::ui::branches_tree::build_branch_view;
use crate::ui::commit_menu::{CommitFacts, CommitMenuAction, commit_menu};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::rewrite_gating;
use crate::ui::widgets::{
    self, BadgeKind, MenuItemKind, MenuItemProps, MenuItemState, RefKind, menu_item,
};
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Grid, Layout, Panel, Popup,
    PopupKind, Pos2, Rect, Response, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetType,
};
use std::ops::Range;
use std::path::PathBuf;
use turbogit_app::log_states::{self, LogRecovery, LogState};
use turbogit_app::root_caches::{LogScope, file_stat};
use turbogit_app::state::{
    AppState, BlameTarget, Dialog, DiffTarget, NewBranchBase, PendingConfirm, Tab, Toast,
};
use turbogit_app::sync_service;
use turbogit_domain::model::{
    BranchKind, ChangeStatus, Commit, CommitId, DateFormat, DisplayName, GitRefKind, PatchOrigin,
    RefState, Root, RootId, SignatureState, forward_slash_path,
};

const BRANCHES_WIDTH: f32 = 210.0;
const FILES_WIDTH: f32 = 344.0;
const DETAILS_HEIGHT: f32 = 440.0;
const LOG_FILE_ROW_HEIGHT: f32 = 40.0;
const STAT_GAP: f32 = 6.0;
const STRIPE_WIDTH: f32 = 3.0;
const GUTTER: f32 = 5.0;
const PANE_GAP: f32 = 8.0;
const PANE_COLUMN_GAP: f32 = 10.0;
const PANE_CARD_PAD: i8 = 8;
const MONO_TEXT: f32 = crate::theme::TYPE_BODY;

const STATUS_PILL: widgets::ChipGeometry = widgets::ChipGeometry {
    height: widgets::CHIP_HEIGHT,
    pad_x: widgets::CHIP_PAD_X,
    radius: crate::theme::CONTROL_RADIUS as f32,
};

const COL_GRAPH: f32 = 10.0;
const COL_HASH: f32 = 26.0;
const COL_AUTHOR: f32 = 84.0;
const COL_MESSAGE: f32 = 164.0;
const COL_ROOTS: f32 = -GUTTER;
const DATE_RIGHT_PAD: f32 = 8.0;
const PILL_RESERVE: f32 = 30.0;

const COMMIT_COLUMNS: [widgets::PaneColumn; 5] = [
    widgets::PaneColumn::start("ROOTS", COL_ROOTS),
    widgets::PaneColumn::start("HASH", COL_HASH),
    widgets::PaneColumn::start("AUTHOR", COL_AUTHOR),
    widgets::PaneColumn::start("MESSAGE", COL_MESSAGE),
    widgets::PaneColumn::end("DATE", DATE_RIGHT_PAD),
];

const GRAPH_COLORS: &[Color32] = &[
    Color32::from_rgb(80, 140, 230),
    Color32::from_rgb(220, 120, 140),
    Color32::from_rgb(120, 200, 130),
    Color32::from_rgb(200, 170, 90),
    Color32::from_rgb(170, 130, 220),
    Color32::from_rgb(90, 190, 200),
    Color32::from_rgb(230, 150, 90),
    Color32::from_rgb(150, 200, 220),
];

pub(crate) struct CommitTable;

impl CommitTable {
    pub(crate) const ROW_HEIGHT: f32 = crate::theme::FILE_ROW_HEIGHT;
    pub(crate) const HASH: f32 = COL_HASH;
    pub(crate) const AUTHOR: f32 = COL_AUTHOR;
    pub(crate) const MESSAGE: f32 = COL_MESSAGE;
    pub(crate) const DATE_RIGHT_PAD: f32 = DATE_RIGHT_PAD;
}

fn fmt_time(t: i64) -> String {
    match Utc.timestamp_opt(t, 0) {
        chrono::LocalResult::Single(dt) => {
            let local: DateTime<Local> = DateTime::from(dt);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}

fn fmt_date(t: i64, mode: DateFormat) -> String {
    match mode {
        DateFormat::Iso => fmt_time(t),
        DateFormat::Absolute => fmt_time(t),
        DateFormat::Relative => {
            let now = Local::now().timestamp();
            let d = now - t;
            if d < 60 {
                format!("{d}s ago")
            } else if d < 3600 {
                format!("{}m ago", d / 60)
            } else if d < 86400 {
                format!("{}h ago", d / 3600)
            } else if d < 2592000 {
                format!("{}d ago", d / 86400)
            } else {
                fmt_time(t)
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn root_color(idx: usize) -> Color32 {
    GRAPH_COLORS[idx % GRAPH_COLORS.len()]
}

fn root_index_of(state: &AppState, root: &RootId) -> usize {
    state
        .multi
        .roots
        .iter()
        .position(|r| &r.id == root)
        .unwrap_or(0)
}

fn visible_root_ids(state: &AppState) -> Vec<RootId> {
    turbogit_app::log_display::visible_roots(state)
}

fn ensure_log_data(state: &mut AppState) {
    for id in visible_root_ids(state) {
        if !state.caches.refs_loaded(&id) {
            state.fetch_refs(id);
        }
    }
    if let (Some(root), Some(cid)) = (
        state.selected_root.clone(),
        state.ui.selected_commit.clone(),
    ) {
        state.ensure_files(&root, &cid);
        state.fetch_file_stats(root, cid);
    }
    if let (Some(root), Some(path)) = (state.selected_root.clone(), state.ui.log_path_scope.clone())
    {
        state.ensure_log(&root, LogScope::Path(path));
    }
    if let Some((root, ref_name)) = state.ui.log_ref_scope.clone() {
        state.ensure_log(&root, LogScope::Ref(ref_name));
    }
    let query = state.ui.log_filter.trim().to_string();
    if !query.is_empty() {
        for id in visible_root_ids(state) {
            state.ensure_log(&id, LogScope::Search(query.clone()));
        }
    }
}

enum ListedWindow {
    Unscoped,
    Scoped(RootId, LogScope),
}

impl ListedWindow {
    fn has_more(&self, state: &AppState) -> bool {
        match self {
            Self::Unscoped => visible_root_ids(state)
                .iter()
                .any(|id| state.caches.log_has_more(id)),
            Self::Scoped(root, scope) => state.caches.scoped_log_has_more(root, scope),
        }
    }

    fn load_more(&self, state: &mut AppState) {
        match self {
            Self::Unscoped => state.load_more_log(),
            Self::Scoped(root, scope) => state.load_more_scoped_log(root, scope),
        }
    }
}

fn listed_windows(state: &AppState) -> Vec<ListedWindow> {
    let mut windows = match (&state.ui.log_ref_scope, &state.ui.log_path_scope) {
        (Some((root, ref_name)), _) => {
            vec![ListedWindow::Scoped(
                root.clone(),
                LogScope::Ref(ref_name.clone()),
            )]
        }
        (None, Some(path)) => state
            .selected_root
            .clone()
            .map(|root| ListedWindow::Scoped(root, LogScope::Path(path.clone())))
            .into_iter()
            .collect(),
        (None, None) => vec![ListedWindow::Unscoped],
    };
    let query = state.ui.log_filter.trim();
    if state.ui.log_path_scope.is_none() && !query.is_empty() {
        windows.extend(
            visible_root_ids(state)
                .into_iter()
                .map(|root| ListedWindow::Scoped(root, LogScope::Search(query.to_owned()))),
        );
    }
    windows
}

fn find_commit<'a>(state: &'a AppState, root: &RootId, cid: &str) -> Option<&'a Commit> {
    if let Some((scope_root, ref_name)) = &state.ui.log_ref_scope
        && let Some(commits) = state.caches.ref_log(scope_root, ref_name)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    if let Some(path) = &state.ui.log_path_scope
        && let Some(commits) = state.caches.path_log(root, path)
    {
        return commits.iter().find(|c| c.id == cid);
    }
    state
        .caches
        .log(root)
        .and_then(|commits| commits.iter().find(|c| c.id == cid))
}

fn row_offset_from_built(index: usize, visible: Range<usize>, pitch: f32) -> f32 {
    (index as f32 - visible.start as f32) * pitch
}

/// Whether a row of `row_height` whose top sits `offset` points below the first
/// built row is already on screen, given a viewport that starts `view_top`
/// points below that row and is `view_height` tall.
///
/// The viewport, not the built range, is the test, and the row has to be
/// FULLY inside it. `show_rows` builds one row past the bottom edge on purpose,
/// so a row can be in the frame and still be below the fold, treating "built"
/// as "on screen" would leave a selection sitting just out of sight, which is
/// the one outcome this whole path exists to prevent.
fn row_is_in_view(offset: f32, row_height: f32, view_top: f32, view_height: f32) -> bool {
    offset >= view_top && offset + row_height <= view_top + view_height
}

/// The scroll delta that puts a row's centre on the viewport's centre.
///
/// `Ui::scroll_with_delta` is inverted against the area's offset (egui applies
/// `-delta` to it), so the delta is the viewport's centre MINUS the row's: a
/// row below the middle yields a negative delta, which moves the offset forwards
/// and reveals later history. The sign is the easy thing to get backwards, so it
/// is a named function rather than an expression at the call site.
fn centre_scroll_delta(row_centre: f32, viewport_centre: f32) -> f32 {
    viewport_centre - row_centre
}

fn commit_index(rows: &[turbogit_app::log_display::LogRow], id: &str) -> Option<usize> {
    rows.iter().position(|row| row.id == id)
}

fn ref_kind(kind: GitRefKind) -> RefKind {
    match kind {
        GitRefKind::Branch => RefKind::Branch,
        GitRefKind::Remote => RefKind::Remote,
        GitRefKind::Tag => RefKind::Tag,
    }
}

pub fn show_log(ui: &mut Ui, state: &mut AppState) {
    if let Some((root, _)) = &state.ui.log_ref_scope
        && state.selected_root.as_ref() != Some(root)
    {
        state.ui.log_ref_scope = None;
    }
    ensure_log_data(state);

    let avail_w = ui.available_width();
    let branches_w = BRANCHES_WIDTH.min((avail_w * 0.25).max(140.0));
    let files_w = FILES_WIDTH.min((avail_w * 0.32).max(180.0));

    Panel::left("log_branches_pane")
        .exact_size(branches_w + PANE_COLUMN_GAP)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE)
        .show(ui, |ui| {
            let region = ui.available_rect_before_wrap();
            ui.scope_builder(
                UiBuilder::new().max_rect(Rect::from_min_max(
                    Pos2::new(region.left(), region.top()),
                    Pos2::new(region.right() - PANE_COLUMN_GAP, region.bottom()),
                )),
                |ui| {
                    pane_card(ui, |ui| branches_pane(ui, state));
                },
            );
        });

    Panel::right("log_right_column")
        .exact_size(files_w + PANE_COLUMN_GAP)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE)
        .show(ui, |ui| {
            let region = ui.available_rect_before_wrap();
            ui.scope_builder(
                UiBuilder::new().max_rect(Rect::from_min_max(
                    Pos2::new(region.left() + PANE_COLUMN_GAP, region.top()),
                    Pos2::new(region.right(), region.bottom()),
                )),
                |ui| {
                    let details_h = DETAILS_HEIGHT.min((ui.available_height() - 80.0).max(96.0));
                    Panel::bottom("log_details_pane")
                        .exact_size(details_h)
                        .resizable(false)
                        .frame(Frame::NONE)
                        .show(ui, |ui| pane_card(ui, |ui| details_pane(ui, state)));
                    ui.add_space(PANE_GAP);
                    let files_h = (ui.available_height() - PANE_CARD_PAD as f32).max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), files_h),
                        Layout::top_down(Align::Min),
                        |ui| pane_card(ui, |ui| files_pane(ui, state)),
                    );
                },
            );
        });

    pane_card(ui, |ui| graph_pane(ui, state));
}

fn pane_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    widgets::card(
        ui,
        widgets::CardFrame::default()
            .padded(PANE_CARD_PAD)
            .stretch_height(),
        add,
    )
    .inner
}

pub fn branches_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::pane_header(ui, "BRANCHES", None, |_ui| {});
    ui.add_space(2.0);
    widgets::search_input(ui, "Search branches", &mut state.ui.log_branch_filter);
    ui.add_space(4.0);

    branch_tree_view::warm_tags(state);

    state.ui.log_tree.show_remotes = true;

    let ids = visible_root_ids(state);
    let mut roots: Vec<Root> = ids
        .iter()
        .filter_map(|id| state.multi.by_id(id).cloned())
        .collect();
    let mut deco_by_root: std::collections::HashMap<
        RootId,
        std::collections::HashMap<String, RefState>,
    > = std::collections::HashMap::new();
    for root in &mut roots {
        let mut deco: std::collections::HashMap<String, RefState> =
            std::collections::HashMap::new();
        for group in state.caches.ref_groups(&root.id) {
            for r in group {
                deco.entry(r.name.clone()).or_insert(r.state);
            }
        }
        for b in &mut root.branches {
            if b.kind == BranchKind::Remote {
                let full = match &b.remote {
                    Some(r) => format!("{r}/{}", b.name),
                    None => b.name.clone(),
                };
                b.gone = deco.get(&full).is_some_and(|st| *st == RefState::Gone);
            }
        }
        deco_by_root.insert(root.id.clone(), deco);
    }
    let tags_by_root = state.caches.tags_by_root();
    let mut view = build_branch_view(
        &roots,
        tags_by_root,
        &std::collections::HashMap::new(),
        &|_| state.ui.log_tree.show_remotes,
    );
    for repo in &mut view.repos {
        if let Some(deco) = deco_by_root.get(&repo.root_id) {
            for tag in &mut repo.tags {
                if let Some(d) = deco.get(&tag.name) {
                    tag.state = *d;
                }
            }
        }
    }

    let props = TreeProps {
        view: &view,
        filter: &state.ui.log_branch_filter,
        repo_filter: None,
        tags_by_root,
        multi_repo: state.multi.roots.len() > 1,
        busy: false,
        has_any_data: true,
        merge_in_progress: false,
        last_fetch: None,
        now: chrono::Utc::now(),
        allows_rename: false,
        allows_context_menu: false,
        id_salt: "log_branch_tree",
        full_height: false,
        collapse_remotes_by_default: true,
    };
    let events = branch_tree_view::branch_tree(ui, &props, &mut state.ui.log_tree);
    for event in events {
        apply_log_tree_event(state, event);
    }

    ui.add_space(8.0);
    roots_filter_section(ui, state);
}

fn apply_log_tree_event(state: &mut AppState, event: TreeEvent) {
    if state.ui.dialog.is_some() || state.ui.confirm.is_some() {
        return;
    }
    match event {
        TreeEvent::RowClicked { root, branch } | TreeEvent::RowActivated { root, branch } => {
            state.ui.log_ref_scope = Some((root.clone(), branch));
            state.selected_root = Some(root);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        TreeEvent::GroupToggled(group) => match group {
            TreeGroup::Recent => {
                let on = state.ui.log_tree.groups.recent;
                state.ui.log_tree.groups.recent = !on;
            }
            TreeGroup::Local => {
                let on = state.ui.log_tree.groups.local;
                state.ui.log_tree.groups.local = !on;
            }
            TreeGroup::Tags => {
                let on = state.ui.log_tree.groups.tags;
                state.ui.log_tree.groups.tags = !on;
            }
        },
        TreeEvent::RepoToggled { root } => {
            if !state.ui.log_tree.collapsed_repos.remove(&root) {
                state.ui.log_tree.collapsed_repos.insert(root);
            }
        }
        TreeEvent::RemoteToggled { root, remote } => {
            let key = (root.clone(), remote.clone());
            if !state.ui.log_tree.collapsed_remotes.remove(&key) {
                state.ui.log_tree.collapsed_remotes.insert(key);
            }
        }
        TreeEvent::RemoteRevealToggled { .. } => {}
        TreeEvent::FetchRequested { .. } => fetch_scope(state),
        _ => {}
    }
}

fn roots_filter_section(ui: &mut Ui, state: &mut AppState) {
    ui.separator();
    widgets::group_title(ui, "Roots");

    let all_active = state.ui.log_root_filter.is_none();
    let (rect, response) = allocate_row(ui);
    paint_log_row(ui, rect, all_active, response.hovered());
    let galley = ui
        .painter()
        .layout_no_wrap("All roots".to_owned(), body_font(), row_name_ink());
    ui.painter().galley(
        Pos2::new(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        row_name_ink(),
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "All roots"));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        state.ui.log_root_filter = None;
    }

    for (idx, root) in state.multi.roots.iter().enumerate() {
        let active = state.ui.log_root_filter.as_ref() == Some(&root.id);
        let label = format!("Root {}", root.id.name());
        let (rect, response) = allocate_row(ui);
        paint_log_row(ui, rect, active, response.hovered());
        if std::env::var("TG_PROBE_TRACE").as_deref() == Ok("1") {
            eprintln!(
                "[root {label}] pointer={:?} rect={rect:?} hovered={}",
                ui.input(|i| i.pointer.hover_pos()),
                response.hovered()
            );
        }
        let child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let cy = rect.center().y;
        child
            .painter()
            .circle_filled(Pos2::new(rect.left() + 7.0, cy), 3.5, root_color(idx));
        let text_galley =
            child
                .painter()
                .layout_no_wrap(label.clone(), body_font(), row_name_ink());
        child.painter().galley(
            Pos2::new(rect.left() + 16.0, cy - text_galley.size().y / 2.0),
            text_galley,
            row_name_ink(),
        );
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Root {}", root.id.name()))
        });
        widgets::focus_ring(ui, &response);
        if response.clicked() {
            state.ui.log_root_filter = Some(root.id.clone());
        }
    }
}

fn log_state_slot(ui: &mut Ui, state: &LogState) -> Option<LogRecovery> {
    if state.is_default() {
        return None;
    }
    if state.is_error() {
        widgets::inline_error(ui, state.label());
        if let Some(reason) = state.reason() {
            widgets::empty_state(ui, reason);
        }
    } else if state.is_loading() {
        widgets::keyed_read_presentation(
            ui,
            widgets::KeyedReadPresentation::Waiting(state.label()),
        );
    } else {
        widgets::empty_state(ui, state.label());
    }
    let recovery = state.recovery()?;
    let mut chosen = None;
    ui.horizontal(|ui| {
        if components::kit_button(ui, components::KitButton::Secondary, recovery.label()).clicked()
        {
            chosen = Some(recovery);
        }
    });
    chosen
}

fn apply_log_recovery(
    state: &mut AppState,
    recovery: LogRecovery,
    refresh: impl FnOnce(&mut AppState),
) {
    match recovery {
        LogRecovery::Refresh => refresh(state),
        LogRecovery::ResetFilters => state.reset_log_filters(),
        LogRecovery::CommitLocalChanges => state.ui.tab = Tab::Commit,
    }
}

fn hidden_jump_target(state: &AppState, cid: &str) -> Option<RootId> {
    visible_root_ids(state)
        .iter()
        .find(|root| find_commit(state, root, cid).is_some())
        .cloned()
}

fn hidden_jump_band(ui: &mut Ui, state: &mut AppState, root: RootId, cid: CommitId) {
    widgets::empty_state(
        ui,
        "The commit you asked for does not match the current filters.",
    );
    let mut escape: Option<bool> = None;
    ui.horizontal(|ui| {
        if components::kit_button(ui, components::KitButton::Primary, "View and reset filters")
            .clicked()
        {
            escape = Some(true);
        }
        if components::kit_button(ui, components::KitButton::Secondary, "View in Diff").clicked() {
            escape = Some(false);
        }
    });
    match escape {
        None => {}
        Some(true) => {
            state.reset_log_filters();
            state.ui.log_hidden_jump = None;
            state.ui.selected_commit = Some(cid.clone());
            state.ui.log_scroll_to = Some(cid);
        }
        Some(false) => {
            let parent = find_commit(state, &root, &cid).and_then(|c| c.parents.first().cloned());
            state.ui.diff = Some(DiffTarget::commit(root, parent, cid, None));
        }
    }
}

fn graph_pane(ui: &mut Ui, state: &mut AppState) {
    super::rewrite_undo::banner(ui, state);
    widgets::pane_header(ui, "COMMITS", None, |_ui| {});
    ui.horizontal(|ui| {
        widgets::search_input(ui, "Search commits", &mut state.ui.log_filter);
        if let Some(path) = state.ui.log_path_scope.clone() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let remove = ui
                    .small_button(RichText::new("×").size(crate::theme::TYPE_DETAIL_TITLE))
                    .on_hover_text("Remove the path filter");
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, "Remove path filter")
                });
                if remove.clicked() {
                    state.ui.log_path_scope = None;
                }
                ui.label(
                    RichText::new(format!("Path filter: {}", path.display()))
                        .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                        .color(Palette::BRAND),
                );
            });
        }
        if let Some((_, ref_name)) = state.ui.log_ref_scope.clone() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let remove = ui
                    .small_button(RichText::new("×").size(crate::theme::TYPE_DETAIL_TITLE))
                    .on_hover_text("Remove the ref filter");
                remove.widget_info(|| {
                    WidgetInfo::labeled(WidgetType::Button, true, "Remove ref filter")
                });
                if remove.clicked() {
                    state.ui.log_ref_scope = None;
                }
                ui.label(
                    RichText::new(format!("Ref filter: {ref_name}"))
                        .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                        .color(Palette::BRAND),
                );
            });
        }
    });

    let filter_state = log_states::filtering(state);
    if let Some(recovery) = log_state_slot(ui, &filter_state) {
        apply_log_recovery(state, recovery, AppState::refresh_log_listing);
    }

    let display = state.sync_log_display();
    let scroll_to = state.ui.log_scroll_to.take();
    if let Some((_, cid)) = state.ui.log_hidden_jump.clone()
        && commit_index(display.rows(), &cid).is_some()
    {
        state.ui.log_hidden_jump = None;
    }
    let mut missed_jump: Option<String> = None;
    let colors = display.lanes();
    let date_mode = state.settings.date_format;
    let multi_root = shows_root_gutter(state);

    let windows = listed_windows(state);
    let may_have_more = windows.iter().any(|window| window.has_more(state));
    let mut load_more = false;

    if multi_root {
        ui.horizontal_wrapped(|ui| {
            for root in &state.multi.roots {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(crate::theme::MARK_RADIUS),
                    root_color(root_index_of(state, &root.id)),
                );
                ui.label(
                    RichText::new(root.id.name())
                        .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                        .color(Palette::INK_3),
                );
            }
        });
    }

    header_cells(ui, multi_root);

    let mut clicked: Option<(RootId, String)> = None;
    let mut opened: Option<(RootId, String)> = None;
    let viewport_height = ui.available_rect_before_wrap().height();
    let scrolled = ScrollArea::vertical().id_salt("log_commit_list").show_rows(
        ui,
        crate::theme::FILE_ROW_HEIGHT,
        display.len(),
        |ui, visible| {
            if let Some(id) = scroll_to.as_deref() {
                match commit_index(display.rows(), id) {
                    None => {
                        missed_jump = Some(id.to_owned());
                    }
                    Some(index) => {
                        let pitch = crate::theme::FILE_ROW_HEIGHT + ui.spacing().item_spacing.y;
                        let offset = row_offset_from_built(index, visible.clone(), pitch);
                        let row_centre =
                            ui.max_rect().top() + offset + crate::theme::FILE_ROW_HEIGHT / 2.0;
                        let viewport = ui.clip_rect();
                        let view_top = viewport.top() - ui.max_rect().top();
                        if !row_is_in_view(
                            offset,
                            crate::theme::FILE_ROW_HEIGHT,
                            view_top,
                            viewport.height(),
                        ) {
                            let viewport_centre = viewport.top() + viewport.height() / 2.0;
                            ui.scroll_with_delta(Vec2::new(
                                0.0,
                                centre_scroll_delta(row_centre, viewport_centre),
                            ));
                        }
                    }
                }
            }
            for index in visible {
                let Some(c) = display.row(index) else {
                    continue;
                };
                match commit_row(ui, state, c, colors, date_mode, multi_root) {
                    RowIntent::None => {}
                    RowIntent::Select => {
                        if clicked.is_none() {
                            clicked = Some((c.root.clone(), c.id.clone()));
                        }
                    }
                    RowIntent::SelectAndOpenMenu => {
                        if clicked.is_none() {
                            clicked = Some((c.root.clone(), c.id.clone()));
                            opened = Some((c.root.clone(), c.id.clone()));
                        }
                    }
                }
            }
        },
    );

    if let Some(cid) = missed_jump.as_deref()
        && let Some(root) = hidden_jump_target(state, cid)
    {
        state.ui.log_hidden_jump = Some((root, cid.to_owned()));
    }
    let hidden = state.ui.log_hidden_jump.clone();
    if let Some((root, cid)) = hidden {
        hidden_jump_band(ui, state, root, cid);
    } else {
        let list_state = log_states::commit_list(state, display.is_empty());
        if let Some(recovery) = log_state_slot(ui, &list_state) {
            apply_log_recovery(state, recovery, AppState::refresh_log_listing);
        }
    }

    if may_have_more
        && settled_at_bottom(
            scrolled.state.offset.y,
            viewport_height,
            scrolled.content_size.y,
            crate::theme::FILE_ROW_HEIGHT,
        )
    {
        load_more = true;
    }

    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if may_have_more && ui.small_button("Load more").clicked() {
                load_more = true;
            }
            ui.label(
                RichText::new(format!("{} shown", display.len()))
                    .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        });
    });

    if load_more {
        for window in &windows {
            window.load_more(state);
        }
    }
    if let Some((root, id)) = clicked {
        select_commit_row(state, root, id);
        state.ui.log_commit_menu = None;
    }
    if let Some(target) = opened {
        state.ui.log_commit_menu = Some(target);
    }

    commit_context_menu(ui, state);
}

fn select_commit_row(state: &mut AppState, root: RootId, id: String) {
    state.selected_root = Some(root);
    state.ui.selected_commit = Some(id);
    state.ui.log_selected_file = None;
}

enum RowIntent {
    None,
    Select,
    SelectAndOpenMenu,
}

fn commit_facts<'a>(state: &'a AppState, root: &'a RootId, cid: &str) -> CommitFacts<'a> {
    let snapshot = state.multi.by_id(root);
    CommitFacts {
        dirty: snapshot.is_some_and(|r| !r.status.changes.is_empty()),
        protected_branch: snapshot
            .and_then(|r| r.current_branch.clone())
            .is_some_and(|b| sync_service::is_protected(&state.settings, &b)),
        on_current_branch: snapshot.is_some_and(|r| r.current_branch.is_some())
            && state
                .caches
                .log(root)
                .is_some_and(|cs| cs.iter().any(|c| c.id == cid)),
        multi_root: state.multi.roots.len() > 1,
        owner: state.holding_operation(root.as_path()),
        scope: rewrite_gating::scope_of(&state.ui.repo_selection, root),
        repo_name: root
            .as_path()
            .utf8_name()
            .or_else(|| root.as_path().to_str())
            .unwrap_or_default(),
    }
}

fn commit_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some((root_id, cid)) = state.ui.log_commit_menu.clone() else {
        return;
    };
    let target = widgets::menu_host::target_of(&root_id, &cid);
    let ctx = ui.ctx().clone();
    widgets::menu_host::host_row_menu(
        ui,
        state,
        "log_commit",
        target,
        |ui, state| {
            find_commit(state, &root_id, &cid)
                .cloned()
                .and_then(|commit| commit_menu(ui, &commit, &commit_facts(state, &root_id, &cid)))
        },
        |state| state.ui.log_commit_menu = None,
        |state, action| apply_commit_action(state, &ctx, &root_id, &cid, action),
    );
}

fn apply_commit_action(
    state: &mut AppState,
    ctx: &egui::Context,
    root: &RootId,
    cid: &str,
    action: CommitMenuAction,
) {
    state.ui.log_scroll_to = Some(cid.to_owned());
    match action {
        CommitMenuAction::CopyHash => {
            ctx.copy_text(cid.to_owned());
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success(format!(
                "Copied {}",
                widgets::short_commit_ref(cid)
            )));
        }
        CommitMenuAction::CopyMessage => {
            let message = find_commit(state, root, cid)
                .map(|c| c.message.clone())
                .unwrap_or_default();
            ctx.copy_text(message);
            state.ui.toast_shown_at = None;
            state.ui.toast = Some(Toast::success("Copied commit message"));
        }
        CommitMenuAction::CherryPickTo => {
            state.ui.dlg.cherry_pick_commit = Some(cid.to_owned());
            state.ui.dialog = Some(Dialog::CherryPickTarget);
        }
        CommitMenuAction::CherryPickAcross => {
            state.open_cherry_across();
            state.ui.dialog = Some(Dialog::CherryPickAcross);
        }
        CommitMenuAction::RevertCommit => {
            state.ui.confirm = Some(PendingConfirm::RevertCommit {
                commit: cid.to_owned(),
            });
        }
        CommitMenuAction::NewBranch => {
            state.open_new_branch(NewBranchBase::Commit(cid.to_owned()), false, None);
        }
        CommitMenuAction::Checkout => {
            let dirty = state
                .multi
                .by_id(root)
                .is_some_and(|r| !r.status.changes.is_empty());
            if dirty {
                state.ui.confirm = Some(PendingConfirm::CheckoutDirty {
                    root: root.clone(),
                    target: cid.to_owned(),
                    kind: turbogit_domain::model::BranchKind::Local,
                    detach: true,
                });
            } else {
                state.checkout_detached_op(root, cid);
            }
        }
        CommitMenuAction::NewTag => {
            state.open_tag_dialog(Some(cid));
        }
        CommitMenuAction::DropCommit => {
            state.open_rewrite_preflight(root, cid, turbogit_app::state::HistoryVerb::Drop);
        }
        CommitMenuAction::CreatePatch => {
            turbogit_app::create_patch::open_on(
                state,
                root.clone(),
                PatchOrigin::Commit { id: cid.to_owned() },
            );
        }
        CommitMenuAction::RewordCommit => {
            state.open_reword_editor(root, cid);
        }
    }
}

fn body_font() -> FontId {
    crate::theme::chrome_font(crate::theme::TYPE_BODY)
}

fn mono_font() -> FontId {
    FontId::new(MONO_TEXT, FontFamily::Monospace)
}

pub(crate) fn row_name_ink() -> Color32 {
    Palette::INK
}

pub(crate) fn row_meta_ink() -> Color32 {
    Palette::INK_2
}

fn allocate_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(
        Vec2::new(width, crate::theme::FILE_ROW_HEIGHT),
        Sense::click(),
    )
}

fn allocate_file_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(Vec2::new(width, LOG_FILE_ROW_HEIGHT), Sense::click())
}

fn elide(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<Galley> {
    let full = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    if text.is_empty() || full.size().x <= max_width {
        return full;
    }
    let chars = text.chars().count();
    let guess = (chars as f32 * (max_width / full.size().x)) as usize;
    let mut keep = guess.saturating_sub(1).min(chars).max(1);
    loop {
        let cut: String = text.chars().take(keep).collect();
        let galley = painter.layout_no_wrap(format!("{cut}…"), font.clone(), color);
        if galley.size().x <= max_width || keep <= 1 {
            return galley;
        }
        keep -= 1;
    }
}

pub(crate) fn paint_log_row(ui: &Ui, rect: Rect, selected: bool, hovered: bool) {
    components::row_shell(
        ui,
        rect,
        if selected {
            components::RowState::FocusSelected
        } else {
            components::RowState::from_flags(false, hovered)
        },
        components::RowShell::Railed,
    );
}

pub(crate) fn shows_root_gutter(state: &AppState) -> bool {
    state.multi.roots.len() > 1
        && state.ui.log_root_filter.is_none()
        && state.ui.log_path_scope.is_none()
        && state.ui.log_ref_scope.is_none()
}

fn header_cells(ui: &mut Ui, multi_root: bool) {
    let available = ui.available_rect_before_wrap();
    let left = table_content_left(available, multi_root);
    widgets::column_header(
        ui,
        Rect::from_min_max(
            Pos2::new(left, available.top()),
            Pos2::new(available.right(), available.bottom()),
        ),
        &COMMIT_COLUMNS,
    );
}

pub(crate) fn table_content_left(row: Rect, multi_root: bool) -> f32 {
    row.left() + if multi_root { GUTTER } else { 0.0 }
}

fn paint_root_swatch(ui: &Ui, row: Rect, root_index: usize) {
    ui.painter().rect_filled(
        Rect::from_min_size(
            Pos2::new(row.left() + GUTTER - STRIPE_WIDTH, row.top()),
            Vec2::new(STRIPE_WIDTH, row.height()),
        ),
        CornerRadius::ZERO,
        root_color(root_index),
    );
}

fn commit_row(
    ui: &mut Ui,
    state: &AppState,
    c: &Commit,
    colors: &std::collections::HashMap<String, usize>,
    date_mode: DateFormat,
    multi_root: bool,
) -> RowIntent {
    let selected = state.ui.selected_commit.as_deref() == Some(c.id.as_str());
    let (rect, response) = allocate_row(ui);

    paint_log_row(ui, rect, selected, response.hovered());

    if multi_root {
        paint_root_swatch(ui, rect, root_index_of(state, &c.root));
    }
    let content_left = table_content_left(rect, multi_root);

    let lane = colors
        .get(&c.id)
        .map(|i| root_color(*i))
        .unwrap_or(Color32::GRAY);
    let center = Pos2::new(content_left + COL_GRAPH, rect.center().y);
    if c.parents.len() > 1 {
        ui.painter()
            .circle_stroke(center, 4.0, egui::Stroke::new(1.5, lane));
    } else {
        ui.painter().circle_filled(center, 4.0, lane);
    }

    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash_galley =
        painter.layout_no_wrap(widgets::short_commit_ref(&c.id), mono_font(), Palette::LINK);
    painter.galley(
        Pos2::new(content_left + COL_HASH, cy - hash_galley.size().y / 2.0),
        hash_galley,
        Palette::LINK,
    );
    let author_galley =
        painter.layout_no_wrap(truncate(&c.author.name, 10), body_font(), row_meta_ink());
    painter.galley(
        Pos2::new(content_left + COL_AUTHOR, cy - author_galley.size().y / 2.0),
        author_galley,
        row_meta_ink(),
    );

    let date_galley =
        painter.layout_no_wrap(fmt_date(c.time, date_mode), body_font(), row_meta_ink());
    let date_x = rect.right() - DATE_RIGHT_PAD - date_galley.size().x;
    painter.galley(
        Pos2::new(date_x, cy - date_galley.size().y / 2.0),
        date_galley,
        row_meta_ink(),
    );

    // Message cell with one collapsed label pill, the wide column, sitting
    // left of the date. Everything is painted directly (galleys + the pill,
    // no child widgets) so the row itself stays the only interactive surface;
    // a child `ui.label` here would sit on top of the row in hit-testing and
    // swallow its clicks.
    let labels = state.caches.refs_for(&c.root, &c.id);
    let message_left = content_left + COL_MESSAGE;
    let budget = (date_x - 8.0 - if labels.is_empty() { 0.0 } else { PILL_RESERVE } - message_left)
        .max(24.0);
    let subject = c.subject();
    let subject_fit = fit_to_budget(&truncate(subject, 44), budget, &|text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), body_font(), row_name_ink())
            .size()
            .x
    });
    let subject_galley = painter.layout_no_wrap(subject_fit, body_font(), row_name_ink());
    let subject_w = subject_galley.size().x;
    painter.galley(
        Pos2::new(message_left, cy - subject_galley.size().y / 2.0),
        subject_galley,
        row_name_ink(),
    );
    let mx = message_left + subject_w + 6.0;

    if !labels.is_empty() {
        let pill = paint_label_pill(&painter, mx, cy);
        if ui.rect_contains_pointer(pill) {
            // The popup anchors in global space; the row lives inside the
            // graph's scroll area, so map the pill rect to screen first.
            let screen_pill = ui
                .ctx()
                .layer_transform_to_global(ui.layer_id())
                .map(|t| t * pill)
                .unwrap_or(pill);
            Popup::new(
                ui.id().with(("log_label_tooltip", &c.id)),
                ui.ctx().clone(),
                screen_pill,
                ui.layer_id(),
            )
            .kind(PopupKind::Tooltip)
            .gap(4.0)
            .interactable(false)
            .show(|ui| {
                for r in labels {
                    ui.label(
                        RichText::new(&r.name)
                            .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                            .color(ref_kind(r.kind).accent()),
                    );
                }
            });
        }
    }

    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("{} {}", widgets::short_commit_ref(&c.id), subject),
        )
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        RowIntent::Select
    } else if response.secondary_clicked() {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        widgets::menu_host::note_anchor(
            ui,
            widgets::menu_host::MenuId::new(
                "log_commit",
                &widgets::menu_host::target_of(&c.root, &c.id),
            ),
            pos,
        );
        RowIntent::SelectAndOpenMenu
    } else {
        RowIntent::None
    }
}

fn paint_label_pill(painter: &egui::Painter, x: f32, cy: f32) -> Rect {
    const ICON_SIZE: f32 = 12.0;
    let colors = BadgeKind::Neutral.colors();
    let geometry = widgets::CHIP_GEOMETRY;
    let rect = Rect::from_min_size(
        Pos2::new(x, cy - geometry.height / 2.0),
        Vec2::new(ICON_SIZE + geometry.pad_x * 2.0, geometry.height),
    );
    painter.rect_filled(rect, widgets::chip_radius(), colors.bg);
    icons::paint_icon(
        painter,
        Pos2::new(
            rect.center().x - ICON_SIZE / 2.0,
            rect.center().y - ICON_SIZE / 2.0,
        ),
        ICON_SIZE,
        Icon::TAG,
        colors.fg,
    );
    rect
}

enum FileAction {
    None,
    Select(PathBuf),
    OpenDiff(PathBuf),
    OpenBlame(PathBuf),
    ScopeHistory(PathBuf),
    CreatePatch(PathBuf),
    OpenMenu(PathBuf),
    Refresh,
}

fn files_pane(ui: &mut Ui, state: &mut AppState) {
    let selection = state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref());
    let files = selection
        .and_then(|(root, cid)| state.caches.files_for(root, cid))
        .unwrap_or(&[]);
    let stats = selection
        .and_then(|(root, cid)| state.caches.file_stats_for(root, cid))
        .unwrap_or(&[]);

    let count = selection.map(|_| files.len().to_string());
    widgets::pane_header(ui, "CHANGED FILES", count.as_deref(), |_ui| {});

    let Some((root_id, cid)) = selection else {
        widgets::empty_state(ui, "Select a commit to see its changed files.");
        return;
    };

    let parent = find_commit(state, root_id, cid).and_then(|c| c.parents.first().cloned());

    let changes_state = log_states::changed_files(state);

    widgets::search_input(ui, "Filter changed files", &mut state.ui.log_file_filter);
    let filter = state.ui.log_file_filter.trim().to_lowercase();
    let row_shown = |change: &turbogit_domain::model::Change| {
        filter.is_empty() || widgets::filter_matches(&change.path.to_string_lossy(), &filter)
    };
    let shown = files.iter().filter(|c| row_shown(c)).count();

    let mut action = FileAction::None;
    ScrollArea::vertical()
        .id_salt("commit_changed_files")
        .show(ui, |ui| {
            for ch in files {
                if !row_shown(ch) {
                    continue;
                }
                let row_action = file_row(ui, state, root_id, ch, file_stat(stats, &ch.path));
                if matches!(action, FileAction::None) {
                    action = row_action;
                }
            }
            if !changes_state.is_default() {
                if let Some(recovery) = log_state_slot(ui, &changes_state)
                    && recovery == LogRecovery::Refresh
                {
                    action = FileAction::Refresh;
                }
            } else if files.is_empty() {
                widgets::empty_state(ui, "No changed files.");
            } else if shown == 0 {
                widgets::empty_state(ui, "No file matches the filter.");
            }
        });

    if let Some(selected) = state.ui.log_selected_file.clone() {
        ui.separator();
        ui.horizontal(|ui| {
            if ui.link("Open diff").clicked() {
                action = FileAction::OpenDiff(selected.clone());
            }
            ui.label(RichText::new("·").color(Palette::INK_3));
            if ui.link("Blame").clicked() {
                action = FileAction::OpenBlame(selected.clone());
            }
            ui.label(RichText::new("·").color(Palette::INK_3));
            if ui.link("Full path history").clicked() {
                action = FileAction::ScopeHistory(selected);
            }
        });
    }

    match action {
        FileAction::None => {}
        FileAction::Refresh => state.refresh_commit_changes(),
        FileAction::Select(path) => {
            state.ui.log_selected_file = Some(path);
        }
        FileAction::OpenMenu(path) => {
            state.ui.log_file_menu = Some((root_id.clone(), cid.to_owned(), path));
        }
        FileAction::OpenDiff(path) => {
            state.ui.log_selected_file = Some(path.clone());
            state.ui.diff = Some(DiffTarget::commit(
                root_id.clone(),
                parent,
                cid.to_owned(),
                Some(path),
            ));
        }
        FileAction::OpenBlame(path) => {
            state.ui.blame = Some(BlameTarget {
                root: root_id.clone(),
                path,
                rev: cid.to_owned(),
            });
            state.ui.blame_caret_line = None;
        }
        FileAction::ScopeHistory(path) => {
            state.ui.log_path_scope = Some(path);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        FileAction::CreatePatch(path) => {
            turbogit_app::create_patch::open_on(
                state,
                root_id.clone(),
                PatchOrigin::FileRevision {
                    commit: cid.to_owned(),
                    path,
                },
            );
        }
    }

    file_row_context_menu(ui, state);
}

fn file_row_context_menu(ui: &mut Ui, state: &mut AppState) {
    let Some((root_id, cid, path)) = state.ui.log_file_menu.clone() else {
        return;
    };
    let target = widgets::menu_host::target_of(&root_id, &path.to_string_lossy());
    widgets::menu_host::host_row_menu(
        ui,
        state,
        "log_file_row",
        target,
        |ui, state| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.set_min_width(240.0);
            let already_scoped = state.ui.log_path_scope.as_deref() == Some(path.as_path());
            let mut picked = None;
            if menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::FILE_CODE,
                    label: "Show blame",
                    data: None,
                    shortcut: None,
                    state: MenuItemState::enabled(),
                    kind: MenuItemKind::Default,
                },
            )
            .clicked()
            {
                picked = Some(FileAction::OpenBlame(path.clone()));
            }
            let history = menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::CLOCK,
                    label: "Show history for file…",
                    data: None,
                    shortcut: None,
                    state: if already_scoped {
                        MenuItemState::disabled("already showing this file's history")
                    } else {
                        MenuItemState::enabled()
                    },
                    kind: MenuItemKind::Default,
                },
            );
            if history.clicked() {
                picked = Some(FileAction::ScopeHistory(path.clone()));
            }
            if menu_item(
                ui,
                MenuItemProps {
                    icon: Icon::ARCHIVE,
                    label: crate::ui::create_patch::VERB,
                    data: None,
                    shortcut: None,
                    state: MenuItemState::enabled(),
                    kind: MenuItemKind::Default,
                },
            )
            .clicked()
            {
                picked = Some(FileAction::CreatePatch(path.clone()));
            }
            picked
        },
        |state| state.ui.log_file_menu = None,
        |state, action| apply_file_action(state, &root_id, &cid, action),
    );
}

fn apply_file_action(state: &mut AppState, root: &RootId, cid: &CommitId, action: FileAction) {
    match action {
        FileAction::OpenBlame(path) => {
            state.ui.blame = Some(BlameTarget {
                root: root.clone(),
                path,
                rev: cid.to_owned(),
            });
            state.ui.blame_caret_line = None;
        }
        FileAction::ScopeHistory(path) => {
            state.ui.log_path_scope = Some(path);
            state.ui.selected_commit = None;
            state.ui.log_selected_file = None;
        }
        FileAction::CreatePatch(path) => {
            turbogit_app::create_patch::open_on(
                state,
                root.clone(),
                PatchOrigin::FileRevision {
                    commit: cid.to_owned(),
                    path,
                },
            );
        }
        _ => {}
    }
}

fn badge_kind(status: ChangeStatus) -> BadgeKind {
    match status {
        ChangeStatus::Added => BadgeKind::Added,
        ChangeStatus::Deleted => BadgeKind::Deleted,
        ChangeStatus::Modified => BadgeKind::Modified,
        _ => BadgeKind::Neutral,
    }
}

fn file_row(
    ui: &mut Ui,
    state: &AppState,
    root: &RootId,
    change: &turbogit_domain::model::Change,
    stat: Option<(usize, usize)>,
) -> FileAction {
    let selected = state.ui.log_selected_file.as_ref() == Some(&change.path);
    let (rect, response) = allocate_file_row(ui);
    paint_log_row(ui, rect, selected, response.hovered());

    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let mut mx = rect.left() + 4.0;

    let kind = badge_kind(change.status);
    let colors = kind.colors();
    let badge_galley = painter.layout_no_wrap(
        change.status.short().to_owned(),
        FontId::new(TYPE_CONTROL, FontFamily::Proportional),
        colors.fg,
    );
    let badge_rect = Rect::from_min_size(
        Pos2::new(mx, cy - STATUS_PILL.height / 2.0),
        STATUS_PILL.size(&badge_galley),
    );
    STATUS_PILL.paint(&painter, badge_rect, badge_galley, colors.bg, colors.fg);
    mx = badge_rect.right() + 6.0;

    let added = stat.filter(|(ins, _)| *ins > 0).map(|(ins, _)| {
        painter.layout_no_wrap(format!("+{ins}"), mono_font(), Palette::STATE_SUCCESS)
    });
    let removed = stat.filter(|(_, dels)| *dels > 0).map(|(_, dels)| {
        painter.layout_no_wrap(format!("−{dels}"), mono_font(), Palette::STATE_ERROR)
    });
    let mut text_right = rect.right() - 4.0;
    for galley in added.iter().chain(removed.iter()) {
        text_right -= galley.size().x + STAT_GAP;
    }

    let ink = row_name_ink();
    let name = change
        .path
        .display_name_lossy_or(&change.path.to_string_lossy());
    let name_galley = elide(&painter, &name, body_font(), ink, text_right - mx);
    painter.galley(Pos2::new(mx, rect.top() + 7.0), name_galley, ink);

    if let Some(dir) = change.path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let dir_font = FontId::new(TYPE_CONTROL, FontFamily::Proportional);
        let dir_text = forward_slash_path(dir);
        let dir_ink = row_meta_ink();
        let dir_galley = elide(&painter, &dir_text, dir_font, dir_ink, text_right - mx);
        painter.galley(Pos2::new(mx, rect.top() + 22.0), dir_galley, dir_ink);
    }

    let mut churn_right = rect.right() - 4.0;
    if let Some(galley) = removed {
        churn_right -= galley.size().x;
        painter.galley(
            Pos2::new(churn_right, rect.top() + 7.0),
            galley,
            Palette::STATE_ERROR,
        );
        churn_right -= STAT_GAP;
    }
    if let Some(galley) = added {
        churn_right -= galley.size().x;
        painter.galley(
            Pos2::new(churn_right, rect.top() + 7.0),
            galley,
            Palette::STATE_SUCCESS,
        );
    }
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, true, change.path.display().to_string())
    });
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        return FileAction::Select(change.path.clone());
    }
    if response.secondary_clicked() {
        let pos = ui
            .input(|i| i.pointer.interact_pos())
            .unwrap_or_else(|| rect.left_bottom());
        let target = widgets::menu_host::target_of(root, &change.path.to_string_lossy());
        widgets::menu_host::note_anchor(
            ui,
            widgets::menu_host::MenuId::new("log_file_row", &target),
            pos,
        );
        return FileAction::OpenMenu(change.path.clone());
    }
    FileAction::None
}

enum DetailAction {
    None,
    SelectParent(CommitId),
    Refresh,
}

struct DetailRow {
    key: &'static str,
    value: String,
    parents: Vec<CommitId>,
    empty: bool,
}

impl DetailRow {
    fn text(key: &'static str, value: impl Into<String>) -> Self {
        Self {
            key,
            value: value.into(),
            parents: Vec::new(),
            empty: false,
        }
    }

    fn parents(key: &'static str, parents: &[CommitId]) -> Self {
        Self {
            key,
            value: String::new(),
            parents: parents.to_vec(),
            empty: parents.is_empty(),
        }
    }
}

fn commit_detail_rows(commit: &Commit) -> Vec<DetailRow> {
    let sig_suffix = match commit.signature {
        SignatureState::Unsigned => String::new(),
        SignatureState::Good => " · signed ✓".to_string(),
        SignatureState::Bad => " · signature BAD".to_string(),
        SignatureState::Unverified => " · signed (unverified)".to_string(),
    };
    vec![
        DetailRow::text(
            "Author",
            format!("{} <{}>", commit.author.name, commit.author.email),
        ),
        DetailRow::text("Date", fmt_time(commit.time)),
        DetailRow::text(
            "Committer",
            format!(
                "{} <{}>{}",
                commit.committer.name, commit.committer.email, sig_suffix
            ),
        ),
        DetailRow::parents("Parents", &commit.parents),
    ]
}

fn paint_detail_list(ui: &mut Ui, rows: &[DetailRow], action: &mut DetailAction) {
    Grid::new("log_commit_facts")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for row in rows {
                ui.label(micro_text(row.key));
                if row.empty {
                    ui.label(RichText::new("—").font(body_font()).color(Palette::INK_3));
                } else if row.parents.is_empty() {
                    ui.label(
                        RichText::new(&row.value)
                            .font(body_font())
                            .color(Palette::INK),
                    );
                } else {
                    ui.horizontal(|ui| {
                        for parent in &row.parents {
                            if ui
                                .link(
                                    RichText::new(widgets::short_commit_ref(parent))
                                        .font(mono_font())
                                        .color(Palette::INK),
                                )
                                .clicked()
                            {
                                *action = DetailAction::SelectParent(parent.clone());
                            }
                        }
                    });
                }
                ui.end_row();
            }
        });
}

fn details_pane(ui: &mut Ui, state: &mut AppState) {
    let mut action = DetailAction::None;
    widgets::pane_header(ui, "COMMIT DETAILS", None, |_ui| {});
    ui.style_mut().spacing.item_spacing.y = 3.0;

    let details_state = log_states::commit_details(state);
    if let Some(recovery) = log_state_slot(ui, &details_state)
        && recovery == LogRecovery::Refresh
    {
        action = DetailAction::Refresh;
    }

    let Some((root_id, cid)) = state
        .selected_root
        .as_ref()
        .zip(state.ui.selected_commit.as_ref())
    else {
        ui.label(
            RichText::new("Select a commit…")
                .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let Some(commit) = find_commit(state, root_id, cid) else {
        return;
    };

    ui.label(
        RichText::new(commit.subject())
            .font(widgets::bold_font_if_available(ui))
            .color(Palette::INK),
    );

    ui.horizontal(|ui| {
        widgets::hash_chip(ui, &widgets::short_commit_ref(&commit.id), "");
    });

    ui.add_space(4.0);
    paint_detail_list(ui, &commit_detail_rows(commit), &mut action);

    let files = state.caches.files_for(root_id, &commit.id).unwrap_or(&[]);
    let stats = state
        .caches
        .file_stats_for(root_id, &commit.id)
        .unwrap_or(&[]);
    let added: usize = stats.iter().filter_map(|f| f.insertions).sum();
    let removed: usize = stats.iter().filter_map(|f| f.deletions).sum();
    ui.add_space(6.0);
    widgets::churn_bar(ui, added, removed);
    ui.horizontal(|ui| {
        ui.label(micro_text(format!(
            "{} changed",
            components::counted(files.len(), "file", "files")
        )));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if removed > 0 {
                ui.label(
                    RichText::new(format!("−{removed}"))
                        .font(mono_font())
                        .color(Palette::STATE_ERROR),
                );
            }
            if added > 0 {
                ui.label(
                    RichText::new(format!("+{added}"))
                        .font(mono_font())
                        .color(Palette::STATE_SUCCESS),
                );
            }
        });
    });

    let body: Vec<&str> = commit
        .message
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .collect();
    if !body.is_empty() {
        let message_height = ui.available_height().max(40.0);
        ScrollArea::vertical()
            .id_salt("commit_message_body")
            .max_height(message_height)
            .show(ui, |ui| {
                for line in body {
                    ui.label(RichText::new(line).font(body_font()).color(Palette::INK));
                }
            });
    }

    match action {
        DetailAction::SelectParent(parent) => {
            state.ui.selected_commit = Some(parent.clone());
            state.ui.log_selected_file = None;
            state.ui.log_scroll_to = Some(parent);
        }
        DetailAction::Refresh => state.refresh_commit_details(),
        DetailAction::None => {}
    }
}

fn micro_text(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .font(FontId::new(TYPE_CONTROL, FontFamily::Proportional))
        .color(Palette::INK_3)
}

/// Whether a scrolled list sits settled at its bottom: `offset` pixels scrolled
/// from the top of a `content`-tall list inside a `viewport`-tall window, with
/// the last `tolerance` pixels counting as arrived.
///
/// Two rules make this the whole automatic trigger (plan P6): a list that does
/// not overflow its viewport is NEVER at the bottom, there is nothing to
/// scroll to, so a one-screen log never auto-pages, and the test is on where
/// the view rests, not on a scroll gesture, which is what lets the wheel, the
/// scrollbar and the keyboard all fire it.
pub fn settled_at_bottom(offset: f32, viewport: f32, content: f32, tolerance: f32) -> bool {
    content > viewport && offset + tolerance >= content - viewport
}

fn fit_to_budget(text: &str, budget: f32, measure: &dyn Fn(&str) -> f32) -> String {
    if measure(text) <= budget {
        return text.to_owned();
    }
    let prefix = |chars: usize| -> String { text.chars().take(chars).collect() };
    let (mut lo, mut hi) = (1usize, text.chars().count());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if measure(&prefix(mid)) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    prefix(lo)
}

#[cfg(test)]
mod tests {
    use super::{
        centre_scroll_delta, commit_index, fit_to_budget, row_is_in_view, row_offset_from_built,
        settled_at_bottom,
    };
    use std::sync::Arc;
    use turbogit_domain::model::{Commit, RootId, Signature, SignatureState};

    fn row(id: &str) -> turbogit_app::log_display::LogRow {
        Arc::new(Commit {
            id: id.to_owned(),
            parents: vec![],
            author: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time: 0,
            },
            committer: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time: 0,
            },
            message: id.to_owned(),
            time: 0,
            root: RootId(Arc::from(std::path::Path::new("alpha"))),
            signature: SignatureState::Unsigned,
        })
    }

    const ROW_H: f32 = crate::theme::FILE_ROW_HEIGHT;
    const PITCH: f32 = 30.0;

    fn advance(c: char) -> f32 {
        if c == '🎉' {
            20.0
        } else if c.is_ascii() {
            6.0
        } else {
            10.0
        }
    }

    fn width_of(text: &str) -> f32 {
        text.chars().map(advance).sum()
    }

    #[test]
    fn a_subject_that_fits_its_budget_is_returned_whole() {
        let subject = "alpha: second commit";
        let budget = width_of(subject);
        assert_eq!(fit_to_budget(subject, budget, &width_of), subject);
    }

    #[test]
    fn a_subject_far_too_long_is_cut_to_the_widest_prefix_that_fits() {
        let long = "a".repeat(200);
        assert_eq!(fit_to_budget(&long, 60.0, &width_of), "a".repeat(10));
        assert_eq!(fit_to_budget(&long, 59.0, &width_of), "a".repeat(9));
        assert_eq!(fit_to_budget(&long, 1_200.0, &width_of), long);
    }

    #[test]
    fn a_multibyte_subject_is_cut_on_a_char_boundary() {
        let subject = "日本語のコミット🎉done";
        let fitted = fit_to_budget(subject, 60.0, &width_of);
        assert_eq!(
            fitted, "日本語のコミ",
            "six full-width chars is exactly 60px"
        );
        assert!(
            subject.starts_with(&fitted),
            "the fit is a prefix of the subject, got {fitted:?}"
        );
        assert!(
            !fitted.contains('\u{fffd}'),
            "a char was split rather than dropped: {fitted:?}"
        );
        assert_eq!(fit_to_budget(subject, 99.0, &width_of), "日本語のコミット");
        assert_eq!(
            fit_to_budget(subject, 100.0, &width_of),
            "日本語のコミット🎉"
        );
    }

    #[test]
    fn a_single_char_subject_is_never_cut_away_entirely() {
        assert_eq!(fit_to_budget("x", 1.0, &width_of), "x");
        assert_eq!(fit_to_budget("🎉", 1.0, &width_of), "🎉");
    }

    #[test]
    fn an_empty_subject_stays_empty() {
        assert_eq!(fit_to_budget("", 24.0, &width_of), "");
    }

    #[test]
    fn a_subject_at_the_row_cap_costs_a_bounded_number_of_layouts() {
        let long = "a".repeat(44);
        let calls = std::cell::Cell::new(0usize);
        let fitted = fit_to_budget(&long, 60.0, &|text: &str| {
            calls.set(calls.get() + 1);
            width_of(text)
        });
        assert_eq!(fitted, "a".repeat(10), "still the widest prefix that fits");
        assert!(
            calls.get() <= 7,
            "a 44-char subject must not cost a layout per dropped char, got {}",
            calls.get()
        );
    }

    const VIEWPORT: f32 = 300.0;
    const CONTENT: f32 = 1_000.0;
    const BOTTOM: f32 = CONTENT - VIEWPORT;
    const ROW: f32 = 24.0;

    #[test]
    fn a_list_that_fits_its_viewport_is_never_at_the_bottom() {
        assert!(!settled_at_bottom(0.0, VIEWPORT, 120.0, ROW));
        assert!(
            !settled_at_bottom(0.0, VIEWPORT, VIEWPORT, ROW),
            "an exact fit is still not an overflowing list"
        );
    }

    #[test]
    fn an_overflowing_list_arrives_within_one_row_of_its_end() {
        assert!(!settled_at_bottom(0.0, VIEWPORT, CONTENT, ROW));
        assert!(!settled_at_bottom(BOTTOM / 2.0, VIEWPORT, CONTENT, ROW));
        assert!(
            settled_at_bottom(BOTTOM - ROW, VIEWPORT, CONTENT, ROW),
            "one row short still counts as arrived"
        );
        assert!(settled_at_bottom(BOTTOM, VIEWPORT, CONTENT, ROW));
        assert!(
            !settled_at_bottom(BOTTOM - ROW - 1.0, VIEWPORT, CONTENT, ROW),
            "a row and a pixel short does not"
        );
    }

    #[test]
    fn a_row_in_the_built_range_offsets_from_the_first_built_row() {
        assert_eq!(row_offset_from_built(10, 10..30, PITCH), 0.0);
        assert_eq!(row_offset_from_built(29, 10..30, PITCH), 19.0 * PITCH);
    }

    #[test]
    fn a_row_above_the_visible_range_is_a_negative_offset() {
        assert_eq!(
            row_offset_from_built(9, 10..30, PITCH),
            -PITCH,
            "one row above the window is one pitch up"
        );
        assert_eq!(
            row_offset_from_built(0, 10..30, PITCH),
            -10.0 * PITCH,
            "the first row of the window, ten rows above the view, is ten pitches up"
        );
    }

    #[test]
    fn a_row_below_the_visible_range_is_a_positive_offset() {
        assert_eq!(
            row_offset_from_built(30, 10..30, PITCH),
            20.0 * PITCH,
            "the row one past the end of the window is twenty pitches below its first row"
        );
        assert_eq!(
            row_offset_from_built(140, 10..30, PITCH),
            130.0 * PITCH,
            "a row far below the window keeps its distance from the window's start"
        );
    }

    #[test]
    fn the_last_row_of_a_window_is_reachable_from_the_top() {
        assert_eq!(
            row_offset_from_built(152, 0..20, PITCH),
            152.0 * PITCH,
            "the last row of a 153-row window is 152 pitches below the first built row"
        );
    }

    #[test]
    fn the_same_row_scrolls_differently_from_a_different_offset() {
        let row = 100;
        assert_eq!(
            row_offset_from_built(row, 0..20, PITCH),
            100.0 * PITCH,
            "with the window at the top, row 100 is a hundred pitches down"
        );
        assert_eq!(
            row_offset_from_built(row, 95..99, PITCH),
            5.0 * PITCH,
            "with the window five rows above it, the same row is five pitches down"
        );
    }

    #[test]
    fn a_row_inside_the_viewport_is_left_alone() {
        assert!(row_is_in_view(150.0, ROW_H, 100.0, 600.0));
        assert!(row_is_in_view(100.0, ROW_H, 100.0, 600.0));
        assert!(row_is_in_view(600.0 - ROW_H, ROW_H, 100.0, 600.0));
    }

    #[test]
    fn a_row_built_past_the_bottom_edge_is_not_on_screen() {
        let viewport_top = 100.0;
        let viewport_height = 600.0;
        let bottom = viewport_top + viewport_height;
        assert!(!row_is_in_view(
            bottom - ROW_H + 1.0,
            ROW_H,
            viewport_top,
            viewport_height
        ));
        assert!(!row_is_in_view(
            bottom + 10.0,
            ROW_H,
            viewport_top,
            viewport_height
        ));
        assert!(!row_is_in_view(
            viewport_top - ROW_H,
            ROW_H,
            viewport_top,
            viewport_height
        ));
        assert!(row_is_in_view(
            bottom - ROW_H,
            ROW_H,
            viewport_top,
            viewport_height
        ));
    }

    #[test]
    fn an_empty_viewport_shows_no_rows() {
        assert!(!row_is_in_view(0.0, ROW_H, 0.0, 0.0));
    }

    #[test]
    fn a_scroll_delta_points_from_the_row_towards_the_viewport_centre() {
        assert_eq!(
            centre_scroll_delta(100.0, 300.0),
            200.0,
            "a row 200 points ABOVE the middle scrolls back down the history"
        );
        assert_eq!(
            centre_scroll_delta(500.0, 300.0),
            -200.0,
            "a row 200 points BELOW the middle scrolls forward"
        );
        assert_eq!(
            centre_scroll_delta(300.0, 300.0),
            0.0,
            "centred needs no scroll"
        );
    }

    #[test]
    fn a_commit_index_is_its_position_in_the_held_window() {
        let rows: Vec<_> = ["a", "b", "c"].into_iter().map(row).collect();
        assert_eq!(commit_index(&rows, "a"), Some(0));
        assert_eq!(commit_index(&rows, "c"), Some(2));
        assert_eq!(
            commit_index(&rows, "not-in-the-window"),
            None,
            "a commit outside the window has no index to scroll to"
        );
        assert_eq!(
            commit_index(&[], "a"),
            None,
            "an empty window scrolls nowhere"
        );
    }
}
