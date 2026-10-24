//! The push verbs: the one that pushes, and the one that opens the dialog.
//!
//! The reference has three push actions and ranks them by how much they disclose
//! — plain Push goes straight away, `Vcs.Push` opens the dialog, force push is a
//! dropdown item that is *absent* on a protected branch. TurboGit had the dialog
//! only, so the common case (one repository, one branch, nothing selected away
//! from it, nothing protected) paid four clicks to say what it means.
//!
//! The shell's frame is deliberately not where these live: `shell.rs`'s retired
//! menubar-and-toolbar pin (`workspace_shell_frame.rs`) forbids a button band of
//! the shell's own, so the VCS operations popup is the app's action surface and
//! the two verbs are its push row. The dialog keeps its dry-run preview, which is
//! stronger evidence than the reference's preview list — the fast path is a
//! shortcut, not a replacement.

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

/// Paint the popup's push row. The direct verb is *absent*, not disabled, when
/// the workspace needs the panel — the reference withholds force push on a
/// protected branch rather than warning about it, and the same enablement-by-
/// absence is what keeps a one-click verb from promising a push it cannot name.
pub fn popup_row(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        // Both labels come from the action list itself: the popup's pinned
        // action set is derived from it, so a restated string here would be a
        // second source of a label that has to match exactly.
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

/// Run the push the panel would have run, without the panel.
///
/// Returns whether a push was dispatched. The command palette reaches this
/// without the popup's enablement, so a caller that cannot honour the verb gets
/// the panel instead of a silent no-op — the fallback is the safe surface, not
/// nothing.
///
/// Same [`PushRequest`] shape and the same `dispatch_push` seam as the panel, so
/// a rejection that comes back from this route classifies and replays exactly
/// like one from the dialog, with the request stored for retry. Nothing here can
/// hold a commit subset: the direct verb pushes the branch, which is what its
/// single label promises.
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
