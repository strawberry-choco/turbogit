use crate::theme::Palette;
use crate::ui::widgets;
use egui::{Color32, Ui};
use turbogit_app::banner::BannerSeverity;
use turbogit_app::state::AppState;

pub use turbogit_app::banner::{Banner as AppBanner, BannerAction, BannerSeverity as AppSeverity};

fn severity_color(s: BannerSeverity) -> Color32 {
    match s {
        BannerSeverity::Info => Palette::STATE_INFO,
        BannerSeverity::Warning => Palette::STATE_WARNING,
        BannerSeverity::Error => Palette::STATE_ERROR,
        BannerSeverity::Success => Palette::STATE_SUCCESS,
    }
}

pub fn maybe_show(ui: &mut Ui, state: &mut AppState) {
    if state.ui.banner.is_none() {
        return;
    }
    let clicked: Option<String> = {
        let banner = state.ui.banner.as_ref().expect("checked is_none above");
        let color = severity_color(banner.severity);
        ui.horizontal(|ui| {
            widgets::accent_bar(ui, color);
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
    if let Some(label) = clicked
        && let Some(b) = state.ui.banner.as_mut()
        && let Some(pos) = b.actions.iter().position(|a| a.label == label)
    {
        let a = b.actions.remove(pos);
        (a.on_click)(state);
    }
}
