//! Shared presentation primitives for the three-pane conflict grammar.
//!
//! This module knows how a conflict pane looks, not what a conflict is. Callers
//! provide labels and already-composed visual text; resolution choices and
//! mutable editor state remain outside this boundary so the kit cannot become a
//! second stateful editor.

use crate::theme::{MARK_RADIUS, MARKER_TINT, Palette, SECTION_TINT, TYPE_BODY, TYPE_CHIP};
use crate::ui::widgets::tint_over_bg;
use egui::{Color32, CornerRadius, Frame, Margin, Rect, RichText, Stroke, Ui, Vec2};

/// Allocate the three equal-width columns used by every conflict surface.
///
/// Keeping the column count and the call to egui's column layout here makes
/// the shared geometry explicit without changing the surface's render order.
pub(crate) fn equal_panes<R>(ui: &mut Ui, add: impl FnOnce(&mut [Ui]) -> R) -> R {
    ui.columns(3, add)
}

/// Paint one conflict-pane header band, optionally marking the focused pane.
pub(crate) fn pane_header(ui: &mut Ui, title: &str, focused: bool) {
    let resp = Frame::new()
        .fill(Palette::SURFACE)
        .inner_margin(Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(16.0);
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(TYPE_BODY)
                    .color(Palette::INK),
            );
        });
    if focused {
        ui.painter().rect_stroke(
            resp.response.rect,
            CornerRadius::same(MARK_RADIUS),
            Stroke::new(2.0, Palette::BRAND),
            egui::StrokeKind::Inside,
        );
    }
}

/// Semantic side of a three-pane conflict view.
///
/// The type keeps the caller from passing arbitrary accent colors: Local is
/// always the information role and Incoming is always the error role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Local,
    Incoming,
}

impl Side {
    fn fill(self) -> Color32 {
        match self {
            Self::Local => tint_over_bg(Palette::STATE_INFO, SECTION_TINT),
            Self::Incoming => tint_over_bg(Palette::STATE_ERROR, SECTION_TINT),
        }
    }

    fn strip(self) -> Color32 {
        match self {
            Self::Local => Palette::STATE_INFO,
            Self::Incoming => Palette::STATE_ERROR,
        }
    }
}

/// Background for a conflict marker strip (warning tint over the app surface).
fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, MARKER_TINT)
}

/// Paint one raw-marker strip row.
pub(crate) fn marker_strip(ui: &mut Ui, glyph: &str) {
    Frame::new()
        .fill(marker_bg())
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(glyph)
                    .monospace()
                    .size(TYPE_CHIP)
                    .color(Palette::STATE_WARNING),
            );
        });
}

/// Paint one tinted side section and its semantic accent strip.
pub(crate) fn side_section(ui: &mut Ui, text: &str, side: Side) {
    let resp = Frame::new()
        .fill(side.fill())
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(text).monospace().color(Palette::INK));
        });
    let r = resp.response.rect;
    ui.painter().rect_filled(
        Rect::from_min_size(r.left_top(), Vec2::new(3.0, r.height())),
        0.0,
        side.strip(),
    );
}

/// Paint the read-only composed Result cell.
///
/// `text` is already-owned, composed visual content. `None` is the unresolved
/// visual state and lets this presentation primitive own the placeholder and
/// warning fill; the surface remains responsible for interpreting resolution
/// choices. The inline surface uses this for its read-only Result, while the
/// resolver uses it only for inactive cells and keeps its active TextEdit
/// outside the kit.
pub(crate) fn result_cell(ui: &mut Ui, text: Option<String>) {
    let (text, fill) = match text {
        Some(text) => (text, Palette::SURFACE),
        None => ("<< unresolved >>".to_owned(), marker_bg()),
    };
    let resp = Frame::new()
        .fill(fill)
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(text).monospace().color(Palette::INK));
        });
    ui.painter().rect_stroke(
        resp.response.rect,
        CornerRadius::same(MARK_RADIUS),
        Stroke::new(2.0, Palette::BRAND),
        egui::StrokeKind::Inside,
    );
}
