//! Shared chip geometry and the semantic chip/badge vocabulary built on it.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Response, Sense, Ui, Vec2, WidgetInfo,
    WidgetType,
};

use super::controls::{focus_ring, tint_over_bg};
use crate::theme::{CONTROL_RADIUS, Palette, TYPE_CONTROL};

/// Alpha used when tinting an accent over [`Palette::BG`] for badge fills.
pub const BADGE_TINT: f32 = 0.18;
/// Shared chip height (pill).
pub const CHIP_HEIGHT: f32 = 18.0;
/// Horizontal text inset on each side of the shared non-interactive chip.
pub const CHIP_PAD_X: f32 = 6.0;
const MICRO_TEXT: f32 = TYPE_CONTROL;

// --- Chip decisions ----------------------------------------------------------

/// Background/foreground pair painted by the badge family.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChipColors {
    pub bg: Color32,
    pub fg: Color32,
}

/// File-status badge kinds (`.tg-badge`, spec §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    Neutral,
    Added,
    Modified,
    Deleted,
}

impl BadgeKind {
    /// The status token this badge kind is decided by (R1.4 mapping).
    pub fn accent(self) -> Color32 {
        match self {
            Self::Neutral => Palette::INK_2,
            Self::Added => Palette::STATE_SUCCESS,
            Self::Modified => Palette::STATE_WARNING,
            Self::Deleted => Palette::STATE_ERROR,
        }
    }

    /// Colors: neutral sits on the input/badge surface token; status badges
    /// tint their accent over BG with accent-colored ink (§2.1/§2.2 usage).
    pub fn colors(self) -> ChipColors {
        let fg = self.accent();
        let bg = match self {
            Self::Neutral => Palette::SURFACE_3,
            _ => tint_over_bg(fg, BADGE_TINT),
        };
        ChipColors { bg, fg }
    }
}

/// Git ref chip kinds (`.tg-label`, spec §7.1/§8.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    Branch,
    Remote,
    Tag,
}

impl RefKind {
    /// The token this ref kind is decided by: branch=brand, remote=success,
    /// tag=warning.
    ///
    /// This is the *colour vocabulary* half of the ref-chip role, and it
    /// outlived the ref-label render function: `ui::log_window` maps its own
    /// reference kind onto [`RefKind::accent`] for colouring and never renders
    /// a ref label. The retired `RefKind::colors` — the solid-pill colour pair
    /// only that render function needed — went with it. Keep `accent()`.
    pub fn accent(self) -> Color32 {
        match self {
            Self::Branch => Palette::BRAND,
            Self::Remote => Palette::STATE_SUCCESS,
            Self::Tag => Palette::STATE_WARNING,
        }
    }
}

// --- Chips -------------------------------------------------------------------

/// Status badge (`.tg-badge`): 18px pill, tinted background + accent ink.
pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) -> Response {
    chip(ui, text, kind.colors())
}

/// Geometry shared by the non-interactive chip family.
///
/// This is deliberately a value, not a semantic chip type: branch, status,
/// reference, welcome, and quiet wrappers keep their own meaning and may
/// intentionally choose different metrics. This contract only centralizes
/// the low-level decisions that are actually common.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChipGeometry {
    /// Outer chip height.
    pub height: f32,
    /// Horizontal text inset on each side.
    pub pad_x: f32,
    /// Corner radius in logical pixels.
    pub radius: f32,
}

impl ChipGeometry {
    /// Measure the full chip for a laid-out label.
    pub fn size(self, galley: &egui::Galley) -> Vec2 {
        Vec2::new(galley.size().x + self.pad_x * 2.0, self.height)
    }

    /// Place a chip with its right edge at `right_edge` and centre on `cy`.
    pub fn rect_right(self, right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
        let size = self.size(galley);
        Rect::from_min_size(Pos2::new(right_edge - size.x, cy - self.height / 2.0), size)
    }

    /// Place a label centred in a chip rect.
    pub fn text_origin(self, rect: Rect, galley: &egui::Galley) -> Pos2 {
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        )
    }

    /// Paint this geometry's body and its centred label.
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

/// The standard non-interactive chip geometry: 18px high, 6px inset, 9px
/// radius. Specialized wrappers may supply another geometry without changing
/// this shared contract.
pub const CHIP_GEOMETRY: ChipGeometry = ChipGeometry {
    height: CHIP_HEIGHT,
    pad_x: CHIP_PAD_X,
    // The theme's compact `CHIP_RADIUS` is 3 px; the shared chip family is the
    // full-height pill role, whose theme-owned radius remains `PILL_RADIUS`.
    radius: crate::theme::PILL_RADIUS as f32,
};

/// The chip's corner radius, derived from the shared geometry.
pub fn chip_radius() -> CornerRadius {
    CornerRadius::same(CHIP_GEOMETRY.radius as u8)
}

/// The rect a shared chip occupies when its right edge is at `right_edge`.
pub fn chip_rect_right(right_edge: f32, cy: f32, galley: &egui::Galley) -> Rect {
    CHIP_GEOMETRY.rect_right(right_edge, cy, galley)
}

/// Place a shared chip's label centred in its rect.
pub fn chip_text_origin(rect: Rect, galley: &egui::Galley) -> Pos2 {
    CHIP_GEOMETRY.text_origin(rect, galley)
}

/// Paint one shared-geometry chip into a caller-placed rect.
pub fn paint_chip(
    painter: &egui::Painter,
    rect: Rect,
    galley: std::sync::Arc<egui::Galley>,
    bg: Color32,
    ink: Color32,
) {
    CHIP_GEOMETRY.paint(painter, rect, galley, bg, ink);
}

fn chip(ui: &mut Ui, text: &str, colors: ChipColors) -> Response {
    let font_id = FontId::new(MICRO_TEXT, FontFamily::Proportional);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font_id, colors.fg);
    let size = CHIP_GEOMETRY.size(&galley);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

    paint_chip(ui.painter(), rect, galley, colors.bg, colors.fg);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response
}
/// Commit hash as a clickable chip: mono hash on `SURFACE_3` (redesign
/// issue 03). The click is *reported*, not acted on — the caller defers the
/// copy like every other pane interaction (plan §1.3).
pub fn hash_chip(ui: &mut Ui, hash: &str, hint: &str) -> Response {
    let font = FontId::new(crate::theme::TYPE_BODY, FontFamily::Monospace);
    let galley = ui
        .painter()
        .layout_no_wrap(hash.to_owned(), font, Palette::BRAND);
    let size = Vec2::new(galley.size().x + CHIP_PAD_X * 2.0, CHIP_HEIGHT + 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), Palette::SURFACE_3);
        ui.painter().galley(
            Pos2::new(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Palette::BRAND,
        );
    }
    focus_ring(ui, &response);
    response.on_hover_text(hint)
}
