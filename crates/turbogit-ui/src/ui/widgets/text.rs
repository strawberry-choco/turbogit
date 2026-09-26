//! General-purpose text utilities used by commit presentations.

use egui::{Color32, FontId, Pos2, Rect};

/// Default number of Unicode characters retained from a commit reference.
pub const SHORT_COMMIT_REF_CHARS: usize = 7;

/// Format a commit reference for compact display without splitting a Unicode
/// scalar value. References at or below the default length pass through intact.
pub fn short_commit_ref(reference: &str) -> String {
    reference.chars().take(SHORT_COMMIT_REF_CHARS).collect()
}

/// Paint `text` centred inside `rect` on **both** axes.
///
/// The calculation is the two-axis centring contract every centred painter
/// shares: lay the string out once, then place the galley's top-left corner
/// half a galley away from the rect's centre, so `rect.center() -
/// galley.size() / 2` is the paint origin. One axis is horizontal, one is
/// vertical, and the helper is the single definition of that pair — a site that
/// recomputes one of the two terms by hand has a real bug, not a style choice.
///
/// This is the text path. The icon path stays with
/// [`crate::ui::icons::centered_icon`], which centres an icon rectangle rather
/// than a laid-out galley.
///
/// Deliberately **not** applied to:
///
/// * [`super::containers::avatar_initials`] — the container is a `circle_filled`
///   of a radius, not a chip rect, so there is no rect to centre a galley in.
/// * [`super::chips::chip_text_origin`] and `ChipGeometry::text_origin` — the
///   geometry-level origin accessor. This helper paints text; those are the
///   destination for a caller that already holds a galley and a chip rect.
/// * [`super::chips::hash_chip`] — interactive and monospace, and it owns its
///   own `CHIP_HEIGHT + 6.0` height rather than the shared geometry, so folding
///   its paint in here would change the chip it draws.
/// * `crate::ui::log_window`'s `paint_label_pill` — an icon-only pill that
///   carries no galley at all (see that function's note).
///
/// The separate family of **vertical-only** centring sites — a galley whose x
/// comes from a layout column, a right-aligned edge, or an icon-plus-label
/// group, and which therefore only offsets y — is a different calculation and
/// is intentionally not swept in here. Unifying it is its own change.
pub fn paint_centered_text(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    painter.galley(
        Pos2::new(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
}
