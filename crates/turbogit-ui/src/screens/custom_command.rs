//! Custom command: one command run verbatim across the selected roots. The
//! fleet preflight is the surface's dialog, and its own route is the page
//! that starts it, offers the recent commands, and reads the command back.
//! The destructive arm is the preflight's, not a second confirmation here.

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::bulk_ops::BulkOp;

use crate::bridge::AppHandle;
use crate::screens::bulk::preflight_dialog;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::input::{InputProps, input};

pub const TITLE: &str = "Custom command";

/// The dialog layer: the custom command's preflight while it is open.
pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.bulk_op == Some(BulkOp::Custom)),
        move || preflight_dialog(handle.clone(), BulkOp::Custom),
    )
}

/// The route's page: the command field, the recents, and the button that
/// opens the preflight with the typed command.
pub fn custom_command_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let generation = signal(0u64);
    let change = handle.clone();
    let open = handle.clone();
    let recents = handle.peek(|s| s.ui.recent_custom_commands.clone());
    let mut page = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .p(px(data::PANEL_PADDING))
        .bg(t.background)
        .test_id(&test_ids::route("custom_command"))
        .child(
            text(TITLE)
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            text("Runs verbatim on every selected repository; the leading “git” is optional.")
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground),
        )
        .child(input(
            InputProps::new()
                .placeholder("status --short")
                .accessibility_label("Command input")
                .remount_on(generation)
                .test_id(&test_ids::custom("input"))
                .on_change(move |text: &str| {
                    let text = text.to_string();
                    change.update(move |s| s.ui.bulk_command = text);
                }),
        ))
        .child(button(
            ButtonProps::new("Open custom command…")
                .variant(ButtonVariant::Primary)
                .test_id(&test_ids::custom("open"))
                .on_press(move || {
                    open.update(|s| {
                        let command = s.ui.bulk_command.trim().to_string();
                        s.open_bulk_preflight(BulkOp::Custom, &[]);
                        s.ui.bulk_command = command;
                    });
                }),
        ));
    if !recents.is_empty() {
        let mut row = view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text("Recent commands")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        for command in recents {
            let press = handle.clone();
            let label = command.clone();
            row = row.child(button(
                ButtonProps::new(command.clone())
                    .variant(ButtonVariant::Ghost)
                    .size(crate::ui::ControlSize::Compact)
                    .test_id(&test_ids::custom(format!("recent:{command}")))
                    .on_press(move || {
                        let label = label.clone();
                        press.update(move |s| {
                            s.open_bulk_preflight(BulkOp::Custom, &[]);
                            s.ui.bulk_command = label.clone();
                        });
                    }),
            ));
        }
        page = page.child(row);
    }
    page
}
