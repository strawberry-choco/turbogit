//! Diff actions and toolbar widgets: hunk/line staging dispatch,
//! the mode/chips/nav toolbar, and the gutter stage buttons (spec R2).

use super::model::{line_counts, mono_font};
use crate::theme::Palette;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::collections::BTreeSet;
use turbogit_app::granular;
use turbogit_app::keyed_read::DiffTarget;
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
/// parses to no hunks (binary). Peeks the diff read, which answers a value
/// that already carries its display model (ADR-0014, ADR-0021), so F7/Shift+F7
/// (spec R7) can consult it per keypress without rebuilding any row map — and
/// without starting a fetch the preview never asked for.
pub(crate) fn preview_hunk_count(state: &AppState) -> usize {
    let Some(path) = state.ui.preview_change.clone() else {
        return 0;
    };
    state
        .peek(preview_target(state, path))
        .map_or(0, |diff| diff.model.hunk_count())
}

/// (added, removed) changed-line counts of the diff the Commit window's
/// preview would render right now — the header's `+N −M` change-size stats
/// (issue 06, design doc §5). `(0, 0)` while nothing is selected, still
/// loading, errored, or the text parses to no rows (binary). Reads the display
/// model the diff value carries, so the header never reparses the patch text
/// per frame.
pub(crate) fn preview_line_counts(state: &AppState, path: &std::path::Path) -> (usize, usize) {
    state
        .peek(preview_target(state, path.to_path_buf()))
        .map_or((0, 0), |diff| line_counts(&diff.model))
}

/// The commit window's preview target for `path`: the live chip and whitespace
/// state decides its sides, and a peek of it spends no git work (ADR-0021).
fn preview_target(state: &AppState, path: std::path::PathBuf) -> DiffTarget {
    DiffTarget::new(
        state.selected_path().unwrap_or_default(),
        None,
        None,
        state.ui.diff_comparison,
        state.ui.diff_ignore_whitespace,
        Some(path),
    )
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
///
/// Ticket 07: the bar is the one shared rail painter's, which absorbed this
/// hand-rolled filled rect. The geometry is unchanged (it already sat flush at
/// the leading edge); what it loses is the second definition of the width, which
/// now comes from `theme::RAIL_WIDTH`, and the guarantee that a row elsewhere
/// that grows a rail places it exactly here.
pub(super) fn paint_selection_bar(painter: &egui::Painter, rect: &Rect) {
    crate::ui::components::paint_rail(painter, *rect);
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
// --- the two axes of scope ----------------------------------------------------

/// One axis of scope, and the frame that makes its segments read as **one**
/// control rather than a run of loose buttons (spec story 22).
///
/// The two axes sit side by side and both answer "which?", so the frame — not
/// the labels — is what tells them apart. They take the two different kinds of
/// frame the palette already has, and a test can see the difference because one
/// boundary is laid down as a fill and the other is drawn as a stroke:
///
/// - [`Axis::Comparison`] — what is being compared (`Repo | Staged | Local`).
///   A **filled pad**: the three chips sit *inside* a raised band, so the group
///   is something the chips are contained by.
/// - [`Axis::Granularity`] — what one action addresses
///   (`File | Hunk | Line`). An **outlined track**: the boundary is drawn around
///   the segments rather than laid down under them, which is a visibly
///   different shape next to the comparison pad.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Axis {
    Comparison,
    Granularity,
}

/// Vertical breathing room between an axis's segments and its frame. Without it
/// the frame's rect is exactly its segments' rect, and a frame that is
/// indistinguishable from what it frames is not a frame — which is also what
/// would let the track's own fill pose as a second boundary.
///
/// Half the shared control padding — the space a control already keeps from its
/// own edge — rather than a new number.
const AXIS_PAD_Y: f32 = crate::theme::BUTTON_PADDING.y / 2.0;

/// Frame one axis of scope's segments as a single grouped control, and return
/// the group's rect.
///
/// Two things make it a group rather than a row of siblings, and both are
/// geometric, so a test can read them off painted output without knowing which
/// helper drew what:
///
/// 1. **Contiguity** — the segments are laid out edge to edge, with the axis's
///    own item spacing zeroed. A gap between two segments of one axis is
///    exactly the "loose buttons" reading this exists to remove. (The spacing is
///    set on the scope's child `Ui`, whose style is clone-on-write, so it cannot
///    leak to the toolbar around it.)
/// 2. **One outer boundary** — the axis paints exactly one frame rect, in the
///    shape [`Axis`] asks for, behind its segments and derived from the same
///    rect they were laid out in. The frame cannot drift away from the segments
///    it groups, because it *is* their rect.
pub(super) fn axis_group(ui: &mut Ui, axis: Axis, body: impl FnOnce(&mut Ui)) -> Rect {
    // Reserved before the body runs so the frame lands behind the segments
    // rather than over them.
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = ui.scope_builder(
        UiBuilder::new().layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            body(ui);
        },
    );
    let segments = inner.response.rect;
    // The frame is the same rect its segments were laid out in, grown by the
    // pad: it can never drift away from what it groups, and it is never
    // coincident with a segment's own fill (which is what would make "one outer
    // boundary" ambiguous to read back off the output).
    let frame = segments.expand2(Vec2::new(0.0, AXIS_PAD_Y));
    let radius = CornerRadius::same(crate::theme::CONTROL_RADIUS);
    let shape = match axis {
        Axis::Comparison => egui::Shape::rect_filled(frame, radius, Palette::SURFACE_2),
        Axis::Granularity => egui::Shape::rect_stroke(
            frame,
            radius,
            Stroke::new(1.0, Palette::LINE),
            StrokeKind::Inside,
        ),
    };
    ui.painter().set(slot, shape);
    segments
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
    // The shared chip height at button padding: this one is clickable, so it
    // takes the same horizontal padding as every other control and as
    // `widgets::segmented_control` (conformance issue 05).
    let pad_x = crate::theme::BUTTON_PADDING.x;
    let font_id = crate::theme::chrome_font(crate::theme::TYPE_CONTROL);

    let idle_fg = if selected {
        Palette::BRAND_INK
    } else {
        Palette::INK_2
    };
    let measured = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id.clone(), idle_fg);
    let geometry = widgets::ChipGeometry {
        height: widgets::CHIP_HEIGHT,
        pad_x,
        radius: widgets::CHIP_GEOMETRY.radius,
    };
    let size = geometry.size(&measured);
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
    geometry.paint(ui.painter(), rect, measured, bg, fg);
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
///
/// The nav scale: a standard 24 px square the host allocates itself, delegating
/// the ghost ladder to [`widgets::ghost_icon_button`].
fn nav_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    const SIZE: f32 = 24.0;
    const ICON_SIZE: f32 = 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::hover());
    let id = ui.id().with(("diff-nav", label));
    widgets::ghost_icon_button(ui, rect, id, label, enabled, |ui, rect, state| {
        let ink = widgets::ButtonVariant::Ghost.text(state);
        icons::centered_icon(ui, icon, rect.center(), ICON_SIZE, ink);
    })
}

/// Compact ghost action button painted inside an already-allocated row rect
/// (gutter scale, 18px): transparent at rest, SURFACE_2 hover fill with
/// INK_2→INK glyph ink, SURFACE_3 while pressed — the same ladder
/// [`nav_button`] uses, shrunk onto the hunk band. A real interactable widget
/// carrying labeled Button accessibility info, so kittest and screen readers
/// can find it.
fn gutter_button(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    glyph: &str,
    label: &str,
    tooltip: &str,
    enabled: bool,
) -> Response {
    let response = widgets::ghost_icon_button(ui, rect, id, label, enabled, |ui, rect, state| {
        let ink = widgets::ButtonVariant::Ghost.text(state);
        widgets::paint_centered_text(ui.painter(), rect, glyph, mono_font(), ink);
    });
    // The tooltip (and the reason a disabled control is inert) is the host's
    // wiring, not the ladder's: the vocabulary never decides wording.
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
    paint: egui::Id,
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

    let base_id = ui.id().with(("diff-gutter", paint));
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

/// Staged state of one viewer hunk (issue 20): the hunk's span, which the row
/// already carries, classified against the selected root's cached staged
/// (HEAD↔index) spans. Only the Repo comparison carries the information —
/// staged and unstaged hunks coexist only there; `None` elsewhere or without
/// stats. This is the paint path's only question about a hunk header, and it
/// asks it of the row rather than of git's `@@` text.
pub(super) fn viewer_hunk_staged_state(
    state: &AppState,
    path: &Option<std::path::PathBuf>,
    span: Option<turbogit_services::hunk_stats::HunkSpan>,
) -> Option<turbogit_services::hunk_stats::StagedState> {
    use turbogit_services::hunk_stats;
    let span = span?;
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
    paint: egui::Id,
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

    let base_id = ui.id().with(("diff-gutter", paint));
    // NOT a `widgets::ghost_icon_button` host, deliberately. This toggle has no
    // `enabled` axis (it is never inert) and its glyph is a constant INK_2 with
    // no hover-ink step, so folding it in would need either a pixel change or a
    // configuration flag on the ladder — the configurable-universal-widget shape
    // the role contract forbids. Revisit only if a shared "constant-ink toggle"
    // role earns its own vocabulary entry.
    let response = ui.interact(btn_rect, base_id.with(("collapse", hunk)), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        Palette::SURFACE_3
    } else if response.hovered() {
        Palette::SURFACE_2
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(
            btn_rect,
            CornerRadius::same(crate::theme::CONTROL_RADIUS),
            fill,
        );
    }
    icons::centered_icon(ui, icon, btn_rect.center(), 12.0, Palette::INK_2);
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
