use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, Sense, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use super::controls::tint_over_bg;
use crate::theme::{
    CHIP_RADIUS, CONTROL_RADIUS, PILL_RADIUS, Palette, TYPE_CHIP, TYPE_CONTROL, data_font,
};

pub const BADGE_TINT: f32 = 0.18;
pub const CHIP_HEIGHT: f32 = 18.0;
pub const CHIP_PAD_X: f32 = 6.0;
const MICRO_TEXT: f32 = TYPE_CONTROL;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChipColors {
    pub bg: Color32,
    pub fg: Color32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    Neutral,
    Added,
    Modified,
    Deleted,
}

impl BadgeKind {
    pub fn accent(self) -> Color32 {
        match self {
            Self::Neutral => Palette::INK_2,
            Self::Added => Palette::STATE_SUCCESS,
            Self::Modified => Palette::STATE_WARNING,
            Self::Deleted => Palette::STATE_ERROR,
        }
    }

    pub fn colors(self) -> ChipColors {
        let fg = self.accent();
        let bg = match self {
            Self::Neutral => Palette::SURFACE_3,
            _ => tint_over_bg(fg, BADGE_TINT),
        };
        ChipColors { bg, fg }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    Branch,
    Remote,
    Tag,
}

impl RefKind {
    pub fn accent(self) -> Color32 {
        match self {
            Self::Branch => Palette::BRAND,
            Self::Remote => Palette::STATE_SUCCESS,
            Self::Tag => Palette::STATE_WARNING,
        }
    }
}

pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) -> Response {
    chip_with(
        ui,
        text,
        CHIP_GEOMETRY,
        FontId::new(MICRO_TEXT, FontFamily::Proportional),
        kind.colors(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChipGeometry {
    pub height: f32,
    pub pad_x: f32,
    pub radius: f32,
}

impl ChipGeometry {
    pub fn size(self, galley: &egui::Galley) -> Vec2 {
        Vec2::new(galley.size().x + self.pad_x * 2.0, self.height)
    }

    pub fn rect_right(self, right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
        let size = self.size(galley);
        Rect::from_min_size(Pos2::new(right_edge - size.x, cy - self.height / 2.0), size)
    }

    pub fn text_origin(self, rect: Rect, galley: &egui::Galley) -> Pos2 {
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        )
    }

    pub fn paint(
        self,
        painter: &egui::Painter,
        rect: Rect,
        galley: std::sync::Arc<egui::Galley>,
        bg: Color32,
        ink: Color32,
    ) {
        painter.rect_filled(rect, CornerRadius::same(self.radius as u8), bg);
        painter.galley(self.text_origin(rect, &galley), galley, ink);
    }
}

pub const CHIP_GEOMETRY: ChipGeometry = ChipGeometry {
    height: CHIP_HEIGHT,
    pad_x: CHIP_PAD_X,
    radius: PILL_RADIUS as f32,
};

pub const COMPACT_CHIP_GEOMETRY: ChipGeometry = ChipGeometry {
    height: CHIP_HEIGHT,
    pad_x: CHIP_PAD_X,
    radius: CHIP_RADIUS as f32,
};

fn chip_font() -> FontId {
    data_font(TYPE_CHIP)
}

pub fn chip_radius() -> CornerRadius {
    CornerRadius::same(CHIP_GEOMETRY.radius as u8)
}

pub fn compact_chip_radius() -> CornerRadius {
    CornerRadius::same(COMPACT_CHIP_GEOMETRY.radius as u8)
}

pub fn chip_rect_right(right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
    CHIP_GEOMETRY.rect_right(right_edge, cy, galley)
}

pub fn chip_text_origin(rect: Rect, galley: &egui::Galley) -> Pos2 {
    CHIP_GEOMETRY.text_origin(rect, galley)
}

pub fn paint_chip(
    painter: &egui::Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    bg: Color32,
    ink: Color32,
) {
    CHIP_GEOMETRY.paint(painter, rect, galley, bg, ink);
}

pub const REF_CHIP_COLORS: ChipColors = ChipColors {
    bg: Palette::RAISED_ON_CARD,
    fg: Palette::INK_2,
};

pub const CURRENT_CHIP_COLORS: ChipColors = ChipColors {
    bg: Palette::ROW_SELECTED,
    fg: Palette::ACCENT_TEXT,
};

pub const COUNT_CHIP_COLORS: ChipColors = ChipColors {
    bg: Palette::RAISED,
    fg: Palette::INK_2,
};

pub fn ref_chip(ui: &mut Ui, text: &str) -> Response {
    chip_with(
        ui,
        text,
        COMPACT_CHIP_GEOMETRY,
        chip_font(),
        REF_CHIP_COLORS,
    )
}

pub fn current_chip(ui: &mut Ui, text: &str) -> Response {
    chip_with(
        ui,
        text,
        COMPACT_CHIP_GEOMETRY,
        chip_font(),
        CURRENT_CHIP_COLORS,
    )
}

pub fn count_chip(ui: &mut Ui, text: &str) -> Response {
    chip_with(
        ui,
        text,
        COMPACT_CHIP_GEOMETRY,
        chip_font(),
        COUNT_CHIP_COLORS,
    )
}

fn chip_with(
    ui: &mut Ui,
    text: &str,
    geometry: ChipGeometry,
    font: FontId,
    colors: ChipColors,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, colors.fg);
    let size = geometry.size(&galley);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    geometry.paint(ui.painter(), rect, galley, colors.bg, colors.fg);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}

pub fn hash_chip(ui: &mut Ui, hash: &str, hint: &str) -> Response {
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Monospace);
    let galley = ui
        .painter()
        .layout_no_wrap(hash.to_owned(), font, Palette::LINK);
    let size = Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), Palette::SURFACE_3);
        ui.painter().galley(
            Pos2::new(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::LINK,
        );
    }
    if hint.is_empty() {
        response
    } else {
        response.on_hover_text(hint)
    }
}
