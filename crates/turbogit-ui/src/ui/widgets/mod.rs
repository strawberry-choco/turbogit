//! Public facade for the shared widget vocabulary.
//!
//! The implementation is split by widget role, while every name that was
//! historically available from `turbogit_ui::ui::widgets` remains explicitly
//! re-exported here. Icon primitives remain owned by [`super::icons`].
//!
//! Behavioral rules implemented by these widgets include token-driven hover,
//! selection, disabled, and focus states. The plain group title is
//! [`group_title`]; the toggleable section band remains
//! [`super::components::section_header`]. General row fills are decided once by
//! [`super::components::row_fill`] and [`paint_row`].

mod chips;
mod containers;
mod controls;
mod feedback;
mod inputs;
mod rows;
mod text;

pub use crate::theme::CASCADE_ACCENT;
pub use chips::{
    BADGE_TINT, BadgeKind, CHIP_GEOMETRY, CHIP_HEIGHT, CHIP_PAD_X, ChipColors, ChipGeometry,
    CountDirection, RefKind, StatusBadge, badge, chip_radius, chip_rect_right, chip_text_origin,
    hash_chip, paint_chip, ref_label, status_badge,
};
pub use containers::{
    AVATAR_SIZE, alert_box, avatar_initials, card, card_header, churn_bar, dialog_footer,
    group_title, note, toolwindow_header,
};
pub use controls::{
    ButtonVariant, WidgetState, action_button, compact_button, compact_button_enabled, focus_ring,
    ghost_button, icon_button, mix, primary_button, segmented_control, tint_over_bg,
};
pub use feedback::{KeyedReadPresentation, inline_error, keyed_read_presentation};
pub use inputs::{search_input, text_input};
pub use rows::{paint_row, tree_row};
pub use text::{SHORT_COMMIT_REF_CHARS, short_commit_ref};

// Kept crate-private for the existing log-window caller; this is not public API.
pub(crate) use containers::bold_font_if_available;
