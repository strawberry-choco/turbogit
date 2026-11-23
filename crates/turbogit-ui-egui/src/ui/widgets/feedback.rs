use egui::{Color32, CornerRadius, Response, RichText, Ui, vec2};

use crate::theme::{MARK_RADIUS, Palette, TYPE_BODY, chrome_font};

const ACCENT_BAR_WIDTH: f32 = 3.0;
const ACCENT_BAR_HEIGHT: f32 = 18.0;

pub fn accent_bar(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ACCENT_BAR_WIDTH, ACCENT_BAR_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, CornerRadius::same(MARK_RADIUS), color);
}

pub fn inline_error(ui: &mut Ui, message: impl Into<String>) -> Response {
    ui.label(
        RichText::new(message.into())
            .font(chrome_font(TYPE_BODY))
            .color(Palette::STATE_ERROR),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyedReadPresentation<'a> {
    Waiting(&'a str),
    Failed(&'a str),
}

pub fn keyed_read_presentation(ui: &mut Ui, presentation: KeyedReadPresentation<'_>) {
    match presentation {
        KeyedReadPresentation::Waiting(message) => {
            ui.spinner();
            ui.label(message);
        }
        KeyedReadPresentation::Failed(message) => {
            inline_error(ui, message);
        }
    }
}
