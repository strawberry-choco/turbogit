//! Shared tree/list row layout and row-fill painting.

use egui::{
    Align, Color32, CornerRadius, Layout, Rect, Response, Sense, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetType,
};

use super::controls::focus_ring;
use crate::theme::{CONTROL_RADIUS, FILE_ROW_HEIGHT, Palette};
use crate::ui::components::{RowState, row_fill};

// --- Trees & lists -----------------------------------------------------------

/// Fixed-height tree row: hover SURFACE_2, selected = BRAND fill with
/// brand-ink content (§7.1/§7.2). The height is the shared
/// [`FILE_ROW_HEIGHT`], so a vocabulary row and a changes-tree row are one row.
pub fn tree_row(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) -> Response {
    row_impl(ui, selected, contents)
}

fn row_impl(ui: &mut Ui, selected: bool, contents: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width();
    // Reserve the exact row space up-front so the centered cross-layout can
    // never swallow the parent's remaining height.
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, FILE_ROW_HEIGHT), Sense::hover());

    // Contents live strictly inside the reserved rect.
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    if selected {
        // Selected rows flip content ink to brand ink (§7.2).
        child.visuals_mut().override_text_color = Some(Palette::BRAND_INK);
    }
    contents(&mut child);

    // Interact *after* the content is registered so the row — not the labels
    // inside it — owns hover and click events across its full rect.
    let id = ui.auto_id_with("tree_row");
    let response = ui.interact(rect, id, Sense::click());

    let fill = row_fill(RowState::from_flags(selected, response.hovered()));
    if fill != Color32::TRANSPARENT {
        // Paint behind the already-emitted content shapes.
        let mut bg = ui.painter().clone();
        bg.set_layer_id(egui::LayerId::new(egui::Order::Background, response.id));
        bg.rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
    focus_ring(ui, &response);

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), ""));
    response
}

/// Paint one row's fill for a [`RowState`] in a rect the caller already
/// allocated.
///
/// [`tree_row`] lays its own row out and needs nothing here; this is for the
/// hand-painted tables — the settings category rail, the rebase todo, the commit
/// window's file rows — that allocate a rect, paint behind their content and
/// still want the one row-state decision. The corner is [`CONTROL_RADIUS`],
/// which is what every such row has always used; a surface that rounds its rows
/// differently (the sidebar's full-bleed band, blame's dense rows) is a real
/// geometry difference and stays local rather than being flattened by this call.
pub fn paint_row(ui: &Ui, rect: Rect, state: RowState) {
    let fill = row_fill(state);
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
}
