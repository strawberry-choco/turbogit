//! The shortcuts window: the same key table the shell binds, painted as a
//! data table. A listed key is a bound key; the lane and the unit suite press
//! three of them and read the state each one moved.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::Tab;

use crate::bridge::AppHandle;
use crate::screens::shell::TabSwitcher;
use crate::test_ids;
use crate::theme as data;
use crate::ui::data_table::{DataTableColumnProps, DataTableProps, data_table};
use crate::ui::dialog::{DialogProps, dialog};

/// One listed shortcut. `keystroke` is the keymap spelling the shell binds;
/// `display` is what the table shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub action: &'static str,
    pub display: &'static str,
    pub keystroke: &'static str,
}

/// Every global shortcut the shell binds, in table order.
pub const KEYMAP: [Binding; 4] = [
    Binding {
        action: "Open the Commit tool window",
        display: "Ctrl+K",
        keystroke: "ctrl+k",
    },
    Binding {
        action: "Open the Push dialog",
        display: "Ctrl+Shift+K",
        keystroke: "ctrl+shift+k",
    },
    Binding {
        action: "Refresh",
        display: "Ctrl+T",
        keystroke: "ctrl+t",
    },
    Binding {
        action: "Open the command palette",
        display: "Ctrl+Shift+A",
        keystroke: "ctrl+shift+a",
    },
];

/// Binds every row of [`KEYMAP`] to its state verb, once per mounted shell.
pub fn bind(switcher: &TabSwitcher, handle: &AppHandle) {
    let commit = switcher.clone();
    on_key("ctrl+k", move |_| commit.select(Tab::Commit));

    let push = handle.clone();
    on_key("ctrl+shift+k", move |_| {
        push.update(|s| s.open_push());
    });

    let refresh = handle.clone();
    on_key("ctrl+t", move |_| {
        refresh.update(|s| s.refresh(Affected::All));
    });

    let palette = handle.clone();
    on_key("ctrl+shift+a", move |_| {
        palette.update(|s| {
            s.ui.command_query.clear();
            s.ui.command_palette = true;
        });
    });
}

/// The dialog layer: the shortcuts table while `ui.shortcuts_open`.
pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.shortcuts_open),
        move || {
            let close = handle.clone();
            dialog(
                DialogProps::new("shortcuts", "Shortcuts")
                    .width(420.0)
                    .cancel_label("Close")
                    .hide_action(true)
                    .on_cancel(move || close.update(|s| s.ui.shortcuts_open = false))
                    .child(virtual_key_table()),
            )
        },
    )
}

/// The route's screen: the same table inline.
pub fn shortcuts_screen(_handle: AppHandle) -> impl IntoElement {
    let t = theme();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .p(px(data::PANEL_PADDING))
        .bg(t.background)
        .test_id(&test_ids::route("shortcuts"))
        .child(
            text("Shortcuts")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(virtual_key_table())
}

fn virtual_key_table() -> impl IntoElement {
    let rows = KEYMAP.len();
    let actions: Vec<String> = KEYMAP.iter().map(|b| b.action.to_string()).collect();
    let keys: Vec<String> = KEYMAP.iter().map(|b| b.display.to_string()).collect();
    let data = nsh_base::DataTableRows::new(rows)
        .texts("action", actions)
        .texts("keys", keys);
    view()
        .h(px(data::GROUP_ROW_HEIGHT * (rows as f32 + 1.0)))
        .w_full()
        .child(data_table(
            DataTableProps::new(
                vec![
                    DataTableColumnProps::new("action", "ACTION").width(260.0),
                    DataTableColumnProps::new("keys", "KEYS").width(140.0),
                ],
                data,
            )
            .row_height(data::GROUP_ROW_HEIGHT)
            .accessibility_label("Shortcuts")
            .test_id(&test_ids::shortcuts("table")),
        ))
}
