//! Inline banner (issue #02): severity-tinted strip with deep-link
//! actions. The data lives on `AppState::ui::banner`; this module
//! renders it. The data types are owned by `turbogit_app::banner` so the
//! layer boundary stays clean (app does not depend on ui).

use crate::theme::Palette;
use egui::{Color32, Ui};
use turbogit_app::banner::{Banner, BannerSeverity};
use turbogit_app::state::AppState;

pub use turbogit_app::banner::{Banner as AppBanner, BannerAction, BannerSeverity as AppSeverity};

/// Map the app-owned severity to a theme color. Kept local to the UI
/// crate so the app layer never references theme tokens.
fn severity_color(s: BannerSeverity) -> Color32 {
    match s {
        BannerSeverity::Info => Palette::STATE_INFO,
        BannerSeverity::Warning => Palette::STATE_WARNING,
        BannerSeverity::Error => Palette::STATE_ERROR,
        BannerSeverity::Success => Palette::STATE_SUCCESS,
    }
}

/// Paint the banner set on `state.ui.banner`. No-op when `None`. The
/// function borrows `state` mutably for the duration of the call; the
/// painter is handed a short-lived borrow of the banner field through
/// the click-tracking pattern below.
pub fn maybe_show(ui: &mut Ui, state: &mut AppState) {
    if state.ui.banner.is_none() {
        return;
    }
    // Phase 1: paint (immutable borrow of banner via split scope).
    // Phase 2: dispatch the click (mutable borrow). The two phases
    // never overlap because the inner `ui.horizontal` scope ends before
    // we touch `state` again.
    let clicked: Option<String> = {
        let banner = state.ui.banner.as_ref().expect("checked is_none above");
        let color = severity_color(banner.severity);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(2), color);
            ui.colored_label(color, &banner.message);
            let mut clicked: Option<String> = None;
            for action in &banner.actions {
                if ui.small_button(&action.label).clicked() {
                    clicked = Some(action.label.clone());
                }
            }
            clicked
        })
        .inner
    };
    // Phase 2: drop the read-borrow, take the matching action.
    if let Some(label) = clicked
        && let Some(b) = state.ui.banner.as_mut()
        && let Some(pos) = b.actions.iter().position(|a| a.label == label)
    {
        let a = b.actions.remove(pos);
        (a.on_click)(state);
    }
}

/// Public entry point for callers that already hold `&Banner` (e.g. a
/// surface that wants to render a local copy). Same click-dispatch
/// contract via the state's banner.
pub fn show(ui: &mut Ui, state: &mut AppState, banner: &Banner) {
    let color = severity_color(banner.severity);
    let clicked: Option<String> = ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, 18.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(2), color);
            ui.colored_label(color, &banner.message);
            let mut clicked: Option<String> = None;
            for action in &banner.actions {
                if ui.small_button(&action.label).clicked() {
                    clicked = Some(action.label.clone());
                }
            }
            clicked
        })
        .inner;
    if let Some(label) = clicked
        && let Some(b) = state.ui.banner.as_mut()
        && let Some(pos) = b.actions.iter().position(|a| a.label == label)
    {
        let a = b.actions.remove(pos);
        (a.on_click)(state);
    }
}
