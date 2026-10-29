use egui::Ui;
use turbogit_app::operation::PushRequest;
use turbogit_app::state::{AppState, Dialog};
use turbogit_app::sync_service::{self, DirectPushTarget};

use crate::ui::popups::Action;
use crate::ui::widgets;

/// The one-click target for the workspace as it stands, or `None` when only the
/// dialog can decide.
pub fn direct_target(state: &AppState) -> Option<DirectPushTarget> {
    sync_service::direct_push_target(
        &state.multi.roots,
        state.selected_root.as_ref(),
        &state.ui.repo_selection,
        &state.settings,
    )
}

pub fn popup_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        if direct_target(state).is_some()
            && widgets::compact_primary_button(ui, Action::PushSimple.label())
                .on_hover_text("Push the current branch without opening the dialog")
                .clicked()
        {
            push_without_dialog(state);
        }
        if ui
            .button(Action::Push.label())
            .on_hover_text("Preview and choose the target first")
            .clicked()
        {
            state.ui.dialog = Some(Dialog::Push);
        }
    });
}

pub fn push_without_dialog(state: &mut AppState) -> bool {
    let Some(target) = direct_target(state) else {
        state.ui.dialog = Some(Dialog::Push);
        return false;
    };
    let mut request =
        PushRequest::single(target.root, target.remote, target.branch, &state.settings);
    request.set_upstream = target.set_upstream;
    state.dispatch_push(request, None);
    true
}
