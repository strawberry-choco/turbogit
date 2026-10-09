//! Post-hoc undo for the targeted history verbs: the notification
//! a landed drop or reword leaves behind, carrying an `Undo` control.
//!
//! The reference's undo lives in the success notification; whether TurboGit's
//! should be a toast or a separate banner is `UNVERIFIED` in the evidence, so
//! this follows the one undo banner the app already has — the branch delete's
//! short-window strip — painted over the log the rewrite happened in.
//!
//! Coverage is deliberately asymmetric, matching the reference's own
//! lifecycle: drop and reword are undoable by name, squash is not. This
//! module is the post-hoc half only; the pre-flight half is the shared
//! `Dialog::RewritePreflight`, and the gating state machine is its own module.
//!
//! The guard is the substance: validity is checked at undo time, never
//! assumed at rewrite time. [`AppState::guard_rewrite_undo`] re-runs it on
//! every completed operation in the root, and a moved repository withholds
//! the offer — the reason is painted, and NO control is. An undo that is
//! offered and then fails is worse than one withheld.
//!
//! Nothing here runs git. The click dispatches through the port
//! ([`history_editor::undo_last_rewrite`], which re-verifies the guard inside
//! the operation — the race window between paint and click closes there).

use egui::{RichText, Ui};
use turbogit_app::operation::Operation;
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, RewriteUndo, RewriteUndoVerb};
use turbogit_services::history_editor;

use crate::theme::{Palette, TYPE_CONTROL, chrome_font};
use crate::ui::components::{KitButton, kit_button};
use crate::ui::widgets;

/// How long the offer lives, matching the branch delete undo's window: after
/// it, the strip disappears on its own. The backup ref outlives the strip —
/// the expiry is the notification's, not the recovery's.
const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// The success title the undo's notification opens with, in the reference's
/// own past tense (`Dropped {N} {commits}` / `Commit message changed`): the
/// drop names the commit it removed, the reword names the fact, not the text.
pub fn success_title(verb: RewriteUndoVerb, commit: &str) -> String {
    match verb {
        RewriteUndoVerb::Drop => {
            format!("Dropped commit {}", widgets::short_commit_ref(commit))
        }
        RewriteUndoVerb::Reword => "Commit message changed".to_string(),
    }
}

/// The undo-impossible reason: validity was checked at undo time and the
/// repository had moved — the reference's `Cannot undo commit message edit` +
/// `Repository has already been changed` pair, folded into one sentence.
pub fn cannot_undo(verb: RewriteUndoVerb) -> &'static str {
    match verb {
        RewriteUndoVerb::Drop => "Cannot undo drop — the repository has changed since",
        RewriteUndoVerb::Reword => {
            "Cannot undo commit message edit — the repository has changed since"
        }
    }
}

/// The undo operation's label. The success toast and the activity feed show it
/// as-is (`Undo drop`); a failure shows `{label}: {error}` — the undo-failed
/// outcome's wording, distinct from the other two by construction.
pub fn undo_label(verb: RewriteUndoVerb) -> &'static str {
    match verb {
        RewriteUndoVerb::Drop => "Undo drop",
        RewriteUndoVerb::Reword => "Undo commit message edit",
    }
}

/// Paint the banner over the log's graph pane while an offer stands: the
/// success title and the `Undo` control while the guard holds it valid, the
/// impossible reason and nothing else once it does not.
pub fn banner(ui: &mut Ui, state: &mut AppState) {
    let Some(offer) = state.ui.rewrite_undo.clone() else {
        return;
    };
    if offer.created_at.elapsed() > UNDO_WINDOW {
        state.ui.rewrite_undo = None;
        return;
    }
    ui.horizontal(|ui| {
        if offer.withheld {
            ui.label(
                RichText::new(cannot_undo(offer.verb))
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::T_SECONDARY),
            );
        } else {
            ui.label(
                RichText::new(success_title(offer.verb, &offer.commit))
                    .font(chrome_font(TYPE_CONTROL))
                    .color(Palette::INK),
            );
            if kit_button(ui, KitButton::Primary, "Undo").clicked() {
                dispatch_undo(state, &offer);
            }
        }
    });
    ui.add_space(4.0);
}

/// Dispatch the undo and consume the offer in the same breath: the operation
/// re-verifies the guard inside the work (the paint-time check cannot close
/// the race an external `git` can open), restores from the backup ref, and
/// discards the spent ref. A failure surfaces through the ordinary
/// completion path as `{label}: {error}` — the undo-failed outcome.
fn dispatch_undo(state: &mut AppState, offer: &RewriteUndo) {
    let root = offer.root.clone();
    let tip = offer.tip_sha.clone();
    state.ui.rewrite_undo = None;
    state.dispatch(Operation::custom(
        undo_label(offer.verb),
        Affected::Root(root.clone()),
        move |v| history_editor::undo_last_rewrite(v, root.as_path(), &tip),
    ));
}
