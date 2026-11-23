use egui::{Color32, Pos2, Sense, Ui, Vec2};

use crate::theme::{GROUP_ROW_HEIGHT, Palette, TYPE_CHIP, TYPE_CONTROL, chrome_font, data_font};
use turbogit_app::state::AppState;

const ROWS: &[(&str, &str)] = &[
    ("Open the Commit tool window", "Ctrl+K"),
    ("Open the Push dialog", "Ctrl+Shift+K"),
    ("Refresh", "Ctrl+T"),
    ("Open the command palette", "Ctrl+Shift+A"),
    ("Toggle the VCS operations popup", "Alt+`"),
    ("Focus the Commit file filter", "/"),
    ("Next / previous hunk", "F7 / Shift+F7"),
    ("Toggle the diff options menu", "Ctrl+Shift+D"),
    ("Focus the opposite diff pane", "Ctrl+Shift+Tab"),
    ("Jump to the source line", "F4"),
    ("Stage / unstage the previewed file", "Space"),
];

pub fn show(ui: &mut Ui, state: &mut AppState) {
    if !state.ui.shortcuts_open {
        return;
    }
    let mut keep_open = true;
    egui::Window::new("Shortcuts")
        .open(&mut keep_open)
        .default_width(340.0)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            for (action, keys) in ROWS {
                row(ui, action, keys);
            }
        });
    if !keep_open {
        state.ui.shortcuts_open = false;
    }
}

fn row(ui: &mut Ui, action: &str, keys: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, GROUP_ROW_HEIGHT), Sense::hover());
    let keys_galley =
        ui.painter()
            .layout_no_wrap(keys.to_owned(), data_font(TYPE_CHIP), Palette::T_MUTED);
    ui.painter().galley(
        Pos2::new(
            rect.right() - 8.0 - keys_galley.size().x,
            rect.center().y - keys_galley.size().y / 2.0,
        ),
        keys_galley,
        Color32::WHITE,
    );
    let label =
        ui.painter()
            .layout_no_wrap(action.to_owned(), chrome_font(TYPE_CONTROL), Palette::INK);
    ui.painter().galley(
        Pos2::new(rect.left() + 8.0, rect.center().y - label.size().y / 2.0),
        label,
        Color32::WHITE,
    );
}
