//! Shared presentation primitives for the three-pane conflict grammar.
//!
//! This module knows how a conflict pane looks, not what a conflict is. Callers
//! provide labels and already-composed visual text; resolution choices and
//! mutable editor state remain outside this boundary so the kit cannot become a
//! second stateful editor.

use crate::theme::{MARK_RADIUS, MARKER_TINT, Palette, SECTION_TINT, TYPE_CHIP};
use crate::ui::components;
use crate::ui::widgets::{self, tint_over_bg};
use egui::{Color32, CornerRadius, Frame, Margin, Pos2, Rect, RichText, Stroke, Ui, Vec2};

/// Allocate the three equal-width columns used by every conflict surface.
///
/// Keeping the column count and the call to egui's column layout here makes
/// the shared geometry explicit without changing the surface's render order.
pub(crate) fn equal_panes<R>(ui: &mut Ui, add: impl FnOnce(&mut [Ui]) -> R) -> R {
    ui.columns(3, add)
}

/// Air between a conflict pane's edge and its header.
///
/// This grammar's three columns sit flush against one another — there is no
/// pane frame to borrow inset from — so the shared header is inset by the
/// grammar's own 8pt. It is also where the focus rail lives: a 2pt accent rail
/// in the air, beside the title, rather than a stroke drawn around the header
/// and the content under it.
const HEADER_AIR: f32 = 8.0;

/// Paint one conflict-pane header, optionally marking the focused pane.
///
/// **The shared pane header, not a header of this module's own.** This used to
/// be a private implementation: its own `SURFACE` box, its own 8/5 margin, a
/// bold `TYPE_BODY` label in `INK`, and a 2pt brand *stroke* around the whole
/// band when focused. Every one of those was a second answer to "what does the
/// top of a pane look like", and it drifted — a bold 12px `INK` title is not
/// what any other pane wears, and a stroke means "this floats", which a
/// pane's own header does not.
///
/// So it delegates: [`widgets::pane_header`] owns the band, the title treatment
/// and the one hairline, and the focus marker left behind is R1's 2pt accent
/// rail at the pane's leading edge — painted by the one rail painter, the same
/// width every selected row's rail is.
pub(crate) fn pane_header(ui: &mut Ui, title: &str, focused: bool) {
    let pane_left = ui.available_rect_before_wrap().left();
    let top = ui.cursor().top();
    ui.horizontal(|ui| {
        ui.add_space(HEADER_AIR);
        // The air and the header are a row, but the header itself is a *block*:
        // a band over a hairline. Giving it a vertical scope of its own is what
        // lets the rule be as wide as the band — a horizontal scope has already
        // spent the whole width on the band, so a rule measured there is one
        // pixel wide.
        ui.vertical(|ui| widgets::pane_header(ui, title, None, |_ui| {}));
    });
    if focused {
        let bottom = ui.cursor().top();
        // A zero-width rect on purpose: `paint_rail` owns the width — it reads
        // the token and paints `[rect.left(), rect.left() + RAIL_WIDTH)`. Naming
        // the width here would be a second definition of it under this module's
        // own roof, which is exactly what the one-painter ratchet refuses.
        components::paint_rail(
            ui.painter(),
            Rect::from_min_max(Pos2::new(pane_left, top), Pos2::new(pane_left, bottom)),
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
