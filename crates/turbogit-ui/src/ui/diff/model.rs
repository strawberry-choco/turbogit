//! The diff family's paint metrics, plus the display model the app layer owns.
//!
//! Row parsing, the per-file section scan and the display-row fold moved to
//! [`turbogit_app::diff_model`] with the keyed read (ADR-0021): a diff's model
//! is built when its content settles and arrives carrying it, so the paint path
//! neither parses nor memoizes — which is what retired the thread-local
//! `PARSED_ROWS` memo and the whole-patch-text compare it paid every frame.
//! What stays here is what only painting can own: the metrics every row band is
//! measured in, and the diff family's monospaced face.

pub use turbogit_app::diff_model::{
    DiffModel, DisplayRow, FileMeta, PaneKind, Row, RowKind, line_counts, pane_kind,
};

// --- metrics -----------------------------------------------------------------

/// Rendered height of one diff line.
pub(super) const ROW_H: f32 = 22.0;
/// Side-by-side pane header height (spec §8.4).
pub(super) const PANE_HEADER_H: f32 = 28.0;
/// Width of the +/- sign column in a unified row.
pub(super) const SIGN_W: f32 = 16.0;
/// Width of the line-number gutter column.
pub(super) const NUM_W: f32 = 40.0;
/// X offset of the code text within a unified row.
pub(super) const TEXT_X: f32 = SIGN_W + NUM_W + 12.0;

/// The diff family's monospaced code face, at the shared body size.
///
/// A display-model module has no business choosing a typeface, and it no longer
/// does: this is `pub(super)` only so `diff/`'s three painters ask one question,
/// and every value in it comes from a `theme` role rather than a pixel (the
/// conformance sweep's issue 17). A call site outside `diff/` should use
/// [`crate::theme::data_font`] directly.
pub(super) fn mono_font() -> egui::FontId {
    crate::theme::data_font(crate::theme::TYPE_BODY)
}
