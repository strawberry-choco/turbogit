use egui::{Rect, Ui};

use crate::ui::components::{RowShell, RowState};

pub fn paint_row(ui: &Ui, rect: Rect, state: RowState) {
    crate::ui::components::row_shell(ui, rect, state, RowShell::Fill);
}
