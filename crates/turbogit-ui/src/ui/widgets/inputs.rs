//! Shared text and search input controls.

use egui::{
    CornerRadius, Frame, Margin, Response, Stroke, StrokeKind, TextEdit, Ui, WidgetInfo, WidgetType,
};

use crate::theme::{CONTROL_RADIUS, Palette};
use crate::ui::icons::{self, Icon};

/// Icon size used by search inputs and contained feedback.
pub(super) const INPUT_ICON_SIZE: f32 = 14.0;

// --- Inputs ------------------------------------------------------------------

/// Single-line text input: SURFACE_3 fill, LINE border, BRAND focus ring.
pub fn text_input(ui: &mut Ui, placeholder: &str, buf: &mut String) -> Response {
    input_frame(ui, placeholder, buf, false)
}

/// Search input: like [`text_input`] plus a leading magnifier icon.
pub fn search_input(ui: &mut Ui, placeholder: &str, buf: &mut String) -> Response {
    input_frame(ui, placeholder, buf, true)
}

/// Whether `haystack` contains `query`, case-insensitively — the one match
/// rule behind every filter paired with [`search_input`].
///
/// The query is **already normalised** by the caller, and how far is a per-surface
/// decision this helper must not make: three surfaces trim, two deliberately do not.
/// Pass the query exactly as the surface built it.
pub fn filter_matches(haystack: &str, query: &str) -> bool {
    haystack.to_lowercase().contains(query)
}

fn input_frame(ui: &mut Ui, placeholder: &str, buf: &mut String, search_icon: bool) -> Response {
    let avail_w = ui.available_width();
    let icon_area = if search_icon {
        INPUT_ICON_SIZE + 4.0
    } else {
        0.0
    };
    // Frame margins (8×2) + stroke (1×2) leave the rest for the edit.
    let edit_w = (avail_w - 16.0 - 2.0 - icon_area).max(40.0);

    let frame = Frame::new()
        .fill(Palette::SURFACE_3)
        .stroke(Stroke::new(1.0, Palette::LINE))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(Margin::symmetric(8, 4));

    let outer = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            if search_icon {
                icons::icon(ui, Icon::SEARCH, INPUT_ICON_SIZE, Palette::INK_3);
                ui.add_space(4.0);
            }
            let resp = ui.add(
                TextEdit::singleline(buf)
                    .hint_text(placeholder)
                    .desired_width(edit_w)
                    .frame(egui::Frame::new()),
            );
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, placeholder));
            resp
        })
        .inner
    });

    let edit_response = outer.inner;
    if edit_response.has_focus() {
        ui.painter().rect_stroke(
            outer.response.rect.expand(1.0),
            CornerRadius::same(CONTROL_RADIUS),
            Stroke::new(1.0, Palette::BRAND),
            StrokeKind::Outside,
        );
    }
    edit_response
}
