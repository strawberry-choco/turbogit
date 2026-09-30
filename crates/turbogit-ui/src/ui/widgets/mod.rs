//! Public facade for the shared widget vocabulary.
//!
//! The implementation is split by widget role, while every name that was
//! historically available from `turbogit_ui::ui::widgets` remains explicitly
//! re-exported here. Icon primitives remain owned by [`super::icons`].
//!
//! Four names were retired from that compatibility surface by the dead-API
//! sweep (the ref-label render function, the status-badge family, the badge
//! family's direction enum, and the fixed-height tree-row wrapper): they had
//! no production caller and survived only on their own characterization tests.
//! `RefKind` and `RefKind::accent` deliberately **stay** — a live log surface
//! maps its own reference kind onto them for colouring without ever rendering a
//! ref label. See `docs/design-system-roles.md`.
//!
//! A fifth name was retired by the pane-chrome ticket: **the tool-window
//! header.** It is gone from this list, and it is gone from `containers` with
//! it, because R7 has exactly one pane header — [`pane_header`] — and the
//! conflict grammar's module-private header was absorbed into that same
//! function rather than left as a second one. Two implementations of "the top
//! strip of a pane" is not two spellings of one decision; it is two headers the
//! user can tell apart. `tests/widget_library.rs` pins the retirement at the
//! declaration and, more usefully, at the paint: four panes rendering the same
//! header geometry.
//!
//! The column chrome sits beside it: [`PaneColumn`] is one column's label and
//! offset, [`column_header`] is the one row that labels them, and
//! [`column_cell`] is how a flow row reaches the same offset its header was
//! painted from.
//!
//! The chip vocabulary itself is **closed at three**: [`ref_chip`],
//! [`current_chip`] and [`count_chip`], one function each. [`badge`] keeps the
//! pill radius and its own slot beside them, and `hash_chip` is a label rather
//! than a control; neither is a fourth chip, and the two chip radii stay
//! distinct.
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
mod menu;
pub mod menu_host;
mod rows;
mod text;

// A frozen compatibility import path: the token's real home is `crate::theme`,
// but `turbogit_ui::ui::widgets::CASCADE_ACCENT` is established public surface
// and must keep resolving whether or not any module here reads it. Do not
// "tidy" this away as an unused re-export.
pub use crate::theme::CASCADE_ACCENT;
pub use chips::{
    BADGE_TINT, BadgeKind, CHIP_GEOMETRY, CHIP_HEIGHT, CHIP_PAD_X, COMPACT_CHIP_GEOMETRY,
    COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, ChipColors, ChipGeometry, REF_CHIP_COLORS, RefKind,
    badge, chip_radius, chip_rect_right, chip_text_origin, compact_chip_radius, count_chip,
    current_chip, hash_chip, paint_chip, ref_chip,
};
pub use containers::{
    AVATAR_SIZE, COLUMN_HEADER_HEIGHT, CardFrame, CardSizing, CardSurface, ColumnAlign, Edge,
    PANE_HEADER_HEIGHT, PaneColumn, alert_box, avatar_initials, card, card_header, cautions_rail,
    churn_bar, column_cell, column_header, dialog_footer, edge_rule, group_title, note,
    pane_header, recovery_note,
};
pub use controls::{
    ButtonVariant, WidgetState, action_button, button_enabled, compact_button,
    compact_button_enabled, compact_primary_button, disabled_child_scope, focus_ring, ghost_button,
    ghost_icon_button, icon_button, icon_button_enabled, mix, primary_button, segmented_control,
    tint_over_bg,
};
pub use feedback::{KeyedReadPresentation, accent_bar, inline_error, keyed_read_presentation};
pub use inputs::{filter_matches, search_input, text_input};
pub use menu::{MenuItemKind, MenuItemProps, MenuItemState, menu_item, menu_rule, menu_surface};
// The host's two lifecycle entry points stay module-qualified
// (`menu_host::note_anchor`, `menu_host::host_menu`) — on this façade a bare
// `show` or `host_menu` would name no widget.
pub use menu_host::{MenuId, target_of};
pub use rows::paint_row;
pub use text::{SHORT_COMMIT_REF_CHARS, empty_state, paint_centered_text, short_commit_ref};

// Kept crate-private for the existing log-window caller; this is not public API.
pub(crate) use containers::bold_font_if_available;
// The shared modal footer rule, crate-private so a sibling *surface* module (a
// modal that rules its body off from an action slot without owning a footer) can
// adopt the one treatment. It is deliberately NOT re-exported on the line above:
// the compatibility façade is a public API, and this is an internal seam.
pub(crate) use containers::footer_rule;
