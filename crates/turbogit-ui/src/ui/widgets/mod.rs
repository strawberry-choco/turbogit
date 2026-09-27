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
    BADGE_TINT, BadgeKind, CHIP_GEOMETRY, CHIP_HEIGHT, CHIP_PAD_X, ChipColors, ChipGeometry,
    RefKind, badge, chip_radius, chip_rect_right, chip_text_origin, hash_chip, paint_chip,
};
pub use containers::{
    AVATAR_SIZE, CardFrame, CardSizing, CardSurface, alert_box, avatar_initials, card, card_header,
    cautions_rail, churn_bar, dialog_footer, group_title, note, recovery_note, toolwindow_header,
};
pub use controls::{
    ButtonVariant, WidgetState, action_button, compact_button, compact_button_enabled,
    disabled_child_scope, focus_ring, ghost_button, ghost_icon_button, icon_button, mix,
    primary_button, segmented_control, tint_over_bg,
};
pub use feedback::{KeyedReadPresentation, accent_bar, inline_error, keyed_read_presentation};
pub use inputs::{search_input, text_input};
pub use menu::{MenuItemKind, MenuItemProps, MenuItemState, menu_item, menu_rule, menu_surface};
// The host's two lifecycle entry points stay module-qualified
// (`menu_host::note_anchor`, `menu_host::host_menu`) — on this façade a bare
// `show` or `host_menu` would name no widget.
pub use menu_host::{MenuId, target_of};
pub use rows::paint_row;
pub use text::{SHORT_COMMIT_REF_CHARS, paint_centered_text, short_commit_ref};

// Kept crate-private for the existing log-window caller; this is not public API.
pub(crate) use containers::bold_font_if_available;
// The shared modal footer rule, crate-private so a sibling *surface* module (a
// modal that rules its body off from an action slot without owning a footer) can
// adopt the one treatment. It is deliberately NOT re-exported on the line above:
// the compatibility façade is a public API, and this is an internal seam.
pub(crate) use containers::footer_rule;
