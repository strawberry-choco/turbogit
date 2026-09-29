//! Shared row painting.
//!
//! The row vocabulary is this module plus the row-state grammar it delegates to
//! ([`crate::ui::components`]): the fill and its radius are decided there —
//! [`crate::ui::components::fill`] and [`crate::ui::components::row_fill`] — and
//! so is the one accent rail, [`crate::ui::components::paint_rail`], so a row's
//! resting, hover and selected paint and the rail beside it cannot come from two
//! opinions.
//!
//! Why the shell is *not* a `pub fn` here: `widgets::rows` is a private module
//! whose public surface is exactly what `widgets/mod.rs` re-exports, so a
//! primitive added here is unreachable from every screen — and the workspace
//! `deny(warnings)` turns that into a compile error rather than a silently
//! unreachable helper. The rail and the shell therefore live in the row-state
//! grammar beside the fill they complement, and this module is the shared
//! spelling of one of its two variants.

use egui::{Rect, Ui};

use crate::ui::components::{RowShell, RowState};

/// Paint one row's fill for a [`RowState`] in a rect the caller already
/// allocated.
///
/// This is [`RowShell::Fill`] — the base form, for the hand-painted tables that
/// want the one row-state decision and bring their own selection marker. The
/// other variant, [`RowShell::Railed`], adds the leading-edge accent rail and is
/// called through [`crate::ui::components::row_shell`] by the screens whose
/// chosen rows carry one (the log; the sidebar and the branches tree paint their
/// own, beside a band this function does not own).
///
/// There is no shared *row-layout* wrapper any more: the convenience
/// fixed-height tree row was retired with its private row implementation
/// (nothing outside the vocabulary ever called it, and it is not a universal
/// row for commits, files, diff lines, or conflict panes). This is what
/// survives for the hand-painted tables — the settings category rail, the
/// rebase todo, the commit window's file rows — that allocate a rect, paint
/// behind their content and still want the one row-state decision.
///
/// The decision itself and the [`CONTROL_RADIUS`](crate::theme::CONTROL_RADIUS)
/// that goes with it now live in `components::fill` (ticket 03), beside
/// `row_fill`: the fill, the radius and the "no text geometry" rule are one
/// construction site, and this remains the seven-call-site spelling. A surface
/// that rounds its rows differently (the sidebar's full-bleed band, blame's
/// dense rows) is a real geometry difference and stays local rather than being
/// flattened by this call.
///
/// The row's **leading-edge rail** is the companion: `components::paint_rail`,
/// the one rail painter that absorbed the sidebar's stroked segment and the diff
/// pane's filled rect. It is paint over this fill, never padding beside it.
pub fn paint_row(ui: &Ui, rect: Rect, state: RowState) {
    crate::ui::components::row_shell(ui, rect, state, RowShell::Fill);
}
