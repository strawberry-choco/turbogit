use egui::{
    Align, Color32, CornerRadius, FontId, Layout, Pos2, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

use super::icons::{self, Icon};
use super::widgets::{
    CHIP_PAD_X, COMPACT_CHIP_GEOMETRY, COUNT_CHIP_COLORS, REF_CHIP_COLORS, WidgetState, count_chip,
    mix, tint_over_bg,
};
use crate::theme::{Palette, TYPE_BODY, TYPE_CHIP, TYPE_SECTION, chrome_font, data_font};

pub const TOOLBAR_H: f32 = 36.0;
pub const SECTION_H: f32 = 26.0;
pub const BRANCH_ROW_H: f32 = 30.0;
pub const SIDE_PANEL_W: f32 = 220.0;
pub const PAD_LIST: f32 = 16.0;
pub const PAD_STRIP: f32 = 12.0;

pub const KIT_ICON: f32 = 12.0;
pub const KIT_ICON_LARGE: f32 = 13.0;
pub const CLICK_TARGET_MIN: f32 = 24.0;
pub const KIT_BUTTON_H: f32 = 28.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Default,
    Hover,
    RowSelected,
    FocusSelected,
}

impl RowState {
    pub fn from_flags(selected: bool, hovered: bool) -> Self {
        match (selected, hovered) {
            (true, _) => Self::RowSelected,
            (false, true) => Self::Hover,
            (false, false) => Self::Default,
        }
    }

    pub fn is_selected(self) -> bool {
        matches!(self, Self::RowSelected | Self::FocusSelected)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowShell {
    Fill,
    Railed,
}

/// Paint one row's shell: the fill at the control radius, and, for
/// [`RowShell::Railed`] on a state [`RowState::is_selected`], the one accent
/// rail at the row's leading edge.
///
/// **The rect is the caller's, already allocated.** Nothing here measures,
/// allocates, or lays out: the body names no `allocate*`, no `available_*`, no
/// `spacing()`, and no font. A row that needs its own geometry lays it out first
/// (which is also the only order in which a row's own buttons keep their clicks)
/// and hands the rect in.
///
/// `tests/branch_component_kit.rs` asserts that from the source, because the
/// failure it prevents is not observable in a frame: a shell that re-measured
/// would still paint a correct-looking row, one pitch out of step with the
/// virtualised list that asked for it.
///
/// The `Fill` variant is the row fill's one construction site. It is the
/// former `fill`, with its decision, its control radius and its "no text
/// geometry" rule absorbed here; the two spellings were the same
/// `(ui, rect, state)` call, and a screen that reached the fill directly rather
/// than through the shell is the two-opinion case the row vocabulary exists to
/// prevent.
pub fn row_shell(ui: &Ui, rect: egui::Rect, state: RowState, variant: RowShell) {
    let fill = row_fill(state);
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(crate::theme::CONTROL_RADIUS), fill);
    }
    if variant == RowShell::Railed && state.is_selected() {
        paint_rail(ui.painter(), rect);
    }
}

pub fn row_fill(state: RowState) -> Color32 {
    match state {
        RowState::Default => Color32::TRANSPARENT,
        RowState::Hover => Palette::RAISED_ON_CARD,
        RowState::RowSelected => Palette::ROW_SELECTED,
        RowState::FocusSelected => Palette::selection_bg(),
    }
}

pub fn paint_rail(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::Pos2::new(rect.left(), rect.top()),
            egui::Pos2::new(rect.left() + crate::theme::RAIL_WIDTH, rect.bottom()),
        ),
        CornerRadius::ZERO,
        Palette::BRAND,
    );
}

pub fn current_row_fill(state: RowState) -> Color32 {
    match state {
        RowState::Default => tint_over_bg(Palette::BRAND, 0.16),
        RowState::Hover => tint_over_bg(Palette::BRAND, 0.24),
        RowState::RowSelected | RowState::FocusSelected => Palette::SELECTION,
    }
}

pub fn row_ink(stale: bool) -> Color32 {
    if stale {
        Palette::T_MUTED
    } else {
        Palette::T_PRIMARY
    }
}

pub const STATE_DOT_R: f32 = 3.0;

fn mark_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    if text.is_empty() {
        return STATE_DOT_R * 2.0;
    }
    STATE_DOT_R * 2.0
        + 6.0
        + ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
}

pub fn state_mark(ui: &mut Ui, text: &str, color: Color32) -> Response {
    mark(ui, text, color, &chrome_font(TYPE_CHIP))
}

fn words_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

fn words(ui: &mut Ui, text: &str, color: Color32, font: &FontId) -> Response {
    let w = words_width(ui, text, font);
    let h = super::widgets::CHIP_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color);
        ui.painter().galley(
            Pos2::new(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

pub fn state_words_width(ui: &Ui, text: &str) -> f32 {
    words_width(ui, text, &chrome_font(TYPE_CHIP))
}

pub fn state_words(ui: &mut Ui, text: &str, color: Color32) -> Response {
    words(ui, text, color, &chrome_font(TYPE_CHIP))
}

fn mark(ui: &mut Ui, text: &str, color: Color32, font: &FontId) -> Response {
    let w = mark_width(ui, text, font);
    let h = super::widgets::CHIP_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(
            Pos2::new(rect.left() + STATE_DOT_R, rect.center().y),
            STATE_DOT_R,
            color,
        );
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), color);
        ui.painter().galley(
            Pos2::new(
                rect.left() + STATE_DOT_R * 2.0 + 6.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

pub fn badge_words_width(ui: &Ui, kind: SyncKind, text: &str) -> f32 {
    words_width(ui, text, &badge_face(kind))
}

pub fn badge_words(ui: &mut Ui, kind: SyncKind, text: &str, color: Color32) -> Response {
    words(ui, text, color, &badge_face(kind))
}

/// The face one sync badge's label is painted in, and why it is not the chrome
/// face. Ahead and behind lead with a direction arrow, and the chrome chain owns
/// U+2191/U+2193 only through a fallback face, so a chrome `↑2` lays its arrow
/// out in one cut and its digit in another, which is two typefaces inside a
/// three-glyph mark. JetBrains Mono carries both arrow and digit, so the badge
/// reads as one thing, and a count with its direction glyph on it is data by §19
/// regardless. `gone` is still a word, so it stays in chrome like every other
/// state.
///
/// The coverage fact is contingent (an added fallback can supply the arrows) and
/// so is not the claim any test makes: what is pinned is that the data face owns
/// both glyphs, and that a row's badge paints in the monospace family.
fn badge_face(kind: SyncKind) -> FontId {
    match kind {
        SyncKind::Ahead | SyncKind::Behind => data_font(TYPE_CHIP),
        SyncKind::Diverged | SyncKind::InSync | SyncKind::Gone => chrome_font(TYPE_CHIP),
    }
}

pub fn ref_chip_with_ink(ui: &mut Ui, text: &str, ink: Color32) -> Response {
    let font = data_font(TYPE_CHIP);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let size = COMPACT_CHIP_GEOMETRY.size(&galley);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        COMPACT_CHIP_GEOMETRY.paint(ui.painter(), rect, galley, REF_CHIP_COLORS.bg, ink);
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

pub fn ref_chip_with_ink_fill() -> Color32 {
    REF_CHIP_COLORS.bg
}

pub fn ref_chip_with_ink_width(ui: &Ui, text: &str) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), data_font(TYPE_CHIP), Color32::WHITE);
    COMPACT_CHIP_GEOMETRY.size(&galley).x
}

pub fn scope_chip(ui: &mut Ui, label: &str) -> Response {
    let font = chrome_font(TYPE_CHIP);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, COUNT_CHIP_COLORS.fg);
    let chevron = super::widgets::CHIP_HEIGHT * 0.5;
    let w = galley.size().x + CHIP_PAD_X * 2.0 + chevron + 4.0;
    let h = super::widgets::CHIP_HEIGHT;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            super::widgets::compact_chip_radius(),
            COUNT_CHIP_COLORS.bg,
        );
        ui.painter().galley(
            Pos2::new(
                rect.left() + CHIP_PAD_X,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            COUNT_CHIP_COLORS.fg,
        );
        icons::paint_icon(
            ui.painter(),
            Pos2::new(
                rect.right() - CHIP_PAD_X - chevron,
                rect.center().y - chevron / 2.0,
            ),
            chevron,
            Icon::CHEVRON_DOWN,
            COUNT_CHIP_COLORS.fg,
        );
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response
}

pub fn mid_op_label(op: &str) -> String {
    format!("{op}…")
}

pub fn section_label(title: &str) -> String {
    title.to_uppercase()
}

/// One 26px section header on its own band: chevron, uppercase label, count
/// chip right-aligned, optional trailing action. Returns the strip's response,
/// clicking the strip toggles expanded/collapsed. The trailing action is
/// rendered *after* the strip's own interact target, so egui hit-testing gives it
/// precedence and a Fetch button never toggles collapse.
pub fn section_header<R>(
    ui: &mut Ui,
    title: &str,
    count: usize,
    expanded: bool,
    actions: impl FnOnce(&mut Ui) -> R,
) -> Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SECTION_H), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::SECTION_BG);

    let id = ui.auto_id_with(("section_header", title));
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, title));

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    child.add_space(PAD_LIST);
    let chevron = if expanded {
        Icon::CHEVRON_DOWN
    } else {
        Icon::CHEVRON_RIGHT
    };
    icons::icon(&mut child, chevron, KIT_ICON, Palette::T_MUTED);
    child.add_space(6.0);
    child.add(egui::Label::new(
        RichText::new(section_label(title))
            .font(chrome_font(TYPE_SECTION))
            .color(Palette::T_SECONDARY),
    ));

    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(PAD_LIST);
        ui.scope(actions);
        ui.add_space(4.0);
        count_chip(ui, &count.to_string());
    });

    response
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncKind {
    Ahead,
    Behind,
    Diverged,
    InSync,
    Gone,
}

pub fn sync_badge(ahead: usize, behind: usize, gone: bool) -> Vec<(SyncKind, String)> {
    if gone {
        return vec![(SyncKind::Gone, "gone".to_string())];
    }
    let mut badges = Vec::new();
    if ahead > 0 {
        badges.push((SyncKind::Ahead, format!("↑{ahead}")));
    }
    if behind > 0 {
        badges.push((SyncKind::Behind, format!("↓{behind}")));
    }
    badges
}

pub fn sync_ink(kind: SyncKind) -> Color32 {
    match kind {
        SyncKind::Ahead | SyncKind::InSync => crate::theme::RepoState::Unpushed.color(),
        SyncKind::Behind => crate::theme::RepoState::Unpulled.color(),
        SyncKind::Diverged => crate::theme::RepoState::Diverged.color(),
        SyncKind::Gone => Palette::DANGER,
    }
}

pub fn middle_truncate_to_width(ui: &Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width.max(0.0)
    };
    if fits(text) {
        return text.to_owned();
    }
    let mut budget = text.chars().count().saturating_sub(1);
    while budget >= 4 {
        let candidate = middle_truncate(text, budget);
        if fits(&candidate) {
            return candidate;
        }
        budget -= 1;
    }
    if fits("…") {
        "…".to_owned()
    } else {
        String::new()
    }
}

/// Truncate a branch name in the middle so the distinguishing prefix and
/// suffix both stay readable (`feature/multi…executor`), never a bare end
/// ellipsis. `max_chars` is the full budget including the ellipsis.
pub fn middle_truncate(name: &str, max_chars: usize) -> String {
    if max_chars < 4 {
        return name.chars().take(max_chars).collect();
    }
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    let keep = max_chars - 1;
    let head = ((keep as f32 * 0.62).round() as usize).clamp(1, keep - 1);
    let tail = keep - head;
    let head_s: String = name.chars().take(head).collect();
    let tail_s: String = name
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head_s}…{tail_s}")
}

pub fn counted(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("{n} {singular}")
    } else {
        format!("{n} {plural}")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KitButton {
    Primary,
    Secondary,
    Quiet,
    Danger,
}

impl KitButton {
    pub fn fill(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (Self::Primary, Idle | Disabled) => Palette::ACCENT,
            (Self::Primary, Hovered) => mix(Palette::ACCENT, Color32::WHITE, 0.10),
            (Self::Primary, Active) => mix(Palette::ACCENT, Color32::WHITE, 0.20),
            (Self::Secondary, Idle | Disabled) => Palette::RAISED,
            (Self::Secondary, Hovered) => mix(Palette::RAISED, Color32::WHITE, 0.10),
            (Self::Secondary, Active) => mix(Palette::RAISED, Color32::WHITE, 0.20),
            (Self::Quiet, Idle | Disabled) => Color32::TRANSPARENT,
            (Self::Quiet, Hovered) => Palette::SURFACE_2,
            (Self::Quiet, Active) => Palette::SURFACE_3,
            (Self::Danger, Idle) => tint_over_bg(Palette::DANGER, 0.12),
            (Self::Danger, Disabled) => Color32::TRANSPARENT,
            (Self::Danger, Hovered) => tint_over_bg(Palette::DANGER, 0.18),
            (Self::Danger, Active) => tint_over_bg(Palette::DANGER, 0.30),
        }
    }

    pub fn ink(self, state: WidgetState) -> Color32 {
        use WidgetState::{Active, Disabled, Hovered, Idle};
        match (self, state) {
            (_, Disabled) => Palette::T_MUTED,
            (Self::Primary, _) => Palette::BRAND_INK,
            (Self::Danger, _) => Palette::DANGER,
            (_, Idle) => Palette::T_SECONDARY,
            (_, Hovered | Active) => Palette::T_PRIMARY,
        }
    }
}

pub fn kit_button(ui: &mut Ui, kind: KitButton, label: &str) -> Response {
    kit_button_at(ui, kind, label, kit_button_width(ui, label))
}

pub fn kit_button_width(ui: &Ui, label: &str) -> f32 {
    12.0 * 2.0
        + ui.painter()
            .layout_no_wrap(label.to_owned(), chrome_font(TYPE_BODY), Color32::WHITE)
            .size()
            .x
}

pub fn kit_button_at(ui: &mut Ui, kind: KitButton, label: &str, width: f32) -> Response {
    let font_id = chrome_font(TYPE_BODY);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id, Color32::WHITE);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(width, KIT_BUTTON_H),
        if ui.is_enabled() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );

    let state = WidgetState::from_response(ui.is_enabled(), &response);

    let painter = ui.painter().clone();
    let radius = CornerRadius::same(crate::theme::CONTROL_RADIUS);
    let fill = kind.fill(state);
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, radius, fill);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(1.0),
            radius,
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
    let ink = kind.ink(state);
    // NOT `widgets::paint_centered_text`, deliberately, this is the same
    // two-axis arithmetic, but the galley above is laid out in `WHITE` (so the
    // width is measured once and reused for the natural-width measure) and the
    // ink is applied at paint time by *overriding* the text color. The shared
    // helper paints with `Painter::galley`, whose color argument is only a
    // fallback for `PLACEHOLDER` spans: a galley laid out in white keeps
    // painting white, so folding this call in would put every label on
    // `WHITE` instead of the state's ink. The two paths are not
    // interchangeable, so this site keeps the override call.
    painter.galley_with_override_text_color(
        egui::Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    response
}

pub fn overflow_button(ui: &mut Ui, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(CLICK_TARGET_MIN, CLICK_TARGET_MIN),
        Sense::click(),
    );
    let hovered = response.hovered() || response.is_pointer_button_down_on();
    let ink = if hovered {
        Palette::T_PRIMARY
    } else {
        Palette::T_MUTED
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    icons::icon(&mut child, Icon::MORE_HORIZONTAL, KIT_ICON, ink);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response
}
