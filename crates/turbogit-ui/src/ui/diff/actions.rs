use super::model::{BarKind, DisplayRow, Gap, Hidden, RowKind, TEXT_X, mono_font};
use crate::theme::Palette;
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use crate::ui::widgets;
use egui::{
    Color32, CornerRadius, FontId, Popup, Pos2, Rect, Response, RichText, Sense, Ui, Vec2,
    WidgetInfo, WidgetType,
};
use turbogit_app::granular;
use turbogit_app::keyed_read::{ContextTarget, DiffTarget};
use turbogit_app::reveals::Ask;
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, ChangeStatus};

const NAV_BUTTON_SIZE: f32 = 24.0;

pub(crate) fn preview_hunk_count(state: &AppState) -> usize {
    let Some(change) = state.ui.preview_change.clone() else {
        return 0;
    };
    state
        .peek(preview_target(state, &change))
        .map_or(0, |diff| diff.model.hunk_count())
}

fn preview_target(state: &AppState, change: &ChangeRef) -> DiffTarget {
    DiffTarget::working_tree(
        change.root.as_path().to_path_buf(),
        state.ui.diff_policies,
        Some(change.path.clone()),
    )
}

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

pub(super) fn diff_options_button(ui: &mut Ui, state: &mut AppState) {
    let trigger = ui.button(
        RichText::new("Diff options").font(crate::theme::chrome_font(crate::theme::TYPE_BODY)),
    );
    let mut popup = Popup::menu(&trigger);
    if state.ui.diff_options_open {
        popup = popup.open(true);
    }
    let mut policies_changed = false;
    match popup.show(|menu| diff_options_menu(menu, state)) {
        None => state.ui.diff_options_open = false,
        Some(shown) => policies_changed = shown.inner,
    }
    if policies_changed {
        state.save_durable_ui();
    }
}

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

pub(super) fn hunk_nav_width(ui: &Ui, current: usize, total_hunks: usize) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let label_w = if total_hunks > 0 {
        let galley = ui.painter().layout_no_wrap(
            format!("{}/{}", current.min(total_hunks - 1) + 1, total_hunks),
            crate::theme::chrome_font(crate::theme::TYPE_BODY),
            Color32::WHITE,
        );
        galley.size().x + gap
    } else {
        0.0
    };
    NAV_BUTTON_SIZE + gap + label_w + NAV_BUTTON_SIZE
}

fn nav_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    const ICON_SIZE: f32 = 14.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(NAV_BUTTON_SIZE), Sense::hover());
    let id = ui.id().with(("diff-nav", label));
    widgets::ghost_icon_button(ui, rect, id, label, enabled, |ui, rect, state| {
        let ink = widgets::ButtonVariant::Ghost.text(state);
        icons::centered_icon(ui, icon, rect.center(), ICON_SIZE, ink);
    })
}

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
    if enabled {
        response.on_hover_text(tooltip)
    } else {
        response.on_hover_text("Resolve the conflict first")
    }
}

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

/// What one bar paints into: the row it fills, the ids its button hangs from,
/// and the comparison its click asks a read of.
pub(super) struct Strip<'a> {
    pub(super) band: Rect,
    pub(super) paint: egui::Id,
    pub(super) key: &'a str,
    pub(super) comparison: &'a DiffTarget,
    pub(super) font: &'a FontId,
}

/// One bar over a context gap: what the gap still hides, and the one edge this
/// bar's one button opens.
pub(super) fn reveal_strip(
    ui: &mut Ui,
    state: &mut AppState,
    strip: Strip<'_>,
    gap: u32,
    info: &Gap,
    kind: BarKind,
    hidden_left: Option<u32>,
) {
    const BTN: f32 = 18.0;
    const PAD_X: f32 = 6.0;
    let Strip {
        band,
        paint,
        key,
        comparison,
        font,
    } = strip;

    ui.painter()
        .rect_filled(band, CornerRadius::ZERO, Palette::SURFACE);
    let caption = match (hidden_left, state.ui.reveals.side(key, info.file)) {
        (Some(n), _) => components::counted(
            n as usize,
            "unchanged line hidden",
            "unchanged lines hidden",
        ),
        // Only the file's own text can size a run that ends at the file's end. Nothing is being
        // read until a tap asks for it, so an untouched tail says what is unknown, not what is
        // in flight.
        (None, Ask::Waiting) => "Reading the file…".to_owned(),
        (None, _) => "Unchanged lines hidden".to_owned(),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(caption, font.clone(), Palette::INK_3);
    ui.painter().galley(
        Pos2::new(
            band.left() + TEXT_X,
            band.center().y - galley.size().y / 2.0,
        ),
        galley,
        Palette::INK_3,
    );

    let base_id = ui.id().with(("diff-gutter", paint));
    let response = strip_button(
        ui,
        Rect::from_min_size(
            Pos2::new(band.left() + PAD_X, band.center().y - BTN / 2.0),
            Vec2::splat(BTN),
        ),
        base_id.with((bar_verb(kind), gap)),
        kind,
        &bar_label(kind, info),
    );
    if response.clicked() {
        open_edge(state, key, comparison, info, gap, kind);
    }
}

/// The id verb this bar's button owns. The two bars of one gap must differ here,
/// or egui merges their pressed states and one lights up when the other is hit.
fn bar_verb(kind: BarKind) -> &'static str {
    match kind {
        BarKind::Next => "expand-next",
        BarKind::Previous => "expand-previous",
        BarKind::Collapsed => "expand-between",
    }
}

/// The glyph that names which edge this bar opens: down for the run that grows
/// away from the hunk above, up for the run that grows into the hunk below. A
/// collapsed bar is one `Edge::Top` tap, so it wears the down chevron too.
fn bar_glyph(kind: BarKind) -> Icon {
    match kind {
        BarKind::Next | BarKind::Collapsed => Icon::CHEVRON_DOWN,
        BarKind::Previous => Icon::CHEVRON_UP,
    }
}

fn strip_button(ui: &mut Ui, rect: Rect, id: egui::Id, kind: BarKind, label: &str) -> Response {
    widgets::ghost_icon_button(ui, rect, id, label, true, |ui, rect, state| {
        let ink = widgets::ButtonVariant::Ghost.text(state);
        icons::centered_icon(ui, bar_glyph(kind), rect.center(), 12.0, ink);
    })
}

/// The bar's own words: the hunk its block reads out of, or, where no hunk borders
/// that side of the gap, the edge of the file it reads out of.
fn bar_label(kind: BarKind, info: &Gap) -> String {
    // On a multi-file patch the ordinal printed here spans files, because
    // `Row::hunk` does, while `HunkTarget::Whole` indexes one file's own hunks.
    // Naming it is safe: staging controls are inert on such a pane, so nothing
    // reads the number this label shows.
    // `bars_for` puts a next bar only where a hunk borders the gap above and a
    // previous bar only where one borders it below, so the file-edge arms a
    // direction cannot reach are spelled anyway and this stays total.
    match (kind, info.above, info.below) {
        (BarKind::Next, Some(hunk), Some(_)) => {
            format!("Expand next lines below hunk {}", hunk + 1)
        }
        (BarKind::Next, _, None) => "Expand next lines at the end of the file".to_owned(),
        (BarKind::Next, None, Some(_)) => "Expand next lines at the top of the file".to_owned(),
        (BarKind::Previous, Some(_), Some(hunk)) => {
            format!("Expand previous lines above hunk {}", hunk + 1)
        }
        (BarKind::Previous, None, _) => "Expand previous lines at the top of the file".to_owned(),
        (BarKind::Previous, Some(_), None) => {
            "Expand previous lines at the end of the file".to_owned()
        }
        // Only an interior gap collapses, so one arm names both hunks; the others
        // keep the file-edge wording of the single bar a leading or trailing run
        // gets instead.
        (BarKind::Collapsed, Some(above), Some(below)) => {
            format!("Expand lines between hunks {} and {}", above + 1, below + 1)
        }
        (BarKind::Collapsed, _, None) => "Expand next lines at the end of the file".to_owned(),
        (BarKind::Collapsed, None, Some(_)) => {
            "Expand previous lines at the top of the file".to_owned()
        }
    }
}

/// One click on a bar: open its edge, owe its file's text, and ask for it.
///
/// No repaint is requested, because a click already buys its own follow-up frame,
/// and per-frame painting reads the counts and the text that landed rather than
/// reaching for a read, so no frame dispatches git work on its own.
fn open_edge(
    state: &mut AppState,
    key: &str,
    comparison: &DiffTarget,
    info: &Gap,
    gap: u32,
    kind: BarKind,
) {
    let hidden = match info.hidden {
        Hidden::Run(n) => Some(n),
        Hidden::ToEof => None,
    };
    if !state.ui.reveals.tap(key, gap as usize, kind.edge(), hidden) {
        return;
    }
    state.ui.reveals.admit(key, info.file);
    state.read(ContextTarget::new(comparison.clone(), info.file));
}

/// Aim the current hunk (CONTEXT.md "Current hunk") at the row under the
/// pointer, but only when the pointer genuinely rests on the rendered diff
/// rows AND moved this frame. A stationary pointer must not fight keyboard
/// or button navigation that just scrolled a different hunk underneath it
/// (spec R7: one canonical selection). Elsewhere, other panes, floating
/// popups, headless state injection, the previous value stays authoritative,
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
