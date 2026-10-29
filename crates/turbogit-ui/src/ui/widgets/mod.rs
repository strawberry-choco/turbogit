mod chips;
mod containers;
mod controls;
mod feedback;
mod inputs;
mod menu;
pub mod menu_host;
mod rows;
mod tabs;
mod text;

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
    pane_header, pane_header_inset, recovery_note,
};
pub use controls::{
    ButtonVariant, ICON_BUTTON_SIZE, WidgetState, action_button, button_enabled, compact_button,
    compact_button_enabled, compact_primary_button, disabled_child_scope, focus_ring, ghost_button,
    ghost_icon_button, icon_button, icon_button_enabled, mix, primary_button, segmented_control,
    tint_over_bg,
};
pub use feedback::{KeyedReadPresentation, accent_bar, inline_error, keyed_read_presentation};
pub use inputs::{filter_matches, search_input, text_input};
pub use menu::{
    MenuItemKind, MenuItemProps, MenuItemState, menu_item, menu_rule, menu_surface,
    take_status_hint,
};
pub use menu_host::{MenuId, target_of};
pub use rows::paint_row;
pub use tabs::{TAB_ITEM_HEIGHT, TAB_STRIP_HEIGHT, TabItem, tab_view};
pub use text::{SHORT_COMMIT_REF_CHARS, empty_state, paint_centered_text, short_commit_ref};

pub(crate) use containers::bold_font_if_available;
pub(crate) use containers::footer_rule;
