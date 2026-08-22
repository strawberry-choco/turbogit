//! Git Log four-pane workspace (issue #12, spec §8.3): branches pane,
//! graph pane, changed-files pane and commit-details pane — plus the legacy
//! History tab (H5–H8) it shares this module with.
//!
//! Layout (spec §8.3):
//! 1. **Branches** (left, 210px): live search; LOCAL / REMOTE / TAGS groups
//!    fed by ref decorations; a bottom `ROOTS` filter for multi-root projects.
//! 2. **Graph** (center): live search toolbar, root-stripe legend, and the
//!    commit table (Graph | Hash | Author | Date | Message) with inline ref
//!    chips (branch=brand, remote=success, tag=warning) and a translucent
//!    `SELECTION_BG` row highlight that keeps lane colors readable.
//! 3. **Changed files** (right-top, 320px): the selected commit's files with
//!    status badges; clicking loads the diff.
//! 4. **Commit details** (right-bottom, ~200px SURFACE): key-value hash /
//!    author / date / parents plus the full message below.

use crate::model::{ChangeStatus, Commit, CommitRef, DateFormat, GitRefKind, RootId};
use crate::state::{AppState, DiffTarget};
use crate::theme::Palette;
use crate::ui::icons;
use crate::ui::icons::Icon;
use crate::ui::widgets::{self, BadgeKind, RefKind};
use chrono::{DateTime, Local, TimeZone, Utc};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin, Panel, Pos2, Rect,
    Response, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};

// --- Pane metrics (spec §8.3) -------------------------------------------------

/// Branches pane width.
const BRANCHES_WIDTH: f32 = 210.0;
/// Right column (changed files + details) width.
const FILES_WIDTH: f32 = 320.0;
/// Commit details pane height.
const DETAILS_HEIGHT: f32 = 200.0;
/// Commit table row height.
const ROW_HEIGHT: f32 = 24.0;
/// Root stripe width on multi-root rows.
const STRIPE_WIDTH: f32 = 3.0;
/// Uppercase micro text (§3.3).
const MICRO_TEXT: f32 = 11.0;
/// Mono cell font size.
const MONO_TEXT: f32 = 12.0;
/// Chip metrics — mirrors `widgets::chip` (`.tg-label` pills).
const CHIP_HEIGHT: f32 = 18.0;
const CHIP_PAD_X: f32 = 6.0;
const CHIP_GAP: f32 = 4.0;

/// Distinct lane colors for the commit graph (Epic D1). Also reused as the
/// deterministic per-root stripe palette.
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

fn fmt_time(t: i64) -> String {
    match Utc.timestamp_opt(t, 0) {
        chrono::LocalResult::Single(dt) => {
            let local: DateTime<Local> = DateTime::from(dt);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        _ => String::new(),
    }
}

/// Render a timestamp according to the configured format (Epic D2).
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

/// Truncate a string to at most `n` chars without panicking on multibyte input.
fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn short(id: &str) -> String {
    truncate(id, 7)
}

/// Deterministic color for the root at `idx` (stripes + legend).
fn root_color(idx: usize) -> Color32 {
    GRAPH_COLORS[idx % GRAPH_COLORS.len()]
}

/// Assign each commit a lane color using a lightweight DAG walk so the list
/// reads like a commit graph (Epic D1). Newest-first input assumed.
fn assign_colors(commits: &[Commit]) -> std::collections::HashMap<String, usize> {
    use std::collections::HashMap;
    let mut color_of: HashMap<String, usize> = HashMap::new();
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut next_color = 0usize;
    for c in commits {
        let idx = lanes
            .iter()
            .position(|l| l.as_deref() == Some(&c.id))
            .unwrap_or_else(|| {
                if let Some(e) = lanes.iter().position(|l| l.is_none()) {
                    e
                } else {
                    lanes.push(None);
                    lanes.len() - 1
                }
            });
        color_of.entry(c.id.clone()).or_insert_with(|| {
            // pick the lane's color, allocating a new one if needed
            if idx < lanes.len() && lanes[idx].is_none() {
                let c = next_color;
                next_color += 1;
                c
            } else {
                idx
            }
        });
        lanes[idx] = c.parents.first().cloned();
        for p in c.parents.iter().skip(1) {
            if let Some(e) = lanes.iter_mut().find(|l| l.is_none()) {
                e.replace(p.clone());
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            } else {
                lanes.push(Some(p.clone()));
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            }
        }
    }
    color_of
}

// --- Data plumbing ------------------------------------------------------------

/// The roots whose history is displayed (roots-filter aware).
fn visible_root_ids(state: &AppState) -> Vec<RootId> {
    match &state.ui.log_root_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    }
}

/// Lazily load (through the engine seam, cached) everything the four panes
/// need beyond the log itself: ref decorations per visible root and the
/// changed-file list of the selected commit.
fn ensure_log_data(state: &mut AppState) {
    for id in visible_root_ids(state) {
        if !state.ref_cache.contains_key(&id) {
            let deco = state.executor.ref_decorations(&id.0).unwrap_or_default();
            state.ref_cache.insert(id, deco.into_iter().collect());
        }
    }
    if let (Some(root), Some(cid)) = (
        state.selected_root.clone(),
        state.ui.selected_commit.clone(),
    ) {
        let key = (root.clone(), cid.clone());
        if !state.files_cache.contains_key(&key) {
            let files = state
                .executor
                .commit_files(&root.0, &cid)
                .unwrap_or_default();
            state.files_cache.insert(key, files);
        }
    }
}

/// The commits currently displayed: union across visible roots (newest first,
/// live-filtered by the graph search box).
fn visible_commits(state: &AppState) -> Vec<Commit> {
    let mut commits: Vec<Commit> = visible_root_ids(state)
        .iter()
        .filter_map(|id| state.log_cache.get(id))
        .flatten()
        .cloned()
        .collect();
    commits.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
    let filter = state.ui.log_filter.to_lowercase();
    if filter.is_empty() {
        return commits;
    }
    commits.retain(|c| {
        c.message.to_lowercase().contains(&filter)
            || c.id.to_lowercase().contains(&filter)
            || c.author.name.to_lowercase().contains(&filter)
    });
    commits
}

fn refs_for(state: &AppState, root: &RootId, cid: &str) -> Vec<CommitRef> {
    state
        .ref_cache
        .get(root)
        .and_then(|m| m.get(cid))
        .cloned()
        .unwrap_or_default()
}

fn ref_kind(kind: GitRefKind) -> RefKind {
    match kind {
        GitRefKind::Branch => RefKind::Branch,
        GitRefKind::Remote => RefKind::Remote,
        GitRefKind::Tag => RefKind::Tag,
    }
}

// --- Composition ----------------------------------------------------------------

pub fn show_log(ui: &mut Ui, state: &mut AppState) {
    ensure_log_data(state);

    // Pane 1 — branches (left, 210px).
    Panel::left("log_branches_pane")
        .exact_size(BRANCHES_WIDTH)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(Margin::same(8)),
        )
        .show(ui, |ui| branches_pane(ui, state));

    // Panes 3+4 — right column: changed files on top, details pinned below.
    Panel::right("log_right_column")
        .exact_size(FILES_WIDTH)
        .resizable(false)
        .frame(Frame::new().fill(Palette::BG))
        .show(ui, |ui| {
            Panel::bottom("log_details_pane")
                .exact_size(DETAILS_HEIGHT)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(Palette::SURFACE)
                        .inner_margin(Margin::same(8)),
                )
                .show(ui, |ui| details_pane(ui, state));
            files_pane(ui, state);
        });

    // Pane 2 — graph fills the remainder.
    graph_pane(ui, state);
}

// --- Pane 1: branches -------------------------------------------------------------

fn branches_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::toolwindow_header(ui, "Branches", |_ui| {});
    ui.add_space(2.0);
    widgets::search_input(ui, "Search branches", &mut state.ui.log_branch_filter);
    ui.add_space(4.0);

    // Union decorations across the visible roots, deduplicated by name+kind.
    let ids = visible_root_ids(state);
    let mut local: Vec<String> = Vec::new();
    let mut remote: Vec<String> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    for id in &ids {
        let Some(map) = state.ref_cache.get(id) else {
            continue;
        };
        for refs in map.values() {
            for r in refs {
                let bucket = match r.kind {
                    GitRefKind::Branch => &mut local,
                    GitRefKind::Remote => &mut remote,
                    GitRefKind::Tag => &mut tags,
                };
                if !bucket.contains(&r.name) {
                    bucket.push(r.name.clone());
                }
            }
        }
    }

    let filter = state.ui.log_branch_filter.to_lowercase();
    let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);

    ScrollArea::vertical().show(ui, |ui| {
        widgets::group_title(ui, "Local");
        for name in local.iter().filter(|n| matches(n)) {
            let current = ids.iter().any(|id| {
                state
                    .multi
                    .by_id(id)
                    .and_then(|r| r.current_branch.clone())
                    .as_deref()
                    == Some(name.as_str())
            });
            branch_row(ui, name, Icon::GIT_BRANCH, current);
        }

        ui.add_space(6.0);
        widgets::group_title(ui, "Remote");
        for name in remote.iter().filter(|n| matches(n)) {
            branch_row(ui, name, Icon::FOLDER_GIT, false);
        }

        ui.add_space(6.0);
        widgets::group_title(ui, "Tags");
        for name in tags.iter().filter(|n| matches(n)) {
            branch_row(ui, name, Icon::TAG, false);
        }

        ui.add_space(8.0);
        roots_filter_section(ui, state);
    });
}

/// One row of the branches pane (LOCAL / REMOTE / TAGS). Live-filtered by the
/// caller; the current branch gets a brand-tinted icon and emphasized ink.
fn branch_row(ui: &mut Ui, name: &str, icon: Icon, current: bool) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, 13.0, Palette::INK_3);
    child.add_space(4.0);
    let ink = if current {
        Palette::INK
    } else {
        Palette::INK_2
    };
    child.label(
        RichText::new(name)
            .font(micro_or_body_font(current))
            .color(ink),
    );
    if current {
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            icons::icon(ui, Icon::CHECK, 13.0, Palette::BRAND);
        });
    }
}

fn micro_or_body_font(emphasized: bool) -> FontId {
    FontId::new(
        if emphasized { 12.0 } else { MICRO_TEXT },
        FontFamily::Proportional,
    )
}

/// Bottom-of-pane ROOTS filter (multi-root): All roots / per-root rows.
fn roots_filter_section(ui: &mut Ui, state: &mut AppState) {
    ui.separator();
    widgets::group_title(ui, "Roots");

    let all_active = state.ui.log_root_filter.is_none();
    let (rect, response) = allocate_row(ui);
    paint_row_fill(ui, &rect, all_active, response.hovered());
    // Painted via galley (not `ui.label`) so the row's widget label stays the
    // only accessibility node carrying "All roots".
    let galley =
        ui.painter()
            .layout_no_wrap("All roots".to_owned(), body_font(), row_ink(all_active));
    ui.painter().galley(
        Pos2::new(rect.left() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        row_ink(all_active),
    );
    response
        .widget_info(move || WidgetInfo::labeled(WidgetType::Button, true, "All roots".to_owned()));
    if response.clicked() {
        state.ui.log_root_filter = None;
    }

    for (idx, root) in state.multi.roots.iter().enumerate() {
        let active = state.ui.log_root_filter.as_ref() == Some(&root.id);
        let label = format!("Root {}", root.id.name());
        let (rect, response) = allocate_row(ui);
        paint_row_fill(ui, &rect, active, response.hovered());
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
        // Root stripe dot in the root's color.
        let cy = rect.center().y;
        child
            .painter()
            .circle_filled(Pos2::new(rect.left() + 7.0, cy), 3.5, root_color(idx));
        // Galley text again: keep the widget label unique in the tree.
        let text_galley =
            child
                .painter()
                .layout_no_wrap(label.clone(), body_font(), row_ink(active));
        child.painter().galley(
            Pos2::new(rect.left() + 16.0, cy - text_galley.size().y / 2.0),
            text_galley,
            row_ink(active),
        );
        let closure_label = label.clone();
        response.widget_info(move || {
            WidgetInfo::labeled(WidgetType::Button, true, closure_label.clone())
        });
        if response.clicked() {
            state.ui.log_root_filter = Some(root.id.clone());
        }
    }
}

// --- Pane 2: graph ------------------------------------------------------------------

fn graph_pane(ui: &mut Ui, state: &mut AppState) {
    // Filter toolbar: live search over message / hash / author.
    ui.horizontal(|ui| {
        widgets::search_input(ui, "Search commits", &mut state.ui.log_filter);
    });

    let commits = visible_commits(state);
    let colors = assign_colors(&commits);
    let date_mode = state.settings.date_format;
    let multi_root = state.multi.roots.len() > 1 && state.ui.log_root_filter.is_none();

    // Root-stripe legend chip row (11px INK_3) for multi-root setups.
    if multi_root {
        ui.horizontal_wrapped(|ui| {
            for (idx, root) in state.multi.roots.iter().enumerate() {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(2), root_color(idx));
                ui.label(
                    RichText::new(root.id.name())
                        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                        .color(Palette::INK_3),
                );
            }
        });
    }

    // Column micro-headers aligned with the cells below.
    let left = ui.cursor().left() + STRIPE_WIDTH;
    header_cells(ui, left);

    ScrollArea::vertical().show(ui, |ui| {
        for c in &commits {
            commit_row(ui, state, c, &colors, date_mode, multi_root);
        }
        if commits.is_empty() {
            ui.label(
                RichText::new("No commits match.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
    });
}

fn body_font() -> FontId {
    FontId::new(12.0, FontFamily::Proportional)
}

fn mono_font() -> FontId {
    FontId::new(MONO_TEXT, FontFamily::Monospace)
}

fn row_ink(active: bool) -> Color32 {
    if active {
        Palette::INK
    } else {
        Palette::INK_2
    }
}

fn allocate_row(ui: &mut Ui) -> (Rect, Response) {
    let width = ui.available_width();
    ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click())
}

/// Row fill decision: active rows keep the translucent selection token,
/// hovered rows take SURFACE_2, idle rows stay transparent.
fn paint_row_fill(ui: &Ui, rect: &Rect, active: bool, hovered: bool) {
    let fill = if active {
        Palette::selection_bg()
    } else if hovered {
        Palette::SURFACE_2
    } else {
        return;
    };
    ui.painter().rect_filled(*rect, CornerRadius::same(4), fill);
}

/// Micro column headers above the commit table.
fn header_cells(ui: &mut Ui, left: f32) {
    let top = ui.cursor().top();
    ui.add_space(16.0);
    let headers = [
        ("GRAPH", 16.0),
        ("HASH", 68.0),
        ("AUTHOR", 148.0),
        ("DATE", 228.0),
        ("MESSAGE", 308.0),
    ];
    for (title, dx) in headers {
        let galley = ui.painter().layout_no_wrap(
            title.to_owned(),
            FontId::new(MICRO_TEXT, FontFamily::Proportional),
            Palette::INK_3,
        );
        ui.painter()
            .galley(Pos2::new(left + dx, top + 2.0), galley, Palette::INK_3);
    }
}

/// One commit-table row: stripe | node | hash | author | date | message(+chips).
fn commit_row(
    ui: &mut Ui,
    state: &mut AppState,
    c: &Commit,
    colors: &std::collections::HashMap<String, usize>,
    date_mode: DateFormat,
    multi_root: bool,
) {
    let selected = state.ui.selected_commit.as_deref() == Some(c.id.as_str());
    let (rect, response) = allocate_row(ui);

    // Translucent selection (SELECTION_BG) keeps lane colors readable (§7.2).
    if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), Palette::selection_bg());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), Palette::SURFACE_2);
    }

    // Root stripe (multi-root only).
    let mut content_left = rect.left();
    if multi_root {
        let idx = state
            .multi
            .roots
            .iter()
            .position(|r| r.id == c.root)
            .unwrap_or(0);
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.top()),
                Vec2::new(STRIPE_WIDTH, rect.height()),
            ),
            CornerRadius::ZERO,
            root_color(idx),
        );
        content_left += STRIPE_WIDTH + 2.0;
    }

    // Graph cell: colored lane node (ring for merges).
    let lane = colors
        .get(&c.id)
        .map(|i| root_color(*i))
        .unwrap_or(Color32::GRAY);
    let center = Pos2::new(content_left + 10.0, rect.center().y);
    if c.parents.len() > 1 {
        ui.painter()
            .circle_stroke(center, 4.0, egui::Stroke::new(1.5, lane));
    } else {
        ui.painter().circle_filled(center, 4.0, lane);
    }

    // Hash | Author | Date cells.
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let hash_galley = painter.layout_no_wrap(short(&c.id), mono_font(), Palette::BRAND);
    painter.galley(
        Pos2::new(content_left + 26.0, cy - hash_galley.size().y / 2.0),
        hash_galley,
        Palette::BRAND,
    );
    let author_galley =
        painter.layout_no_wrap(truncate(&c.author.name, 10), body_font(), Palette::INK_2);
    painter.galley(
        Pos2::new(content_left + 84.0, cy - author_galley.size().y / 2.0),
        author_galley,
        Palette::INK_2,
    );
    let date_galley =
        painter.layout_no_wrap(fmt_date(c.time, date_mode), body_font(), Palette::INK_3);
    painter.galley(
        Pos2::new(content_left + 164.0, cy - date_galley.size().y / 2.0),
        date_galley,
        Palette::INK_3,
    );

    // Message cell with inline ref chips. Everything is painted directly
    // (galleys + pills, no child widgets) so the row itself stays the only
    // interactive surface — a child `ui.label` here would sit on top of the
    // row in hit-testing and swallow its clicks.
    let painter = ui.painter().clone();
    let mut mx = content_left + 236.0;
    let subject = c.message.lines().next().unwrap_or("");
    let subject_galley = painter.layout_no_wrap(truncate(subject, 44), body_font(), Palette::INK);
    let subject_w = subject_galley.size().x;
    painter.galley(
        Pos2::new(mx, cy - subject_galley.size().y / 2.0),
        subject_galley,
        Palette::INK,
    );
    mx += subject_w + 6.0;
    for r in refs_for(state, &c.root, &c.id) {
        mx = paint_ref_chip(&painter, r.name.clone(), ref_kind(r.kind), mx, cy);
    }

    let closure_label = format!("{} {}", short(&c.id), subject);
    response
        .widget_info(move || WidgetInfo::labeled(WidgetType::Button, true, closure_label.clone()));
    if response.clicked() {
        state.ui.selected_commit = Some(c.id.clone());
        state.ui.log_selected_file = None;
    }
}

/// Paint one `.tg-label` pill (18px, kind colors) at `(x, cy)` and return the
/// x position after it. Painter-only: registers no widget (see commit_row).
fn paint_ref_chip(painter: &egui::Painter, text: String, kind: RefKind, x: f32, cy: f32) -> f32 {
    let colors = kind.colors();
    let font = FontId::new(MICRO_TEXT, FontFamily::Proportional);
    let galley = painter.layout_no_wrap(text, font, colors.fg);
    let rect = Rect::from_min_size(
        Pos2::new(x, cy - CHIP_HEIGHT / 2.0),
        Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT),
    );
    painter.rect_filled(
        rect,
        CornerRadius::same((CHIP_HEIGHT / 2.0) as u8),
        colors.bg,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            cy - galley.size().y / 2.0,
        ),
        galley,
        colors.fg,
    );
    rect.right() + CHIP_GAP
}

// --- Pane 3: changed files -----------------------------------------------------------

fn files_pane(ui: &mut Ui, state: &mut AppState) {
    let files = state
        .selected_root
        .clone()
        .zip(state.ui.selected_commit.clone())
        .and_then(|(root, cid)| state.files_cache.get(&(root, cid)))
        .cloned()
        .unwrap_or_default();

    widgets::toolwindow_header(ui, &format!("Changed files ({})", files.len()), |_ui| {});

    let Some((root_id, cid)) = state
        .selected_root
        .clone()
        .zip(state.ui.selected_commit.clone())
    else {
        ui.label(
            RichText::new("Select a commit to see its changed files.")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let parent = state
        .log_cache
        .get(&root_id)
        .and_then(|cs| cs.iter().find(|c| c.id == cid))
        .and_then(|c| c.parents.first().cloned());

    ScrollArea::vertical().show(ui, |ui| {
        for ch in &files {
            file_row(ui, state, ch, &root_id, &cid, parent.clone());
        }
        if files.is_empty() {
            ui.label(
                RichText::new("No changed files.")
                    .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                    .color(Palette::INK_3),
            );
        }
    });
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
    state: &mut AppState,
    change: &crate::model::Change,
    root_id: &RootId,
    cid: &str,
    parent: Option<String>,
) {
    let path_str = change.path.display().to_string();
    let selected = state.ui.log_selected_file.as_ref() == Some(&change.path);
    let (rect, response) = allocate_row(ui);
    paint_row_fill(ui, &rect, selected, response.hovered());

    // Painter-only contents so the row owns the pointer (see commit_row).
    let painter = ui.painter().clone();
    let cy = rect.center().y;
    let mut mx = rect.left() + 4.0;

    // Status badge pill (mirrors `widgets::badge` metrics).
    let kind = badge_kind(change.status);
    let colors = kind.colors();
    let badge_galley = painter.layout_no_wrap(
        change.status.short().to_owned(),
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        colors.fg,
    );
    let badge_rect = Rect::from_min_size(
        Pos2::new(mx, cy - CHIP_HEIGHT / 2.0),
        Vec2::new(badge_galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT),
    );
    painter.rect_filled(badge_rect, CornerRadius::same(4), colors.bg);
    painter.galley(
        Pos2::new(
            badge_rect.center().x - badge_galley.size().x / 2.0,
            cy - badge_galley.size().y / 2.0,
        ),
        badge_galley,
        colors.fg,
    );
    mx = badge_rect.right() + 6.0;

    let path_galley =
        painter.layout_no_wrap(truncate(&path_str, 34), body_font(), row_ink(selected));
    painter.galley(
        Pos2::new(mx, cy - path_galley.size().y / 2.0),
        path_galley,
        row_ink(selected),
    );

    let closure_label = path_str.clone();
    response
        .widget_info(move || WidgetInfo::labeled(WidgetType::Button, true, closure_label.clone()));
    if response.clicked() {
        state.ui.log_selected_file = Some(change.path.clone());
        state.ui.diff = Some(DiffTarget {
            root: root_id.clone(),
            left: parent,
            right: Some(cid.to_owned()),
            path: Some(change.path.clone()),
        });
    }
}

// --- Pane 4: commit details -----------------------------------------------------------

fn details_pane(ui: &mut Ui, state: &mut AppState) {
    widgets::group_title(ui, "Commit details");
    // Compact vertical rhythm so the full message fits the 200px pane.
    ui.style_mut().spacing.item_spacing.y = 3.0;

    let Some((root_id, cid)) = state
        .selected_root
        .clone()
        .zip(state.ui.selected_commit.clone())
    else {
        ui.label(
            RichText::new("Select a commit…")
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
        return;
    };

    let Some(commit) = state
        .log_cache
        .get(&root_id)
        .and_then(|cs| cs.iter().find(|c| c.id == cid))
        .cloned()
    else {
        return;
    };

    // Key-value block (compact single rows — the pane is 200px tall).
    ui.horizontal(|ui| {
        kv_label(ui, "Hash:");
        ui.label(
            RichText::new(short(&commit.id))
                .font(mono_font())
                .color(Palette::BRAND),
        );
    });
    kv_value(
        ui,
        "Author:",
        format!("{} <{}>", commit.author.name, commit.author.email),
    );
    kv_value(ui, "Date:", fmt_time(commit.time));

    ui.horizontal(|ui| {
        kv_label(ui, "Parents:");
        if commit.parents.is_empty() {
            ui.label(RichText::new("—").font(body_font()).color(Palette::INK_3));
        } else {
            for p in &commit.parents {
                ui.label(
                    RichText::new(short(p))
                        .font(mono_font())
                        .color(Palette::INK),
                );
            }
        }
    });

    // File summary badges ("2 modified · 1 added").
    let files = state
        .files_cache
        .get(&(root_id, commit.id.clone()))
        .cloned()
        .unwrap_or_default();
    let modified = files
        .iter()
        .filter(|f| f.status == ChangeStatus::Modified)
        .count();
    let added = files
        .iter()
        .filter(|f| f.status == ChangeStatus::Added)
        .count();
    let deleted = files
        .iter()
        .filter(|f| f.status == ChangeStatus::Deleted)
        .count();
    let summary = [modified, added, deleted]
        .into_iter()
        .zip(["modified", "added", "deleted"])
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect::<Vec<_>>()
        .join(" · ");
    if !summary.is_empty() {
        ui.label(
            RichText::new(summary)
                .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                .color(Palette::INK_3),
        );
    }

    // Full message below the key-value block.
    ScrollArea::vertical().show(ui, |ui| {
        for line in commit.message.lines().filter(|l| !l.trim().is_empty()) {
            ui.label(RichText::new(line).font(body_font()).color(Palette::INK));
        }
    });
}

fn kv_label(ui: &mut Ui, key: &str) {
    ui.label(
        RichText::new(key)
            .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
            .color(Palette::INK_3),
    );
}

fn kv_value(ui: &mut Ui, key: &str, value: impl Into<String>) {
    ui.horizontal(|ui| {
        kv_label(ui, key);
        ui.label(
            RichText::new(value.into())
                .font(body_font())
                .color(Palette::INK),
        );
    });
}

// --- Legacy History tab (unchanged behavior, H5–H8) -----------------------------------

pub fn show_history(ui: &mut Ui, state: &mut AppState) {
    ui.heading("File / Selection History");
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut state.ui.history_path);
        if ui.button("Show history").clicked() {
            if let Some(id) = &state.selected_root {
                let id = id.clone();
                let path = std::path::PathBuf::from(state.ui.history_path.trim());
                let executor = state.executor.clone();
                let tx = state.tx.clone();
                std::thread::spawn(move || {
                    let res = executor.log(
                        &id.0,
                        &crate::model::LogOpts {
                            path: Some(path),
                            ..Default::default()
                        },
                    );
                    let _ = tx.send(crate::engine::AppEvent::LogLoaded {
                        root: id,
                        commits: res,
                    });
                });
            }
        }
        if ui.button("Blame").clicked() {
            let root = state.selected_path();
            let path = std::path::PathBuf::from(state.ui.history_path.trim());
            if let Some(r) = root {
                match state.executor.blame(&r, &path, None) {
                    Ok(lines) => {
                        for l in lines.iter().take(200) {
                            ui.colored_label(
                                Color32::from_gray(150),
                                format!(
                                    "{} {} {}",
                                    &l.commit[..7.min(l.commit.len())],
                                    l.author,
                                    l.content
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        ui.colored_label(Color32::RED, e.to_string());
                    }
                }
            }
        }
    });
    // File history reuses the log cache for the selected root (path-scoped).
    let commits = state
        .selected_root
        .as_ref()
        .and_then(|id| state.log_cache.get(id).cloned())
        .unwrap_or_default();
    ui.separator();
    ui.label(format!("{} commits touching this path", commits.len()));
    ScrollArea::vertical().show(ui, |ui| {
        for c in &commits {
            let first = c.message.lines().next().unwrap_or("").to_string();
            if ui
                .selectable_label(false, format!("{}  {}", short(&c.id), first))
                .clicked()
            {
                state.ui.diff = Some(DiffTarget {
                    root: state.selected_root.clone().unwrap(),
                    left: c.parents.first().cloned(),
                    right: Some(c.id.clone()),
                    path: Some(std::path::PathBuf::from(state.ui.history_path.trim())),
                });
            }
        }
    });
    if let Some(d) = state.ui.diff.clone() {
        if d.path.is_some() {
            crate::ui::diff::render_diff(ui, state, &d.left, &d.right, &d.path);
        }
    }
}
