//! Shared row-fill painting.

use egui::{Color32, CornerRadius, Rect, Ui};

use crate::theme::CONTROL_RADIUS;
use crate::ui::components::{RowState, row_fill};

// --- Trees & lists -----------------------------------------------------------

/// Paint one row's fill for a [`RowState`] in a rect the caller already
/// allocated.
///
/// There is no shared *row-layout* wrapper any more: the convenience
/// fixed-height tree row was retired with its private row implementation
/// (nothing outside the vocabulary ever called it, and it is not a universal
/// row for commits, files, diff lines, or conflict panes). This is what
/// survives for the hand-painted tables — the settings category rail, the
/// rebase todo, the commit window's file rows — that allocate a rect, paint
/// behind their content and still want the one row-state decision. The corner
/// is [`CONTROL_RADIUS`], which is what every such row has always used; a
/// surface that rounds its rows differently (the sidebar's full-bleed band,
/// blame's dense rows) is a real geometry difference and stays local rather
/// than being flattened by this call.
pub fn paint_row(ui: &Ui, rect: Rect, state: RowState) {
    let fill = row_fill(state);
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), fill);
    }
}
