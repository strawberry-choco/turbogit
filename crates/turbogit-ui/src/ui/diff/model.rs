pub(crate) use turbogit_app::diff_model::{
    DiffModel, DisplayRow, FileMeta, Highlight, PaneKind, Row, RowKind, pane_kind,
};

pub(super) fn is_file_row(row: &Row) -> bool {
    row.kind == RowKind::Meta
        && (row.text.starts_with("diff --git ") || row.text.starts_with("diff --cc "))
}

pub(super) const ROW_H: f32 = 22.0;
pub(super) const PANE_HEADER_H: f32 = 28.0;
pub(super) const SIGN_W: f32 = 16.0;
pub(super) const NUM_W: f32 = 40.0;
pub(super) const TEXT_X: f32 = SIGN_W + NUM_W + 12.0;

pub(super) fn mono_font() -> egui::FontId {
    crate::theme::data_font(crate::theme::TYPE_BODY)
}
