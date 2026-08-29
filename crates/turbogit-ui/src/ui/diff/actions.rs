//! Diff actions and toolbar widgets: hunk/line staging dispatch,
//! the mode/chips/nav toolbar, and the gutter stage buttons (spec R2).

use super::model::{diff_model, mono_font};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Response, Sense, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::collections::BTreeSet;
use turbogit_app::granular::{self, comparison_triple, diff_key};
use turbogit_app::state::{AppState, DiffComparison};
use turbogit_domain::model::ChangeStatus;

// --- partial staging (spec R2) ----------------------------------------------

/// Resolve the previewed file's [`ChangeStatus`] from the selected root's
/// cached changelists (read-only per frame). The commit window previews
/// root-relative paths; absolute paths match too so other callers stay safe.
/// Unlisted paths fall back to [`ChangeStatus::Modified`] — controls stay
/// enabled and the engine seam remains the final authority. Also serves the
/// palette's Stage/Unstage Hunk verbs.
pub(crate) fn preview_status(state: &AppState, path: Option<&std::path::Path>) -> ChangeStatus {
    let Some(path) = path else {
        return ChangeStatus::Modified;
    };
    if let Some(id) = &state.selected_root
        && let Some(root) = state.multi.by_id(id)
        && let Some(c) = root.resolve_change(path)
    {
        return c.status;
    }
    ChangeStatus::Modified
}

/// Hunk count of the diff the Commit window's preview would render right
/// now — 0 while nothing is selected, still loading, errored, or the text
/// parses to no hunks (binary). Reads the memoized display model beside the
/// cache (ADR-0014), so F7/Shift+F7 (spec R7) can consult it per keypress
/// without rebuilding any row map.
pub(crate) fn preview_hunk_count(state: &AppState) -> usize {
    let Some(root) = state.selected_path() else {
        return 0;
    };
    let Some(path) = state.ui.preview_change.clone() else {
        return 0;
    };
    let (eff_left, eff_right, staged) = comparison_triple(&None, &None, state.ui.diff_comparison);
    let key = diff_key(
        &root,
        &eff_left,
        &eff_right,
        staged,
        state.ui.diff_ignore_whitespace,
        &Some(path),
    );
    state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(_, t)| diff_model(t).hunk_count())
        .unwrap_or(0)
}

/// Whether one changed line currently sits in the accumulated sub-hunk
/// selection (spec R2 story 3).
pub(super) fn line_selected(
    state: &AppState,
    path: &Option<std::path::PathBuf>,
    hunk: usize,
    ord: usize,
) -> bool {
    path.as_ref()
        .and_then(|p| state.ui.line_selections.get(p))
        .and_then(|m| m.get(&hunk))
        .is_some_and(|s| s.contains(&ord))
}

/// Selected-line marker (spec R2 story 3): a BRAND edge bar on the row's
/// left — the IDE-gutter convention, readable over both diff band tints.
pub(super) fn paint_selection_bar(painter: &egui::Painter, rect: &Rect) {
    painter.rect_filled(
        Rect::from_min_size(rect.left_top(), Vec2::new(2.0, rect.height())),
        CornerRadius::ZERO,
        Palette::BRAND,
    );
}

/// The accumulated line selection for one hunk, when any.
fn line_selection_for(
    state: &AppState,
    path: &Option<std::path::PathBuf>,
    hunk: usize,
) -> Option<BTreeSet<usize>> {
    let lines = path
        .as_ref()
        .and_then(|p| state.ui.line_selections.get(p))
        .and_then(|m| m.get(&hunk))?;
    (!lines.is_empty()).then(|| lines.clone())
}

/// Dispatch granular stage/unstage of one whole hunk or the accumulated
/// sub-hunk line selection (spec R2): pure intent — the core granular module
/// resolves the cached patch text (ADR-0013), status, routing, label, and
/// scope.
fn dispatch_hunk_action(
    state: &mut AppState,
    hunk: usize,
    stage: bool,
    path: &Option<std::path::PathBuf>,
) {
    let target = match line_selection_for(state, path, hunk) {
        // Story 3: an accumulated sub-hunk selection narrows the patch to
        // exactly the toggled lines; otherwise the whole hunk applies.
        Some(lines) => granular::HunkTarget::Lines(hunk, lines),
        None => granular::HunkTarget::Whole(hunk),
    };
    let Some(path) = path.clone() else {
        return;
    };
    granular::dispatch(state, path, target, stage);
}
// --- toolbar widgets ---------------------------------------------------------

/// Compact segmented control (spec §8.4): SURFACE_2 track, the selected
/// segment sits on SURFACE_3 with INK ink. Returns the clicked option index.
pub(super) fn segmented_control(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
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
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, *option));
        widgets::focus_ring(ui, &resp);
        if resp.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}

/// Revision chips (spec §8.4): Repo/Staged/Local select the documented
/// working-tree comparison pair.
pub(super) fn comparison_chips(ui: &mut Ui, state: &mut AppState) {
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
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    widgets::focus_ring(ui, &response);
    response
}

/// Hunk navigation ‹ n/N › (spec §8.4). Stepping clamps to [0, total):
/// Previous never goes above the first hunk, Next never past the last.
pub(super) fn hunk_nav(ui: &mut Ui, state: &mut AppState, total_hunks: usize) {
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
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::focus_ring(ui, &response);
    response
}

/// Compact ghost action button painted inside an already-allocated row rect
/// (gutter scale, 18px): transparent at rest, SURFACE_2 hover fill with
/// INK_2→INK glyph ink, SURFACE_3 while pressed — the [`nav_button`] ladder
/// shrunk onto the hunk band. A real interactable widget carrying labeled
/// Button accessibility info, so kittest and screen readers can find it.
fn gutter_button(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    glyph: &str,
    label: &str,
    tooltip: &str,
    enabled: bool,
) -> Response {
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
    paint_centered(ui.painter(), rect, glyph, mono_font(), ink);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    widgets::focus_ring(ui, &response);
    if enabled {
        response.on_hover_text(tooltip)
    } else {
        response.on_hover_text("Resolve the conflict first")
    }
}

/// Stage/unstage gutter pair on a hunk-header band (spec R2): two compact
/// buttons at the band's left edge — "+" stages that whole hunk forward,
/// "−" reverse-applies it out of the index (1-based numbering in labels).
/// Conflicted files keep the pair visible but inert; conflicts resolve
/// through the conflict modal.
pub(super) fn hunk_gutter_actions(
    ui: &mut Ui,
    state: &mut AppState,
    band: Rect,
    diff_key: &str,
    hunk: usize,
    status: ChangeStatus,
    path: &Option<std::path::PathBuf>,
) {
    const BTN: f32 = 18.0;
    const PAD_X: f32 = 6.0;
    const GAP: f32 = 4.0;
    let enabled = status != ChangeStatus::Conflicted;

    let y = band.center().y - BTN / 2.0;
    let stage_rect = Rect::from_min_size(Pos2::new(band.left() + PAD_X, y), Vec2::splat(BTN));
    let unstage_rect = Rect::from_min_size(
        Pos2::new(band.left() + PAD_X + BTN + GAP, y),
        Vec2::splat(BTN),
    );

    let base_id = ui.id().with(("diff-gutter", diff_key));
    let n = hunk + 1;
    let stage_label = format!("Stage hunk {n}");
    let stage = gutter_button(
        ui,
        stage_rect,
        base_id.with(("stage", hunk)),
        "+",
        &stage_label,
        "Stage this hunk",
        enabled,
    );
    let unstage_label = format!("Unstage hunk {n}");
    let unstage = gutter_button(
        ui,
        unstage_rect,
        base_id.with(("unstage", hunk)),
        "-",
        &unstage_label,
        "Unstage this hunk",
        enabled,
    );

    if stage.clicked() {
        dispatch_hunk_action(state, hunk, true, path);
    }
    if unstage.clicked() {
        dispatch_hunk_action(state, hunk, false, path);
    }
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
pub(super) fn paint_centered(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
) {
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
/// Aim the current hunk (CONTEXT.md "Current hunk") at the row under the
/// pointer — but only when the pointer genuinely rests on the rendered diff
/// rows AND moved this frame. A stationary pointer must not fight keyboard
/// or button navigation that just scrolled a different hunk underneath it
/// (spec R7: one canonical selection). Elsewhere — other panes, floating
/// popups, headless state injection — the previous value stays authoritative,
/// so navigation and the palette verbs operate on the hunk last aimed at.
pub(super) fn commit_current_hunk(
    state: &mut AppState,
    ui: &Ui,
    rows_rect: Option<Rect>,
    frame_hover: Option<usize>,
) {
    let moved = ui.input(|i| i.pointer.motion().is_some_and(|d| d != Vec2::ZERO));
    let inside = ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| rows_rect.is_some_and(|r| r.contains(p)));
    if moved
        && inside
        && let Some(hunk) = frame_hover
    {
        state.ui.diff_current_hunk = hunk;
    }
}
