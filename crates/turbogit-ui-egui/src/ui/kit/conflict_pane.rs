use crate::theme::{MARK_RADIUS, MARKER_TINT, Palette, SECTION_TINT, TYPE_CHIP};
use crate::ui::components;
use crate::ui::widgets::{self, tint_over_bg};
use egui::{Color32, CornerRadius, Frame, Margin, Pos2, Rect, RichText, Stroke, Ui, Vec2};

pub(crate) fn equal_panes<R>(ui: &mut Ui, add: impl FnOnce(&mut [Ui]) -> R) -> R {
    ui.columns(3, add)
}

const HEADER_AIR: f32 = 8.0;

pub(crate) fn pane_header(ui: &mut Ui, title: &str, focused: bool) {
    let pane_left = ui.available_rect_before_wrap().left();
    let top = ui.cursor().top();
    ui.horizontal(|ui| {
        ui.add_space(HEADER_AIR);
        // The air and the header are a row, but the header itself is a *block*:
        // a band over a hairline. Giving it a vertical scope of its own is what
        // lets the rule be as wide as the band, a horizontal scope has already
        // spent the whole width on the band, so a rule measured there is one
        // pixel wide.
        ui.vertical(|ui| widgets::pane_header(ui, title, None, |_ui| {}));
    });
    if focused {
        let bottom = ui.cursor().top();
        components::paint_rail(
            ui.painter(),
            Rect::from_min_max(Pos2::new(pane_left, top), Pos2::new(pane_left, bottom)),
        );
    }
}

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

fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, MARKER_TINT)
}

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
