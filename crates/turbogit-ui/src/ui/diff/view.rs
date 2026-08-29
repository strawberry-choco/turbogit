//! Diff rendering: the virtualized entry point plus the unified and
//! side-by-side row painters (spec §8.4, ADR-0014).

use super::actions::{
    commit_current_hunk, comparison_chips, hunk_gutter_actions, hunk_nav, line_selected,
    paint_centered, paint_selection_bar, preview_status, segmented_control,
};
use super::model::{
    DiffModel, DisplayRow, NUM_W, PANE_HEADER_H, PaneKind, ROW_H, Row, RowKind, SIGN_W, TEXT_X,
    diff_model, mono_font, pane_kind,
};
use super::panes::{
    binary_placeholder, ensure_diff, ensure_pane_bytes, pane_byte_lens, render_image_pane,
};
use crate::theme::Palette;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, ScrollArea, Sense, Ui,
    Vec2, WidgetInfo, WidgetType,
};
use std::ops::Range;
use turbogit_app::granular::{self, comparison_triple, diff_key};
use turbogit_app::state::{AppState, DiffComparison};
use turbogit_domain::model::ChangeStatus;

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
    let (eff_left, eff_right, staged) = comparison_triple(left, right, comparison);

    ensure_diff(state, &root, &eff_left, &eff_right, staged, ignore_ws, path);
    let key = diff_key(&root, &eff_left, &eff_right, staged, ignore_ws, path);

    // Cached display model (absent while loading / before first load).
    // Borrowed, not cloned (plan §1.1): both probes below end the immutable
    // borrow of `state.ui` immediately — before any `&mut state` use below —
    // and the model itself is memoized beside the raw cache (ADR-0014).
    let cached = state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .is_some();
    let model = state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(_, t)| diff_model(t));
    let total_hunks = model.as_ref().map_or(0, |m| m.hunk_count());

    // Toolbar chrome (spec §8.4): mode · chips · hunk nav · whitespace.
    // Wrapped so narrow panes (commit preview) push the toggle to a second
    // row instead of clipping it out of reach.
    ui.horizontal_wrapped(|ui| {
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

    let model = match model {
        Some(model) => model,
        None if cached => {
            ui.label("(no differences)");
            return;
        }
        None => {
            if state.ui.diff_loading {
                ui.spinner();
                ui.label("Computing diff…");
            } else {
                ui.label("(no diff)");
            }
            return;
        }
    };

    // Non-text diffs render outside the display-row model (spec R8,
    // ADR-0015): a lone binary change or an image pair replaces the rows
    // entirely — the mode toggle has no second layout to switch to, and
    // with no hunks there is nothing to navigate or stage. The pane key
    // inherits the diff cache key, so any reload of the patch text (ops
    // invalidate it) also refetches the pane bytes.
    match pane_kind(&model.files) {
        PaneKind::Text => {}
        PaneKind::Binary => {
            let meta = &model.files[0];
            let pane_key = format!("{key}#bin");
            ensure_pane_bytes(
                state,
                pane_key.clone(),
                &root,
                &eff_left,
                &eff_right,
                staged,
                meta,
                false,
            );
            // While loading (or when a side is unreadable) the sizes are
            // unresolved and the bare description shows — the graceful
            // fallback text.
            let sizes = pane_byte_lens(state, &pane_key);
            binary_placeholder(ui, sizes);
            return;
        }
        PaneKind::Image => {
            let meta = &model.files[0];
            let pane_key = format!("{key}#img");
            render_image_pane(
                ui, state, &pane_key, &root, &eff_left, &eff_right, staged, meta,
            );
            return;
        }
    }

    // Pure rename (100% similar, no content hunks): the header plus a note
    // instead of an empty scroller (spec R8).
    if model.pure_rename() {
        if let Some(text) = &model.rename_header {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
            paint_rename_header(ui.painter(), &rect, text, &mono_font());
        }
        ui.colored_label(Palette::INK_2, "No content changes.");
        return;
    }

    // Gutter staging controls need the previewed file's status (spec R2);
    // resolved once per frame from the cached changelists.
    let status = preview_status(state, path.as_deref());

    let side_by_side = state.ui.diff_side_by_side;
    // Paging total (ADR-0014): side-by-side walks the paired display rows,
    // unified ignores pairing and pages one slot per underlying row.
    let total_rows = if side_by_side {
        model.display.len()
    } else {
        model.raw_count
    };

    // Side-by-side pane header band (spec §8.4), pinned above the paged
    // rows: `show_rows` reserves exactly the uniform-height row window, so
    // the fixed-height band leaves the scrolled content. Floating scrollbars
    // reserve no width, so the panes below still measure the same `half`.
    if side_by_side {
        let width = ui.available_width();
        let gap = ui.style().spacing.item_spacing.x;
        let half = ((width - gap * 2.0) / 2.0).max(120.0);
        let (left_label, right_label) = pane_labels(comparison);
        ui.horizontal(|ui| {
            header_band(ui, half, &format!("Before {left_label}"));
            header_band(ui, half, &format!("After {right_label}"));
        });
    }

    // Named id salt: the commit window's `ui.columns` panes share one stable
    // child id, and egui's default ScrollArea salt is constant — an unnamed
    // area here would share persisted scrollbar state with the changelist
    // pane's area and flip-flop visibility (a zero-delay repaint loop).
    ScrollArea::vertical().id_salt("diff_viewer").show_rows(
        ui,
        ROW_H,
        total_rows,
        |ui, visible| {
            // Hunk navigation (ADR-0014): index-based — aim the aimed hunk's
            // first display row at the viewport center instead of relying on
            // a realized widget (`resp.scroll_to_me` cannot reach rows this
            // window didn't build). Issued inside the closure because egui
            // consumes scroll targets set by an area's content; ones set
            // before the area begins are stashed for outer areas. At most
            // once per (diff, hunk) — re-issuing every frame would keep the
            // ScrollArea repainting forever.
            if state.ui.diff_current_hunk > 0
                && let Some(row_idx) =
                    model.first_row_for_hunk(state.ui.diff_current_hunk, side_by_side)
                && hunk_needs_scroll(ui, &key, state.ui.diff_current_hunk)
            {
                let pitch = ROW_H + ui.spacing().item_spacing.y;
                let y = ui.max_rect().top() + (row_idx as f32 - visible.start as f32) * pitch;
                let rect = Rect::from_min_size(
                    Pos2::new(ui.max_rect().left(), y),
                    Vec2::new(ui.max_rect().width(), ROW_H),
                );
                ui.scroll_to_rect(rect, Some(Align::Center));
            }
            // The match above guarantees the rendered diff is `key`, so hunk
            // scroll-dedup state can be namespaced per diff with it (issue #11).
            if side_by_side {
                render_side_by_side(ui, state, &model, visible, &key, status, path);
            } else {
                render_unified(ui, state, &model, visible, &key, status, path);
            }
        },
    );
}

/// Issue the hunk scroll request at most once per (diff, hunk): re-issuing
/// the index-based scroll every frame would keep the ScrollArea repainting
/// forever.
fn hunk_needs_scroll(ui: &Ui, diff_key: &str, idx: usize) -> bool {
    let id = egui::Id::new(("diff_hunk_scrolled", diff_key, idx));
    let done = ui.ctx().memory(|m| m.data.get_temp::<bool>(id)) == Some(true);
    if !done {
        ui.ctx().memory_mut(|m| m.data.insert_temp(id, true));
    }
    !done
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
/// Unified mode (spec §8.4): one full-width band per underlying row, paging
/// only the visible window of the cached display model (ADR-0014). Pairs are
/// flattened back into their constituent rows, so output is pixel-identical
/// to the pre-virtualization loop.
fn render_unified(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    visible: Range<usize>,
    diff_key: &str,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    let width = ui.available_width();
    let painter = ui.painter().clone();
    let font = mono_font();

    let end = visible.end.min(model.raw_count);
    let start = visible.start.min(end);
    if start < end {
        // Hover tracking (spec R2): which hunk sits under the pointer this
        // frame — visible rows only, which is correct under virtualization.
        let mut frame_hover: Option<usize> = None;
        let mut rows_rect: Option<Rect> = None;
        // The underlying-row window may open or close mid-pair; visit every
        // display element touching it and keep only the rows inside it.
        let first = model.raw_to_display[start] as usize;
        let last = model.raw_to_display[end - 1] as usize;
        for disp in &model.display[first..=last] {
            match disp {
                DisplayRow::Full(row) => {
                    if visible.contains(&row.ord) {
                        unified_row(
                            ui,
                            state,
                            row,
                            width,
                            &painter,
                            &font,
                            diff_key,
                            status,
                            path,
                            &mut frame_hover,
                            &mut rows_rect,
                        );
                    }
                }
                DisplayRow::Pair(del, add) => {
                    for row in [del.as_ref(), add.as_ref()].into_iter().flatten() {
                        if visible.contains(&row.ord) {
                            unified_row(
                                ui,
                                state,
                                row,
                                width,
                                &painter,
                                &font,
                                diff_key,
                                status,
                                path,
                                &mut frame_hover,
                                &mut rows_rect,
                            );
                        }
                    }
                }
            }
        }
        commit_current_hunk(state, ui, rows_rect, frame_hover);
    }
}

/// One unified band: allocation, hover tracking, click toggle, and the
/// token-exact paint for a single underlying row (spec §8.4/R2). Hunk
/// scrolling is handled centrally by the [`ScrollArea::show_rows`] caller.
#[allow(clippy::too_many_arguments)]
fn unified_row(
    ui: &mut Ui,
    state: &mut AppState,
    row: &Row,
    width: f32,
    painter: &egui::Painter,
    font: &FontId,
    diff_key: &str,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
    frame_hover: &mut Option<usize>,
    rows_rect: &mut Option<Rect>,
) {
    // Changed lines are clickable toggles (spec R2 story 3); everything
    // else stays hover-only.
    let toggleable = matches!(row.kind, RowKind::Del | RowKind::Add);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, ROW_H),
        if toggleable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    *rows_rect = Some(match *rows_rect {
        Some(union) => union.union(rect),
        None => rect,
    });
    if !matches!(row.kind, RowKind::Meta | RowKind::RenameHeader) && resp.hovered() {
        *frame_hover = Some(row.hunk);
    }
    if toggleable {
        // Accessibility: the row is labeled by its content text so
        // tooling can target individual changed lines.
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, row.text.as_str()));
        if resp.clicked() {
            granular::toggle_line_selection(state, path, row.hunk, row.line_ord);
        }
    }
    match row.kind {
        RowKind::Meta => {
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::INK_3,
                font,
            );
        }
        RowKind::RenameHeader => paint_rename_header(painter, &rect, &row.text, font),
        RowKind::Hunk => {
            painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::INK_3,
                font,
            );
            hunk_gutter_actions(ui, state, rect, diff_key, row.hunk, status, path);
        }
        RowKind::Context => {
            paint_gutter(painter, &rect, " ", row.new_no, Palette::INK_3, font);
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::INK,
                font,
            );
        }
        RowKind::Del => {
            painter.rect_filled(rect, CornerRadius::ZERO, Palette::DIFF_DEL_BG);
            if line_selected(state, path, row.hunk, row.line_ord) {
                paint_selection_bar(painter, &rect);
            }
            paint_gutter(
                painter,
                &rect,
                "-",
                row.old_no,
                Palette::DIFF_DEL_TEXT,
                font,
            );
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::DIFF_DEL_TEXT,
                font,
            );
        }
        RowKind::Add => {
            painter.rect_filled(rect, CornerRadius::ZERO, Palette::DIFF_ADD_BG);
            if line_selected(state, path, row.hunk, row.line_ord) {
                paint_selection_bar(painter, &rect);
            }
            paint_gutter(
                painter,
                &rect,
                "+",
                row.new_no,
                Palette::DIFF_ADD_TEXT,
                font,
            );
            paint_cell(
                painter,
                rect.left() + TEXT_X,
                &rect,
                &row.text,
                Palette::DIFF_ADD_TEXT,
                font,
            );
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

/// Paint the rename-header text inside a row-shaped band — shared by the
/// in-scroller display row and the pure-rename static band (spec R8).
fn paint_rename_header(painter: &egui::Painter, rect: &Rect, text: &str, font: &FontId) {
    paint_cell(
        painter,
        rect.left() + TEXT_X,
        rect,
        text,
        Palette::INK_2,
        font,
    );
}
/// One side-by-side cell: optional token background band, muted gutter
/// number, sign marker, and code text. Changed cells are clickable line
/// toggles labeled by their content (spec R2 story 3); `selected` paints
/// the BRAND edge bar. Returns the cell's response so the row loop can
/// track hover and toggle clicks.
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
    toggleable: bool,
    selected: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(width, ROW_H),
        if toggleable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if let Some(bg) = fill {
        painter.rect_filled(rect, CornerRadius::ZERO, bg);
    }
    if selected {
        paint_selection_bar(painter, &rect);
    }
    if toggleable {
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
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
    resp
}

/// Side-by-side mode (spec §8.4): paired Del/Add bands plus full-width
/// context/hunk/meta rows, paging only the visible window of the cached
/// display model (ADR-0014). The pane header band is pinned by the caller.
/// Hover tracking and toggles apply to visible rows only — correct under
/// virtualization.
fn render_side_by_side(
    ui: &mut Ui,
    state: &mut AppState,
    model: &DiffModel,
    visible: Range<usize>,
    diff_key: &str,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    let width = ui.available_width();
    let spacing = ui.style().spacing.item_spacing.x;
    let half = ((width - spacing * 2.0) / 2.0).max(120.0);

    let painter = ui.painter().clone();
    let font = mono_font();
    // Hover tracking (spec R2): which hunk sits under the pointer this frame.
    let mut frame_hover: Option<usize> = None;
    let mut rows_rect: Option<Rect> = None;

    let end = visible.end.min(model.display.len());
    let start = visible.start.min(end);
    for disp in &model.display[start..end] {
        match disp {
            DisplayRow::Full(row) => match row.kind {
                RowKind::Meta => {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    paint_cell(
                        &painter,
                        rect.left() + TEXT_X,
                        &rect,
                        &row.text,
                        Palette::INK_3,
                        &font,
                    );
                }
                RowKind::RenameHeader => {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    paint_rename_header(&painter, &rect, &row.text, &font);
                }
                RowKind::Hunk => {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
                    rows_rect = Some(match rows_rect {
                        Some(union) => union.union(rect),
                        None => rect,
                    });
                    if resp.hovered() {
                        frame_hover = Some(row.hunk);
                    }
                    painter.rect_filled(rect, CornerRadius::ZERO, Palette::SURFACE);
                    paint_cell(
                        &painter,
                        rect.left() + TEXT_X,
                        &rect,
                        &row.text,
                        Palette::INK_3,
                        &font,
                    );
                    hunk_gutter_actions(ui, state, rect, diff_key, row.hunk, status, path);
                }
                RowKind::Context => {
                    let hunk = row.hunk;
                    ui.horizontal(|ui| {
                        let old_cell = cell_band(
                            ui,
                            half,
                            None,
                            row.old_no,
                            ' ',
                            &row.text,
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        );
                        let new_cell = cell_band(
                            ui,
                            half,
                            None,
                            row.new_no,
                            ' ',
                            &row.text,
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        );
                        if old_cell.hovered() || new_cell.hovered() {
                            frame_hover = Some(hunk);
                        }
                    });
                }
                // Changed rows always live in pairs (build_model invariant).
                RowKind::Del | RowKind::Add => unreachable!("changed rows are always paired"),
            },
            DisplayRow::Pair(d, a) => {
                let hunk = d.as_ref().map(|r| r.hunk).or(a.as_ref().map(|r| r.hunk));
                ui.horizontal(|ui| {
                    let del_cell = if let Some(d) = d {
                        let selected = line_selected(state, path, d.hunk, d.line_ord);
                        let cell = cell_band(
                            ui,
                            half,
                            Some(Palette::DIFF_DEL_BG),
                            d.old_no,
                            '-',
                            &d.text,
                            Palette::DIFF_DEL_TEXT,
                            &painter,
                            &font,
                            true,
                            selected,
                        );
                        if cell.clicked() {
                            granular::toggle_line_selection(state, path, d.hunk, d.line_ord);
                        }
                        cell
                    } else {
                        cell_band(
                            ui,
                            half,
                            None,
                            0,
                            ' ',
                            "",
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        )
                    };
                    let add_cell = if let Some(a) = a {
                        let selected = line_selected(state, path, a.hunk, a.line_ord);
                        let cell = cell_band(
                            ui,
                            half,
                            Some(Palette::DIFF_ADD_BG),
                            a.new_no,
                            '+',
                            &a.text,
                            Palette::DIFF_ADD_TEXT,
                            &painter,
                            &font,
                            true,
                            selected,
                        );
                        if cell.clicked() {
                            granular::toggle_line_selection(state, path, a.hunk, a.line_ord);
                        }
                        cell
                    } else {
                        cell_band(
                            ui,
                            half,
                            None,
                            0,
                            ' ',
                            "",
                            Palette::INK,
                            &painter,
                            &font,
                            false,
                            false,
                        )
                    };
                    if (del_cell.hovered() || add_cell.hovered())
                        && let Some(h) = hunk
                    {
                        frame_hover = Some(h);
                    }
                });
            }
        }
    }
    commit_current_hunk(state, ui, rows_rect, frame_hover);
}
