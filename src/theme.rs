//! Visual design system (Epic A).
//!
//! TurboGit previously applied *no* styling — it used raw `egui` defaults, which
//! is why it read as an unfinished prototype. This module defines coherent
//! Dark / Light / High-contrast `Visuals` presets (Darcula-like), a spacing &
//! typography scale, and helper to tint icons consistently with the active
//! theme. One call to [`configure_style`] styles the whole app.

use crate::model::ThemeMode;
use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2, Visuals};

/// Accent (selection / primary action) color for the active theme.
pub fn accent() -> Color32 {
    Color32::from_rgb(75, 110, 175)
}

/// Muted icon/foreground tint that reads well on the active theme.
pub fn icon_color() -> Color32 {
    Color32::from_gray(180)
}

fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.window_fill = Color32::from_rgb(43, 43, 43);
    v.panel_fill = Color32::from_rgb(49, 51, 53);
    v.extreme_bg_color = Color32::from_rgb(28, 28, 28);
    v.override_text_color = Some(Color32::from_rgb(200, 200, 200));
    v.faint_bg_color = Color32::from_rgb(56, 58, 60);
    v.code_bg_color = Color32::from_rgb(60, 60, 60);
    v.hyperlink_color = Color32::from_rgb(104, 151, 229);
    v.warn_fg_color = Color32::from_rgb(224, 75, 74);
    v.error_fg_color = Color32::from_rgb(226, 75, 74);
    v.selection.bg_fill = Color32::from_rgba_premultiplied(75, 110, 175, 90);
    v.selection.stroke = Stroke::new(1.0, accent());
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(49, 51, 53);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(190, 190, 190));
    v.widgets.inactive.bg_fill = Color32::from_rgb(60, 63, 65);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(200, 200, 200));
    v.widgets.hovered.bg_fill = Color32::from_rgb(72, 75, 78);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(225, 225, 225));
    v.widgets.active.bg_fill = Color32::from_rgb(82, 85, 88);
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(235, 235, 235));
    v.widgets.open.bg_fill = Color32::from_rgb(60, 63, 65);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v
}

fn light_visuals() -> Visuals {
    let mut v = Visuals::light();
    v.dark_mode = false;
    v.window_fill = Color32::from_rgb(255, 255, 255);
    v.panel_fill = Color32::from_rgb(246, 246, 246);
    v.extreme_bg_color = Color32::from_rgb(220, 220, 220);
    v.override_text_color = Some(Color32::from_rgb(30, 30, 30));
    v.faint_bg_color = Color32::from_rgb(236, 236, 236);
    v.code_bg_color = Color32::from_rgb(232, 232, 232);
    v.hyperlink_color = Color32::from_rgb(25, 95, 165);
    v.warn_fg_color = Color32::from_rgb(163, 60, 29);
    v.error_fg_color = Color32::from_rgb(163, 45, 45);
    v.selection.bg_fill = Color32::from_rgba_premultiplied(75, 110, 175, 70);
    v.selection.stroke = Stroke::new(1.0, accent());
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v
}

fn high_contrast_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.window_fill = Color32::from_rgb(0, 0, 0);
    v.panel_fill = Color32::from_rgb(0, 0, 0);
    v.extreme_bg_color = Color32::from_rgb(0, 0, 0);
    v.override_text_color = Some(Color32::from_rgb(255, 255, 255));
    v.faint_bg_color = Color32::from_rgb(20, 20, 20);
    v.code_bg_color = Color32::from_rgb(20, 20, 20);
    v.hyperlink_color = Color32::from_rgb(120, 200, 255);
    v.warn_fg_color = Color32::from_rgb(255, 200, 0);
    v.error_fg_color = Color32::from_rgb(255, 90, 90);
    v.selection.bg_fill = Color32::from_rgb(255, 255, 0);
    v.selection.stroke = Stroke::new(2.0, Color32::from_rgb(255, 255, 255));
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(0, 0, 0);
    v.widgets.noninteractive.fg_stroke = Stroke::new(2.0, Color32::from_rgb(255, 255, 255));
    v.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 30);
    v.widgets.inactive.fg_stroke = Stroke::new(2.0, Color32::from_rgb(255, 255, 255));
    v.widgets.hovered.bg_fill = Color32::from_rgb(60, 60, 60);
    v.widgets.hovered.fg_stroke = Stroke::new(2.0, Color32::from_rgb(255, 255, 255));
    v.widgets.active.bg_fill = Color32::from_rgb(90, 90, 90);
    v.widgets.active.fg_stroke = Stroke::new(2.0, Color32::from_rgb(255, 255, 255));
    v.widgets.open.bg_fill = Color32::from_rgb(30, 30, 30);
    v.window_stroke = Stroke::new(2.0, Color32::from_rgb(255, 255, 255));
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v
}

/// Apply the theme's `Visuals` plus a shared spacing / typography scale.
/// Called once per theme change (guarded by `last_applied_theme` in `app.rs`).
pub fn configure_style(ctx: &Context, mode: ThemeMode) {
    ctx.all_styles_mut(|style| {
        style.visuals = match mode {
            ThemeMode::Dark => dark_visuals(),
            ThemeMode::Light => light_visuals(),
            ThemeMode::HighContrast => high_contrast_visuals(),
        };

        // Spacing scale (Epic A4).
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(10);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
        style.spacing.indent = 14.0;

        // Typography scale (Epic A4): slightly larger body, mono for code.
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(14.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(14.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(12.0, FontFamily::Proportional));

        style.animation_time = 0.12;
    });
}
