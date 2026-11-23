//! The styled controls every screen imports. Each one takes variant, size,
//! state props, and slots, and paints only from the token layers: chrome
//! roles from `nsh_base::theme()`, data roles from [`crate::theme`]. No
//! literal color or radius appears here.
//!
//! nsh-base's component elements carry no `test_id` in 0.5.x, so the
//! wrappers that hold one mount it inside a view the selector names.

pub mod alert;
pub mod button;
pub mod callback;
pub mod card;
pub mod checkbox;
pub mod chip;
pub mod combobox;
pub mod command;
pub mod data_table;
pub mod dialog;
pub mod icon_button;
pub mod image;
pub mod input;
pub mod menu;
pub mod pane_header;
pub mod popover;
pub mod progress;
pub mod row;
pub mod scrollbar;
pub mod select;
pub mod switch;
pub mod tab;
pub mod toast;
pub mod toolbar;
pub mod tooltip;
pub mod tree;

pub use alert::alert_dialog;
pub use button::{ButtonProps, ButtonVariant, button};
pub use callback::OnChange;
pub use card::{CardProps, card};
pub use checkbox::{CheckboxProps, CheckboxState, checkbox};
pub use chip::{ChipProps, ChipVariant, chip};
pub use combobox::{ComboboxProps, combobox};
pub use command::{CommandProps, command};
pub use data_table::{DataTableColumnProps, DataTableProps, data_table};
pub use dialog::{DialogProps, dialog};
pub use icon_button::{IconButtonProps, icon_button};
pub use input::{InputProps, input, text_area};
pub use menu::{MenuProps, context_menu_host, menu};
pub use pane_header::{PaneHeaderProps, pane_header};
pub use popover::{PopoverProps, popover};
pub use progress::{ProgressProps, progress};
pub use row::{RowProps, RowState, row};
pub use scrollbar::{ScrollbarProps, scrollbar};
pub use select::{SelectProps, select};
pub use switch::{SwitchProps, switch};
pub use tab::{TabItem, TabsProps, tabs};
pub use toast::{ToastShared, toast_layer};
pub use toolbar::{ToolbarProps, toolbar};
pub use tooltip::{TooltipProps, tooltip};
pub use tree::{TreeEntryProps, TreeProps, tree};

/// A control's size step. The heights follow the density constants; the
/// type size follows the scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlSize {
    /// The dense step: toolbars, table rows, dialog footers.
    Compact,
    /// The default: shell buttons, inputs.
    #[default]
    Regular,
    /// Dialog titles' action row, welcome's primary calls.
    Large,
}

/// Text and padding for a size step, shared by the controls that have one.
pub(crate) fn size_pad_x(size: ControlSize) -> f32 {
    match size {
        ControlSize::Compact => crate::theme::DENSITY_COMPACT_BUTTON_PAD_X,
        ControlSize::Regular => crate::theme::BUTTON_PAD_X,
        ControlSize::Large => crate::theme::BUTTON_PAD_X + 4.0,
    }
}

pub(crate) fn size_pad_y(size: ControlSize) -> f32 {
    match size {
        ControlSize::Compact => crate::theme::DENSITY_COMPACT_BUTTON_PAD_Y,
        ControlSize::Regular => crate::theme::BUTTON_PAD_Y,
        ControlSize::Large => crate::theme::BUTTON_PAD_Y + 3.0,
    }
}

pub(crate) fn size_type(size: ControlSize) -> f32 {
    match size {
        ControlSize::Compact => crate::theme::TYPE_CONTROL,
        ControlSize::Regular => crate::theme::TYPE_BODY,
        ControlSize::Large => crate::theme::TYPE_DETAIL_TITLE,
    }
}
