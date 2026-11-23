use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Frame, InnerResponse, Layout, Margin,
    Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};

use super::chips::BADGE_TINT;
use super::chips::count_chip;
use super::controls::tint_over_bg;
use super::inputs::INPUT_ICON_SIZE;
use crate::theme::{CONTROL_RADIUS, Palette, TYPE_CONTROL, TYPE_SECTION};
use crate::ui::components;
use crate::ui::icons::{self, Icon};
use turbogit_app::history_editor::RebaseCaution;

const CHURN_BAR_HEIGHT: f32 = 4.0;
const ALERT_ICON_SIZE: f32 = INPUT_ICON_SIZE;
const MICRO_TEXT: f32 = TYPE_CONTROL;
const BOLD_FAMILY: &str = "jetbrains-mono-bold";

/// Bold body font via the named family registered by `install_fonts`,
/// falling back to the regular proportional face when fonts are not
/// installed yet (epaint panics on unbound families).
pub(crate) fn bold_font_if_available(ui: &Ui) -> FontId {
    let has_bold = ui.ctx().fonts(|f| {
        f.definitions()
            .families
            .contains_key(&FontFamily::Name(BOLD_FAMILY.into()))
    });
    if has_bold {
        FontId::new(
            crate::theme::TYPE_DETAIL_TITLE,
            FontFamily::Name(BOLD_FAMILY.into()),
        )
    } else {
        crate::theme::chrome_font(crate::theme::TYPE_DETAIL_TITLE)
    }
}

pub(crate) fn footer_rule(ui: &mut Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::RULE_FOOTER);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Bottom,
    Top,
    Right,
}

pub fn edge_rule(painter: &Painter, rect: Rect, edge: Edge, ink: Color32) {
    let (a, b) = match edge {
        Edge::Bottom => (
            Pos2::new(rect.left(), rect.bottom() - 0.5),
            Pos2::new(rect.right(), rect.bottom() - 0.5),
        ),
        Edge::Top => (
            Pos2::new(rect.left(), rect.top() + 0.5),
            Pos2::new(rect.right(), rect.top() + 0.5),
        ),
        Edge::Right => (
            Pos2::new(rect.right() - 0.5, rect.top()),
            Pos2::new(rect.right() - 0.5, rect.bottom()),
        ),
    };
    painter.line_segment([a, b], Stroke::new(1.0, ink));
}

pub fn dialog_footer<R>(ui: &mut Ui, buttons: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    footer_rule(ui);
    ui.add_space(6.0);
    ui.with_layout(Layout::right_to_left(Align::Center), buttons)
}

const BORDER_HAIRLINE_WIDTH: f32 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardSurface {
    Content,
    Raised,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CardSizing {
    Stretch,
    StretchHeight,
    MinWidth(f32),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CardFrame {
    pub surface: CardSurface,
    pub pad: i8,
    pub sizing: CardSizing,
    pub bordered: bool,
}

impl Default for CardFrame {
    fn default() -> Self {
        Self {
            surface: CardSurface::Content,
            pad: crate::theme::PANEL_PADDING as i8,
            sizing: CardSizing::Stretch,
            bordered: false,
        }
    }
}

impl CardFrame {
    pub fn bordered(self) -> Self {
        Self {
            bordered: true,
            ..self
        }
    }

    pub fn raised(self) -> Self {
        Self {
            surface: CardSurface::Raised,
            ..self
        }
    }

    pub fn padded(self, pad: i8) -> Self {
        Self { pad, ..self }
    }

    pub fn min_width(self, width: f32) -> Self {
        Self {
            sizing: CardSizing::MinWidth(width),
            ..self
        }
    }

    pub fn stretch_height(self) -> Self {
        Self {
            sizing: CardSizing::StretchHeight,
            ..self
        }
    }
}

pub fn card<R>(
    ui: &mut Ui,
    frame: CardFrame,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let fill = match frame.surface {
        CardSurface::Content => Palette::CONTENT_BG,
        CardSurface::Raised => Palette::SURFACE,
    };
    let mut card = Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(crate::theme::CARD_RADIUS))
        .inner_margin(Margin::same(frame.pad));
    if frame.bordered {
        card = card.stroke(Stroke::new(BORDER_HAIRLINE_WIDTH, Palette::LINE));
    }
    card.show(ui, |ui| match frame.sizing {
        CardSizing::Stretch => {
            ui.set_width(ui.available_width());
            add_contents(ui)
        }
        CardSizing::StretchHeight => {
            let height = ui.available_height();
            ui.set_width(ui.available_width());
            let out = add_contents(ui);
            ui.add_space((height - ui.min_rect().height()).max(0.0));
            out
        }
        CardSizing::MinWidth(w) => {
            ui.set_min_width(w);
            add_contents(ui)
        }
    })
}

pub fn card_header(ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
    contents(ui);
    ui.separator();
}

pub fn note<R>(
    ui: &mut Ui,
    severity: Option<Color32>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let mut frame = Frame::new()
        .fill(Palette::SURFACE_2)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::same(8));
    if let Some(ink) = severity {
        frame = frame.stroke(Stroke::new(1.0, ink));
    }
    frame.show(ui, add_contents)
}

pub fn cautions_rail(ui: &mut Ui, cautions: &[RebaseCaution]) {
    if cautions.is_empty() {
        return;
    }
    group_title(ui, &format!("CAUTIONS · {}", cautions.len()));
    for caution in cautions {
        let text = match caution {
            RebaseCaution::ConflictRisk { files } => {
                format!(
                    "Conflicts likely on {}",
                    components::counted(*files, "file", "files")
                )
            }
            RebaseCaution::MixedIdentities { authors } => format!(
                "Mixed committer identities ({}) — verify signatures",
                components::counted(*authors, "author", "authors")
            ),
        };
        ui.colored_label(Palette::STATE_WARNING, format!("⚠ {text}"));
    }
    ui.add_space(6.0);
}

pub fn recovery_note(ui: &mut Ui, backup_ref: &str) {
    group_title(ui, "RECOVERY");
    ui.label("A backup ref is written before the first commit is replayed, and the recovery path restores the pre-rewrite state from it.");
    ui.monospace(backup_ref.to_owned());
}

pub const PANE_HEADER_HEIGHT: f32 = 28.0;

pub const COLUMN_HEADER_HEIGHT: f32 = 16.0;

const HEADER_RULE_HEIGHT: f32 = 1.0;

const PANE_HEADER_CHIP_GAP: f32 = 6.0;

/// Allocate a header band of exactly `height` with **no** inter-item gap above
/// or below it, and run `add` in it.
///
/// egui's own allocation advances the caller's cursor past the rect *plus*
/// `spacing.item_spacing.y`, so a band followed by its own hairline would float
/// that hairline six points below the band it belongs to, a rule that is
/// decoration rather than structure. A header is band-then-rule with nothing in
/// between, so the gap is suppressed here and handed back to the caller
/// afterwards, which keeps the rhythm of everything *after* the header exactly
/// as it was.
fn header_band<R>(
    ui: &mut Ui,
    width: f32,
    height: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let gap = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    let band = ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        Layout::left_to_right(Align::Center),
        add,
    );
    ui.spacing_mut().item_spacing.y = gap;
    band
}

pub fn pane_header<R>(
    ui: &mut Ui,
    title: &str,
    count: Option<&str>,
    actions: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    pane_header_inset(ui, title, count, 0.0, actions)
}

pub fn pane_header_inset<R>(
    ui: &mut Ui,
    title: &str,
    count: Option<&str>,
    inset: f32,
    actions: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    // The width is read ONCE, before the band is allocated. A band takes the
    // full available width, so re-reading it afterwards asks a horizontal
    // layout for the air its own first child already used, which is zero, and
    // which silently produced a hairline one pixel wide.
    let width = ui.available_width();
    let band = header_band(ui, width, PANE_HEADER_HEIGHT, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // Spacing is zeroed first, so the inset is exactly `inset`.
        ui.add_space(inset);
        let (font, ink) = pane_title();
        ui.label(RichText::new(title).font(font).color(ink));
        if let Some(count) = count {
            ui.add_space(PANE_HEADER_CHIP_GAP);
            count_chip(ui, count);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), actions)
            .inner
    });
    pane_header_rule(ui, width);
    band
}

fn pane_header_rule(ui: &mut Ui, width: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, HEADER_RULE_HEIGHT), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::RULE_STRUCTURAL);
    rect
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColumnAlign {
    Start,
    End,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PaneColumn {
    pub label: &'static str,
    pub x: f32,
    pub align: ColumnAlign,
}

impl PaneColumn {
    pub const fn start(label: &'static str, x: f32) -> Self {
        Self {
            label,
            x,
            align: ColumnAlign::Start,
        }
    }

    pub const fn end(label: &'static str, inset: f32) -> Self {
        Self {
            label,
            x: inset,
            align: ColumnAlign::End,
        }
    }

    pub fn origin(&self, row: Rect) -> f32 {
        match self.align {
            ColumnAlign::Start => row.left() + self.x,
            ColumnAlign::End => row.right() - self.x,
        }
    }
}

pub fn column_header(ui: &mut Ui, row: Rect, columns: &[PaneColumn]) -> Rect {
    let (font, ink) = pane_title();
    let width = ui.available_width();
    let band = header_band(ui, width, COLUMN_HEADER_HEIGHT, |_| {});
    let rule = pane_header_rule(ui, width);
    let centre_y = rule.top() - HEADER_RULE_HEIGHT - COLUMN_HEADER_HEIGHT / 2.0;
    for column in columns {
        let galley = ui
            .painter()
            .layout_no_wrap(column.label.to_owned(), font.clone(), ink);
        let x = match column.align {
            ColumnAlign::Start => column.origin(row),
            ColumnAlign::End => column.origin(row) - galley.size().x,
        };
        ui.painter()
            .galley(Pos2::new(x, centre_y - galley.size().y / 2.0), galley, ink);
    }
    let _ = band;
    rule
}

pub fn column_cell<R>(
    ui: &mut Ui,
    columns: &[PaneColumn],
    index: usize,
    cell: impl FnOnce(&mut Ui) -> R,
) -> R {
    let want = columns[index].origin(ui.max_rect());
    let have = ui.cursor().left();
    if want > have {
        ui.add_space(want - have);
    }
    cell(ui)
}

fn pane_title() -> (FontId, Color32) {
    (
        FontId::new(TYPE_SECTION, FontFamily::Proportional),
        Palette::INK_3,
    )
}

pub fn group_title(ui: &mut Ui, title: &str) {
    ui.label(micro_header(title));
}

fn micro_header(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
        .color(Palette::INK_3)
}
pub const AVATAR_SIZE: f32 = 28.0;

pub fn avatar_initials(ui: &mut Ui, name: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(AVATAR_SIZE), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.circle_filled(
        rect.center(),
        AVATAR_SIZE / 2.0,
        tint_over_bg(Palette::BRAND, BADGE_TINT),
    );
    let galley = painter.layout_no_wrap(
        initials_of(name),
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        Palette::INK,
    );
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        Palette::INK,
    );
}

fn initials_of(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().and_then(|w| w.chars().next());
    let last = words.next_back().and_then(|w| w.chars().next());
    match (first, last) {
        (Some(a), Some(b)) => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), None) => a.to_uppercase().to_string(),
        (None, _) => String::new(),
    }
}

pub fn alert_box(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(Palette::SURFACE_WARNING)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::splat(ALERT_ICON_SIZE), Sense::hover());
                icons::paint_icon(
                    ui.painter(),
                    Pos2::new(rect.left(), rect.center().y - ALERT_ICON_SIZE / 2.0),
                    ALERT_ICON_SIZE,
                    Icon::ALERT_TRIANGLE,
                    Palette::STATE_WARNING,
                );
                let width = ui.available_width();
                ui.add_sized(
                    Vec2::new(width.max(0.0), 0.0),
                    egui::Label::new(
                        RichText::new(text)
                            .font(FontId::new(MICRO_TEXT, FontFamily::Proportional))
                            .color(Palette::STATE_WARNING),
                    )
                    .wrap(),
                );
            });
        });
}

pub fn churn_bar(ui: &mut Ui, added: usize, removed: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, CHURN_BAR_HEIGHT), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = CornerRadius::same(CHURN_BAR_HEIGHT as u8);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, Palette::SURFACE_3);
    let total = added + removed;
    if total == 0 {
        return;
    }
    let added_width = rect.width() * added as f32 / total as f32;
    if added_width > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(added_width, rect.height())),
            radius,
            Palette::STATE_SUCCESS,
        );
    }
    let removed_width = rect.width() - added_width;
    if removed_width > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left() + added_width, rect.top()),
                Vec2::new(removed_width, rect.height()),
            ),
            radius,
            Palette::STATE_ERROR,
        );
    }
}
