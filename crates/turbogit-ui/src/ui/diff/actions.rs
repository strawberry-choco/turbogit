//! Diff actions and toolbar widgets: hunk/line staging dispatch,
//! the mode and hunk-nav toolbar, and the gutter stage buttons (spec R2).

use super::model::{DisplayRow, RowKind, line_counts, mono_font};
use crate::theme::Palette;
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Color32, CornerRadius, Pos2, Rect, Response, RichText, Sense, Ui, Vec2, WidgetInfo, WidgetType,
};
use turbogit_app::granular;
use turbogit_app::keyed_read::DiffTarget;
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, ChangeStatus};

// --- partial staging (spec R2) ----------------------------------------------

/// Hunk count of the diff the Commit window's preview would render right
/// now — 0 while nothing is selected, still loading, errored, or the text
/// parses to no hunks (binary). Peeks the diff read, which answers a value
/// that already carries its display model (ADR-0014, ADR-0021), so F7/Shift+F7
/// (spec R7) can consult it per keypress without rebuilding any row map — and
/// without starting a fetch the preview never asked for.
pub(crate) fn preview_hunk_count(state: &AppState) -> usize {
    let Some(change) = state.ui.preview_change.clone() else {
        return 0;
    };
    state
        .peek(preview_target(state, &change))
        .map_or(0, |diff| diff.model.hunk_count())
}

/// (added, removed) changed-line counts of the diff the Commit window's
/// preview would render right now — the header's `+N −M` change-size stats
/// (issue 06, design doc §5). `(0, 0)` while nothing is selected, still
/// loading, errored, or the text parses to no rows (binary). Reads the display
/// model the diff value carries, so the header never reparses the patch text
/// per frame.
pub(crate) fn preview_line_counts(state: &AppState, change: &ChangeRef) -> (usize, usize) {
    state
        .peek(preview_target(state, change))
        .map_or((0, 0), |diff| line_counts(&diff.model))
}

/// The commit window's preview target for `change`: the repository is the one the
/// change names, because the changes list spans every registered root and a
/// repo-relative path cannot say which one to read. The viewer's policy pair
/// decides its model, and a peek of it spends no git work (ADR-0021).
fn preview_target(state: &AppState, change: &ChangeRef) -> DiffTarget {
    DiffTarget::working_tree(
        change.root.as_path().to_path_buf(),
        state.ui.diff_policies,
        Some(change.path.clone()),
    )
}

/// The line the caret sits on, with the change that owns it — the target of `F4`
/// (Jump to Source), which has to land in the same place the reader was looking,
/// in the same repository.
///
/// The caret is **derived**, not stored: it is the first changed row of the
/// current hunk on the active side, which is the position the hunk navigator just
/// aimed at and the row the pane's own caret bar is painted over. A second cursor
/// of its own would be a second canonical selection, which CONTEXT.md "Current
/// hunk" exists to forbid. A side with no numbered row there — a pure insertion
/// read as the `Before` pane — has no line to jump to, and says so.
pub(crate) fn preview_caret_line(state: &AppState) -> Option<(ChangeRef, usize)> {
    let change = state.ui.preview_change.clone()?;
    let diff = state.peek(preview_target(state, &change))?;
    let want_add = state.ui.diff_pane == turbogit_app::state::DiffPane::New;
    diff.model
        .display
        .iter()
        .flat_map(|row| match row {
            DisplayRow::Full(r) => vec![r],
            DisplayRow::Pair(d, a) => d.iter().chain(a.iter()).collect(),
        })
        .filter(|r| r.hunk == state.ui.diff_current_hunk)
        .find(|r| r.kind == if want_add { RowKind::Add } else { RowKind::Del })
        .map(|r| r.new_no.max(r.old_no))
        .filter(|line| *line > 0)
        .map(|line| (change, line))
}

/// The diff viewer's settings menu (`⌃⇧D`): the two five-value policies, in the
/// reference's order — what counts as a difference first, then how finely a
/// difference is highlighted.
///
/// Both groups are in one menu rather than two controls because they are decided
/// together: a reviewer and the code they are reviewing agree on what changed only
/// once both questions have an answer.
///
/// Answers whether the frame changed anything, which is when the choice is
/// written down: a policy outlives the session it was picked in, because two
/// people reading one commit have to be reading it the same way tomorrow.
pub(super) fn diff_options_menu(ui: &mut Ui, state: &mut AppState) -> bool {
    use turbogit_domain::model::{DiffHighlighting, DiffWhitespace};
    let mut changed = false;
    let caption = crate::theme::chrome_font(crate::theme::TYPE_SECTION);
    ui.label(
        RichText::new(components::section_label("Ignore Differences"))
            .font(caption.clone())
            .color(Palette::INK_3),
    );
    for value in DiffWhitespace::ALL {
        if ui
            .radio_value(&mut state.ui.diff_policies.whitespace, value, value.label())
            .changed()
        {
            changed = true;
        }
    }
    ui.add_space(crate::theme::ITEM_SPACING.y);
    ui.label(
        RichText::new(components::section_label("Highlighting Differences"))
            .font(caption)
            .color(Palette::INK_3),
    );
    for value in DiffHighlighting::ALL {
        if ui
            .radio_value(
                &mut state.ui.diff_policies.highlighting,
                value,
                value.label(),
            )
            .changed()
        {
            changed = true;
        }
    }
    changed
}

// --- toolbar widgets ---------------------------------------------------------

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
    change: &Option<ChangeRef>,
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

    // The gutter pair means exactly one thing per button: this hunk, whole. In a
    // combined view a hunk can hold both staged and unstaged rows, so the button
    // names the direction and `Whole` resolves which appliable section's hunks
    // that direction reaches.
    if stage.clicked()
        && let Some(change) = change.clone()
    {
        granular::dispatch(
            state,
            change,
            granular::HunkTarget::Whole(hunk, granular::Direction::Stage),
        );
    }
    if unstage.clicked()
        && let Some(change) = change.clone()
    {
        granular::dispatch(
            state,
            change,
            granular::HunkTarget::Whole(hunk, granular::Direction::Unstage),
        );
    }
}

/// Hunk-header right side (issue 20, screen 06): the changed-line count, or
/// "N hidden" while the hunk is collapsed, plus the collapse toggle.
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
) {
    const BTN: f32 = 18.0;
    const PAD_X: f32 = 6.0;
    const GAP: f32 = 4.0;
    let n = hunk + 1;
    let count_text = if collapsed {
        format!("{hidden_rows} hidden")
    } else {
        components::counted(changed_lines, "line", "lines")
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
