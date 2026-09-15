//! Diff actions and toolbar widgets: hunk/line staging dispatch,
//! the mode/chips/nav toolbar, and the gutter stage buttons (spec R2).

use super::model::{diff_model, line_counts, mono_font};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, Response, Sense, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::collections::BTreeSet;
use turbogit_app::granular::{self, comparison_triple, diff_key};
use turbogit_app::root_caches::StatsView;
use turbogit_app::state::{AppState, DiffComparison, Granularity};
use turbogit_domain::model::ChangeStatus;

// --- partial staging (spec R2) ----------------------------------------------

/// Staging granularity toggle (issue 19, screen 06): File | Hunk | Line —
/// what one selection actuation on the diff surface addresses. Switching
/// clears accumulated selections ([`granular::set_granularity`] lifetime
/// rule), so a same-value re-click is filtered out. The push_id scope keeps
/// the segmented control's inner widget ids distinct from the mode toggle's
/// (same `("segment", i)` salts otherwise collide).
pub(super) fn granularity_toggle(ui: &mut Ui, state: &mut AppState) {
    const MODES: [Granularity; 3] = [Granularity::File, Granularity::Hunk, Granularity::Line];
    ui.push_id("granularity", |ui| {
        let selected = MODES
            .iter()
            .position(|m| *m == state.ui.diff_granularity)
            .unwrap_or(2);
        if let Some(idx) = widgets::segmented_control(ui, &["File", "Hunk", "Line"], selected)
            && MODES[idx] != state.ui.diff_granularity
        {
            granular::set_granularity(state, MODES[idx]);
        }
    });
}

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

/// (added, removed) changed-line counts of the diff the Commit window's
/// preview would render right now — the header's `+N −M` change-size stats
/// (issue 06, design doc §5). `(0, 0)` while nothing is selected, still
/// loading, errored, or the text parses to no rows (binary). Reads the
/// memoized display model beside the cache (ADR-0014), so the header never
/// reparses the patch text per frame.
pub(crate) fn preview_line_counts(state: &AppState, path: &std::path::Path) -> (usize, usize) {
    let Some(root) = state.selected_path() else {
        return (0, 0);
    };
    let (eff_left, eff_right, staged) = comparison_triple(&None, &None, state.ui.diff_comparison);
    let key = diff_key(
        &root,
        &eff_left,
        &eff_right,
        staged,
        state.ui.diff_ignore_whitespace,
        &Some(path.to_path_buf()),
    );
    state
        .ui
        .diff_cache
        .as_ref()
        .filter(|(k, _)| k == &key)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(_, t)| line_counts(&diff_model(t)))
        .unwrap_or((0, 0))
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
/// unselected = SURFACE_3 with muted ink that brightens on hover. Also
/// serves the staged-hunk chip rail (issue 20).
pub(crate) fn chip_button(ui: &mut Ui, label: &str, selected: bool) -> Response {
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

/// Staged state of one viewer hunk (issue 20): the hunk's own `@@` header
/// span classified against the selected root's cached staged (HEAD↔index)
/// spans. Only the Repo comparison carries the information — staged and
/// unstaged hunks coexist only there; `None` elsewhere or without stats.
pub(super) fn viewer_hunk_staged_state(
    state: &AppState,
    path: &Option<std::path::PathBuf>,
    hunk_header: &str,
) -> Option<turbogit_services::hunk_stats::StagedState> {
    use turbogit_services::hunk_stats::{self, HunkSpan};
    if state.ui.diff_comparison != DiffComparison::Repo {
        return None;
    }
    let path = path.as_ref()?;
    let root_id = state.selected_root.as_ref()?;
    // A tracked file with nothing staged is absent from the staged view —
    // exactly the "not staged" case, so absent entries classify against an
    // empty span list.
    let staged = state
        .caches
        .hunk_stats(root_id)?
        .file(StatsView::Staged, path)
        .map(|f| f.hunks.clone())
        .unwrap_or_default();
    let span: HunkSpan = hunk_stats::parse_hunk_header(hunk_header)?;
    Some(hunk_stats::hunk_staged_state(&span, &staged))
}

/// Hunk-header right side (issue 20, screen 06): the changed-line count with
/// its staged-state annotation — "N hidden" while collapsed — plus a chevron
/// toggle. Collapsing hides the hunk's body rows behind the count; expanding
#[allow(clippy::too_many_arguments)]
pub(super) fn hunk_header_extras(
    ui: &mut Ui,
    state: &mut AppState,
    band: Rect,
    diff_key: &str,
    hunk: usize,
    changed_lines: usize,
    collapsed: bool,
    hidden_rows: usize,
    staged_state: Option<turbogit_services::hunk_stats::StagedState>,
) {
    const BTN: f32 = 18.0;
    const PAD_X: f32 = 6.0;
    const GAP: f32 = 4.0;
    let n = hunk + 1;
    let count_text = if collapsed {
        format!("{hidden_rows} hidden")
    } else {
        let lines = if changed_lines == 1 {
            "1 line".to_owned()
        } else {
            format!("{changed_lines} lines")
        };
        match staged_state {
            Some(turbogit_services::hunk_stats::StagedState::Staged) => {
                format!("{lines} · staged")
            }
            Some(turbogit_services::hunk_stats::StagedState::Partial) => {
                format!("{lines} · part")
            }
            Some(turbogit_services::hunk_stats::StagedState::Unstaged) => {
                format!("{lines} · not staged")
            }
            None => lines,
        }
    };
    let (icon, label, tooltip) = if collapsed {
        (
            Icon::CHEVRON_DOWN,
            format!("Expand hunk {n}"),
            "Expand this hunk",
        )
    } else {
        (
            Icon::CHEVRON_RIGHT,
            format!("Collapse hunk {n}"),
            "Collapse this hunk",
        )
    };

    let count_galley = ui
        .painter()
        .layout_no_wrap(count_text, mono_font(), Palette::INK_3);
    let btn_rect = Rect::from_min_size(
        Pos2::new(band.right() - PAD_X - BTN, band.center().y - BTN / 2.0),
        Vec2::splat(BTN),
    );
    let count_rect = Rect::from_min_max(
        Pos2::new(btn_rect.left() - GAP - count_galley.size().x, band.top()),
        Pos2::new(btn_rect.left() - GAP, band.bottom()),
    );
    ui.painter().galley(
        Pos2::new(
            count_rect.left(),
            band.center().y - count_galley.size().y / 2.0,
        ),
        count_galley,
        Palette::INK_3,
    );

    let base_id = ui.id().with(("diff-gutter", diff_key));
    let response = ui.interact(btn_rect, base_id.with(("collapse", hunk)), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        Palette::SURFACE_3
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(btn_rect, CornerRadius::same(4), fill);
    }
    paint_icon_at(ui, icon, btn_rect.center(), 12.0, Palette::INK_2);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label.as_str()));
    widgets::focus_ring(ui, &response);
    if response.clicked() {
        if collapsed {
            state.ui.diff_collapsed.remove(&hunk);
        } else {
            state.ui.diff_collapsed.insert(hunk);
        }
    }
    response.on_hover_text(tooltip);
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
