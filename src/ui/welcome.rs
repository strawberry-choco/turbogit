//! Welcome page (issue #9 placeholder; real content lands in ticket #10).
//!
//! Shown instead of the active tool window whenever no project is open
//! (`AppState::show_welcome`, spec §9.2). This ticket only establishes the
//! visibility state model, so the page is a minimal branded placeholder —
//! the full welcome layout (action cards, clone box, recent projects) is
//! ticket #10's scope.

use crate::state::AppState;
use crate::theme::Palette;
use egui::{Align, FontFamily, FontId, Layout, RichText, Ui};

/// Minimal Welcome placeholder, centered in the central body.
pub fn placeholder(ui: &mut Ui, _state: &mut AppState) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.label(
            RichText::new("Welcome to TurboGit")
                .font(FontId::new(28.0, FontFamily::Proportional))
                .color(Palette::INK),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("A fast, keyboard-friendly Git client for your desktop.")
                .size(13.0)
                .color(Palette::INK_3),
        );
        ui.add_space(16.0);
        ui.label(
            RichText::new("Open a project to get started — File → Open Project…")
                .size(12.0)
                .color(Palette::INK_3),
        );
    });
}
