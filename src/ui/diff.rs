//! Diff viewer (spec §8.4, issue #13).
//!
//! Restyled onto the central [`crate::theme::Palette`] tokens and the shared
//! widget vocabulary — behavior preserved, visual migration only:
//!
//! - **Async + cached** engine access through the [`GitExecutor`] seam
//!   (Epic E7/J1): diffs are computed on a worker thread and cached, so no
//!   `git diff` runs synchronously per frame.
//! - **Segmented control** toggles Side-by-Side | Unified rendering.
//! - **Revision chips** select the working-tree comparison pair:
//!   Repo = HEAD↔worktree, Staged = HEAD↔index, Local = index↔worktree.
//!   Explicit commit-to-commit targets (Git Log) keep their fixed pair and
//!   hide the chips.
//! - **Hunk navigation** ‹ n/N › steps between parsed hunks.
//! - **Ignore whitespace** feeds `DiffOpts::ignore_whitespace` into the
//!   engine call and the cache key.
//! - Add/del lines paint token-exact backgrounds (`DIFF_ADD_BG` /
//!   `DIFF_DEL_BG`) with muted `INK_3` gutter numbers; hunk headers sit on
//!   SURFACE (spec §2.3).

use crate::engine::AppEvent;
use crate::model::DiffOpts;
use crate::state::{AppState, DiffComparison};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Response, ScrollArea,
    Sense, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::sync::Arc;

// --- metrics -----------------------------------------------------------------

/// Rendered height of one diff line.
const ROW_H: f32 = 22.0;
/// Side-by-side pane header height (spec §8.4).
const PANE_HEADER_H: f32 = 28.0;
/// Width of the +/- sign column in a unified row.
const SIGN_W: f32 = 16.0;
/// Width of the line-number gutter column.
const NUM_W: f32 = 40.0;
/// X offset of the code text within a unified row.
const TEXT_X: f32 = SIGN_W + NUM_W + 12.0;

fn mono_font() -> FontId {
    FontId::new(12.0, FontFamily::Monospace)
}

// --- row model ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowKind {
    Meta,
    Hunk,
    Context,
    Del,
    Add,
}

struct Row {
    kind: RowKind,
    text: String,
    /// Hunk index (Hunk rows only).
    hunk: usize,
    /// 1-based old-file line number (0 when not applicable).
    old_no: usize,
    /// 1-based new-file line number (0 when not applicable).
    new_no: usize,
}

/// `@@ -a,b +c,d @@` → `(a, c)` (defaults to 1 when absent).
fn hunk_starts(header: &str) -> (usize, usize) {
    let mut old = 1usize;
    let mut new = 1usize;
    for tok in header.split_whitespace() {
        if let Some(rest) = tok.strip_prefix('-') {
            if let Some(n) = rest.split(',').next().and_then(|v| v.parse().ok()) {
                old = n;
            }
        } else if let Some(rest) = tok.strip_prefix('+') {
            if let Some(n) = rest.split(',').next().and_then(|v| v.parse().ok()) {
                new = n;
            }
        }
    }
    (old, new)
}

/// Parse unified-diff text into renderable rows, tracking 1-based line
/// numbers from each hunk header so gutters can show real positions.
fn parse(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut hunk = 0usize;
    let mut old_no = 1usize;
    let mut new_no = 1usize;
    for line in text.lines() {
        if line.starts_with("@@") {
            let (o, n) = hunk_starts(line);
            old_no = o;
            new_no = n;
            rows.push(Row {
                kind: RowKind::Hunk,
                text: line.to_string(),
                hunk,
                old_no: 0,
                new_no: 0,
            });
            hunk += 1;
        } else if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("---")
            || line.starts_with("+++")
            || line.starts_with("new file")
            || line.starts_with("Binary")
            || line.starts_with('\\')
        {
            rows.push(Row {
                kind: RowKind::Meta,
                text: line.to_string(),
                hunk: 0,
                old_no: 0,
                new_no: 0,
            });
        } else if let Some(body) = line.strip_prefix('+') {
            rows.push(Row {
                kind: RowKind::Add,
                text: body.to_string(),
                hunk: 0,
                old_no: 0,
                new_no,
            });
            new_no += 1;
        } else if let Some(body) = line.strip_prefix('-') {
            rows.push(Row {
                kind: RowKind::Del,
                text: body.to_string(),
                hunk: 0,
                old_no,
                new_no: 0,
            });
            old_no += 1;
        } else {
            let body = line.strip_prefix(' ').unwrap_or(line);
            rows.push(Row {
                kind: RowKind::Context,
                text: body.to_string(),
                hunk: 0,
                old_no,
                new_no,
            });
            old_no += 1;
            new_no += 1;
        }
    }
    rows
}

// --- engine access -----------------------------------------------------------

/// Build a cache key that uniquely identifies this diff request.
fn diff_key(
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    ignore_whitespace: bool,
    path: &Option<std::path::PathBuf>,
) -> String {
    format!("{root:?}|{left:?}|{right:?}|staged={staged}|ws={ignore_whitespace}|{path:?}")
}

/// Trigger an async diff load if the cache is missing/stale and not already
/// loading. Resets hunk navigation whenever a fresh load starts so the ‹n/N›
/// counter always describes the content being displayed.
fn ensure_diff(
    state: &mut AppState,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    ignore_whitespace: bool,
    path: &Option<std::path::PathBuf>,
) {
    let key = diff_key(root, left, right, staged, ignore_whitespace, path);
    let stale = state
        .ui
        .diff_cache
        .as_ref()
        .map(|(k, _)| k != &key)
        .unwrap_or(true);
    if stale && !state.ui.diff_loading {
        state.ui.diff_loading = true;
        state.ui.diff_error = None;
        state.ui.diff_current_hunk = 0;
        let executor: Arc<dyn crate::engine::GitExecutor> = state.executor.clone();
        let tx = state.tx.clone();
        let root = root.to_path_buf();
        let opts = DiffOpts {
            staged,
            ignore_whitespace,
            left: left.clone(),
            right: right.clone(),
            path: path.clone(),
            ..DiffOpts::default()
        };
        std::thread::spawn(move || {
            let res = executor.diff(&root, &opts);
            let _ = tx.send(AppEvent::DiffReady { key, result: res });
        });
    }
}

// --- entry point -------------------------------------------------------------

pub fn render_diff(
    ui: &mut Ui,
    state: &mut AppState,
    left: &Option<String>,
    right: &Option<String>,
    path: &Option<std::path::PathBuf>,
) {
    let root = match state.selected_path() {
        Some(r) => r,
        None => {
            ui.label("No repository selected.");
            return;
        }
    };

    // Working-tree comparisons expose the revision chips (spec §8.4);
    // explicit commit-to-commit targets keep their fixed revision pair.
    let working_tree = left.is_none() && right.is_none();
    let comparison = state.ui.diff_comparison;
    let ignore_ws = state.ui.diff_ignore_whitespace;
    let (eff_left, eff_right, staged): (Option<String>, Option<String>, bool) = if working_tree {
        match comparison {
            DiffComparison::Repo => (Some("HEAD".to_owned()), None, false),
            DiffComparison::Staged => (None, None, true),
            DiffComparison::Local => (None, None, false),
        }
    } else {
        (left.clone(), right.clone(), false)
    };

    ensure_diff(state, &root, &eff_left, &eff_right, staged, ignore_ws, path);
    let key = diff_key(&root, &eff_left, &eff_right, staged, ignore_ws, path);

    // Parsed rows from the cache (absent while loading / before first load).
    let cached = state.ui.diff_cache.clone().filter(|(k, _)| *k == key);
    let parsed = cached
        .as_ref()
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(_, t)| parse(t));
    let total_hunks = parsed.as_ref().map_or(0, |rs| {
        rs.iter().filter(|r| r.kind == RowKind::Hunk).count()
    });

    // Toolbar chrome (spec §8.4): mode · chips · hunk nav · whitespace.
    ui.horizontal(|ui| {
        let selected = if state.ui.diff_side_by_side { 0 } else { 1 };
        if let Some(idx) = segmented_control(ui, &["Side-by-Side", "Unified"], selected) {
            state.ui.diff_side_by_side = idx == 0;
        }
        if working_tree {
            ui.separator();
            comparison_chips(ui, state);
        }
        ui.separator();
        hunk_nav(ui, state, total_hunks);
        ui.separator();
        ui.checkbox(&mut state.ui.diff_ignore_whitespace, "Ignore whitespace");
    });

    if let Some(err) = &state.ui.diff_error {
        ui.colored_label(Palette::STATE_ERROR, err);
        return;
    }

    let rows = match (cached, parsed) {
        (_, Some(rows)) => rows,
        (None, None) => {
            if state.ui.diff_loading {
                ui.spinner();
                ui.label("Computing diff…");
            } else {
                ui.label("(no diff)");
            }
            return;
        }
        (Some(_), None) => {
            ui.label("(no differences)");
            return;
        }
    };

    ScrollArea::vertical().show(ui, |ui| {
        if state.ui.diff_side_by_side {
            render_side_by_side(ui, state, &rows, comparison);
        } else {
            render_unified(ui, state, &rows);
        }
    });
}

// --- toolbar widgets ---------------------------------------------------------

/// Compact segmented control (spec §8.4): SURFACE_2 track, the selected
/// segment sits on SURFACE_3 with INK ink. Returns the clicked option index.
fn segmented_control(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    const SEGMENT_H: f32 = 24.0;
    const PAD_X: f32 = 10.0;
    let font_id = FontId::new(12.0, FontFamily::Proportional);

    let widths: Vec<f32> = options
        .iter()
        .map(|o| {
            let g = ui
                .painter()
                .layout_no_wrap((*o).to_owned(), font_id.clone(), Color32::WHITE);
            g.size().x + PAD_X * 2.0
        })
        .collect();
    let track_w: f32 = widths.iter().sum();

    let (track, _) = ui.allocate_exact_size(Vec2::new(track_w, SEGMENT_H), Sense::hover());
    ui.painter()
        .rect_filled(track, CornerRadius::same(4), Palette::SURFACE_2);

    let mut clicked = None;
    let mut x = track.left();
    for (i, option) in options.iter().enumerate() {
        let seg = Rect::from_min_size(Pos2::new(x, track.top()), Vec2::new(widths[i], SEGMENT_H));
        let id = ui.id().with(("diff-segment", i));
        let resp = ui.interact(seg, id, Sense::click());
        let is_selected = i == selected;
        if is_selected {
            ui.painter()
                .rect_filled(seg, CornerRadius::same(3), Palette::SURFACE_3);
        }
        let ink = if is_selected || resp.hovered() {
            Palette::INK
        } else {
            Palette::INK_2
        };
        paint_centered(ui.painter(), seg, option, font_id.clone(), ink);
        resp.widget_info(move || {
            WidgetInfo::labeled(WidgetType::Button, true, (*option).to_owned())
        });
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}

/// Revision chips (spec §8.4): Repo/Staged/Local select the documented
/// working-tree comparison pair.
fn comparison_chips(ui: &mut Ui, state: &mut AppState) {
    for (cmp, label) in [
        (DiffComparison::Repo, "Repo"),
        (DiffComparison::Staged, "Staged"),
        (DiffComparison::Local, "Local"),
    ] {
        let selected = state.ui.diff_comparison == cmp;
        if chip_button(ui, label, selected).clicked() {
            state.ui.diff_comparison = cmp;
        }
    }
}

/// Pill-shaped selectable chip: selected = solid BRAND with brand ink,
/// unselected = SURFACE_3 with muted ink that brightens on hover.
fn chip_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
    const CHIP_H: f32 = 18.0;
    const PAD_X: f32 = 10.0;
    let font_id = FontId::new(11.0, FontFamily::Proportional);

    let idle_fg = if selected {
        Palette::BRAND_INK
    } else {
        Palette::INK_2
    };
    let measured = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id.clone(), idle_fg);
    let size = Vec2::new(measured.size().x + PAD_X * 2.0, CHIP_H);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let id = ui.id().with(("diff-chip", label));
    let response = ui.interact(rect, id, Sense::click());

    let bg = if selected {
        Palette::BRAND
    } else {
        Palette::SURFACE_3
    };
    let fg = if selected || response.hovered() {
        if selected {
            Palette::BRAND_INK
        } else {
            Palette::INK
        }
    } else {
        Palette::INK_2
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(CHIP_H as u8 / 2), bg);
    paint_centered(ui.painter(), rect, label, font_id, fg);
    response.widget_info(move || WidgetInfo::labeled(WidgetType::Button, true, label.to_owned()));
    response
}

/// Hunk navigation ‹ n/N › (spec §8.4). Stepping clamps to [0, total):
/// Previous never goes above the first hunk, Next never past the last.
fn hunk_nav(ui: &mut Ui, state: &mut AppState, total_hunks: usize) {
    let enabled = total_hunks > 0;
    let prev = nav_button(ui, Icon::CHEVRON_LEFT, "Previous hunk", enabled);
    if enabled {
        let current = state.ui.diff_current_hunk.min(total_hunks - 1);
        ui.label(format!("{}/{}", current + 1, total_hunks));
    }
    let next = nav_button(ui, Icon::CHEVRON_RIGHT, "Next hunk", enabled);

    if prev.clicked() {
        state.ui.diff_current_hunk = state.ui.diff_current_hunk.saturating_sub(1);
    }
    if next.clicked() && enabled {
        state.ui.diff_current_hunk = (state.ui.diff_current_hunk + 1).min(total_hunks - 1);
    }
}

/// Square ghost icon button with an explicit accessibility label.
fn nav_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    const SIZE: f32 = 24.0;
    const ICON_SIZE: f32 = 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::hover());
    let id = ui.id().with(("diff-nav", label));
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if response.is_pointer_button_down_on() {
        Palette::SURFACE_3
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    }
    let ink = if !enabled {
        Palette::INK_3
    } else if response.hovered() || response.is_pointer_button_down_on() {
        Palette::INK
    } else {
        Palette::INK_2
    };
    paint_icon_at(ui, icon, rect.center(), ICON_SIZE, ink);
    let closure_label = label.to_owned();
    response.widget_info(move || {
        WidgetInfo::labeled(WidgetType::Button, enabled, closure_label.clone())
    });
    response
}

/// Paint one icon primitive centered at `origin` without disturbing layout.
fn paint_icon_at(ui: &mut Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let origin = Pos2::new(center.x - size / 2.0, center.y - size / 2.0);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(origin, Vec2::splat(size)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    icons::icon(&mut child, icon, size, color);
}

/// Paint a string centered inside `rect`.
fn paint_centered(painter: &egui::Painter, rect: Rect, text: &str, font: FontId, color: Color32) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}

// --- rendering ---------------------------------------------------------------

/// Pane-header labels documenting the active pair (spec §8.4).
fn pane_labels(comparison: DiffComparison) -> (&'static str, &'static str) {
    match comparison {
        DiffComparison::Repo => ("HEAD", "Working tree"),
        DiffComparison::Staged => ("HEAD", "Index"),
        DiffComparison::Local => ("Index", "Working tree"),
    }
}

/// One vertically-centered text cell painted at absolute x inside `rect`.
fn paint_cell(
    painter: &egui::Painter,
    x: f32,
    rect: &Rect,
    text: &str,
    color: Color32,
    font: &FontId,
) {
    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// Unified-mode sign + line-number gutter cells (muted INK_3 numbers).
fn paint_gutter(
    painter: &egui::Painter,
    rect: &Rect,
    sign: &str,
    num: usize,
    sign_color: Color32,
    font: &FontId,
) {
    paint_cell(painter, rect.left() + 4.0, rect, sign, sign_color, font);
    let num_text = num.to_string();
    let galley = painter.layout_no_wrap(num_text, font.clone(), Palette::INK_3);
    let x = rect.left() + SIGN_W + NUM_W - galley.size().x - 6.0;
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        Palette::INK_3,
    );
}

fn render_unified(ui: &mut Ui, state: &mut AppState, rows: &[Row]) {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let font = mono_font();
    for row in rows {
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        match row.kind {
            RowKind::Meta => {
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &row.text,
                    Palette::INK_3,
                    &font,
                );
            }
            RowKind::Hunk => {
                painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &row.text,
                    Palette::INK_3,
                    &font,
                );
                if state.ui.diff_current_hunk > 0 && row.hunk == state.ui.diff_current_hunk {
                    resp.scroll_to_me(Some(Align::Center));
                }
            }
            RowKind::Context => {
                paint_gutter(&painter, &rect, " ", row.new_no, Palette::INK_3, &font);
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &row.text,
                    Palette::INK,
                    &font,
                );
            }
            RowKind::Del => {
                painter.rect_filled(rect, CornerRadius::ZERO, Palette::DIFF_DEL_BG);
                paint_gutter(
                    &painter,
                    &rect,
                    "-",
                    row.old_no,
                    Palette::DIFF_DEL_TEXT,
                    &font,
                );
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &row.text,
                    Palette::DIFF_DEL_TEXT,
                    &font,
                );
            }
            RowKind::Add => {
                painter.rect_filled(rect, CornerRadius::ZERO, Palette::DIFF_ADD_BG);
                paint_gutter(
                    &painter,
                    &rect,
                    "+",
                    row.new_no,
                    Palette::DIFF_ADD_TEXT,
                    &font,
                );
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &row.text,
                    Palette::DIFF_ADD_TEXT,
                    &font,
                );
            }
        }
    }
}

/// Side-by-side pane header band (SURFACE, spec §8.4).
fn header_band(ui: &mut Ui, width: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, PANE_HEADER_H), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(4), Palette::SURFACE);
    paint_centered(
        ui.painter(),
        rect,
        label,
        FontId::new(12.0, FontFamily::Proportional),
        Palette::INK,
    );
}

/// One side-by-side cell: optional token background band, muted gutter
/// number, sign marker, and code text.
#[allow(clippy::too_many_arguments)]
fn cell_band(
    ui: &mut Ui,
    width: f32,
    fill: Option<Color32>,
    num: usize,
    sign: char,
    text: &str,
    text_color: Color32,
    painter: &egui::Painter,
    font: &FontId,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
    if let Some(bg) = fill {
        painter.rect_filled(rect, CornerRadius::ZERO, bg);
    }
    if num > 0 {
        let galley = painter.layout_no_wrap(num.to_string(), font.clone(), Palette::INK_3);
        painter.galley(
            Pos2::new(
                rect.left() + NUM_W - galley.size().x - 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::INK_3,
        );
    }
    paint_cell(
        painter,
        rect.left() + NUM_W + 6.0,
        &rect,
        &sign.to_string(),
        text_color,
        font,
    );
    paint_cell(
        painter,
        rect.left() + NUM_W + 20.0,
        &rect,
        text,
        text_color,
        font,
    );
}

fn render_side_by_side(
    ui: &mut Ui,
    state: &mut AppState,
    rows: &[Row],
    comparison: DiffComparison,
) {
    let width = ui.available_width();
    let spacing = ui.style().spacing.item_spacing.x;
    let half = ((width - spacing * 2.0) / 2.0).max(120.0);
    let (left_label, right_label) = pane_labels(comparison);

    ui.horizontal(|ui| {
        header_band(ui, half, &format!("Before {left_label}"));
        header_band(ui, half, &format!("After {right_label}"));
    });

    let painter = ui.painter().clone();
    let font = mono_font();

    // Pair consecutive Del/Add lines; render context/hunk/meta as full-width.
    let mut i = 0;
    while i < rows.len() {
        match rows[i].kind {
            RowKind::Meta => {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &rows[i].text,
                    Palette::INK_3,
                    &font,
                );
                i += 1;
            }
            RowKind::Hunk => {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
                paint_cell(
                    &painter,
                    rect.left() + TEXT_X,
                    &rect,
                    &rows[i].text,
                    Palette::INK_3,
                    &font,
                );
                if state.ui.diff_current_hunk > 0 && rows[i].hunk == state.ui.diff_current_hunk {
                    resp.scroll_to_me(Some(Align::Center));
                }
                i += 1;
            }
            RowKind::Context => {
                let row = &rows[i];
                ui.horizontal(|ui| {
                    cell_band(
                        ui,
                        half,
                        None,
                        row.old_no,
                        ' ',
                        &row.text,
                        Palette::INK,
                        &painter,
                        &font,
                    );
                    cell_band(
                        ui,
                        half,
                        None,
                        row.new_no,
                        ' ',
                        &row.text,
                        Palette::INK,
                        &painter,
                        &font,
                    );
                });
                i += 1;
            }
            RowKind::Del | RowKind::Add => {
                // Gather a run of Del/Add lines to pair up.
                let mut dels: Vec<&Row> = Vec::new();
                let mut adds: Vec<&Row> = Vec::new();
                while i < rows.len() {
                    match rows[i].kind {
                        RowKind::Del => {
                            dels.push(&rows[i]);
                            i += 1;
                        }
                        RowKind::Add => {
                            adds.push(&rows[i]);
                            i += 1;
                        }
                        _ => break,
                    }
                }
                let pairs = dels.len().max(adds.len());
                for p in 0..pairs {
                    let d = dels.get(p);
                    let a = adds.get(p);
                    ui.horizontal(|ui| {
                        if let Some(d) = d {
                            cell_band(
                                ui,
                                half,
                                Some(Palette::DIFF_DEL_BG),
                                d.old_no,
                                '-',
                                &d.text,
                                Palette::DIFF_DEL_TEXT,
                                &painter,
                                &font,
                            );
                        } else {
                            cell_band(ui, half, None, 0, ' ', "", Palette::INK, &painter, &font);
                        }
                        if let Some(a) = a {
                            cell_band(
                                ui,
                                half,
                                Some(Palette::DIFF_ADD_BG),
                                a.new_no,
                                '+',
                                &a.text,
                                Palette::DIFF_ADD_TEXT,
                                &painter,
                                &font,
                            );
                        } else {
                            cell_band(ui, half, None, 0, ' ', "", Palette::INK, &painter, &font);
                        }
                    });
                }
            }
        }
    }
}
