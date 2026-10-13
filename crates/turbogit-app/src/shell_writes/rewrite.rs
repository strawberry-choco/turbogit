//! The two writes the rewrite machinery offers after a history change: undo
//! the last rewrite from its backup ref, and abort an interactive rebase.
//!
//! Both act on the same backup ref, so both are guarded writes and neither is
//! reachable from a surface that has not already asked the app whether the
//! guard holds.

use turbogit_domain::model::RootId;
use turbogit_services::history_editor;

use crate::operation::Operation;
use crate::root_caches::Affected;
use crate::state::RewriteUndoVerb;

/// The undo operation's label. The success toast and the activity feed show it
/// as-is (`Undo drop`); a failure shows `{label}: {error}` — the undo-failed
/// outcome's wording, distinct from the other two by construction.
///
/// Re-exported by the banner module that paints the offer, which is where the
/// companion `success_title` and `cannot_undo` strings live.
pub fn undo_label(verb: RewriteUndoVerb) -> &'static str {
    match verb {
        RewriteUndoVerb::Drop => "Undo drop",
        RewriteUndoVerb::Reword => "Undo commit message edit",
    }
}

/// Restore `root`'s history from the rewrite backup, discarding the spent ref.
///
/// `verb` is a parameter rather than a constant because the label the banner
/// shows and the label the toast reports are the same one, and the offer knows
/// which rewrite it is undoing.
pub fn undo(root: &RootId, verb: RewriteUndoVerb, tip: &str) -> Operation {
    let tip = tip.to_string();
    let root = root.clone();
    Operation::custom(undo_label(verb), Affected::Root(root.clone()), move |v| {
        history_editor::undo_last_rewrite(v, root.as_path(), &tip)
    })
}

/// Restore `root` to the state its interactive rebase's pre-flight backup
/// recorded.
pub fn abort_interactive_rebase(root: &RootId) -> Operation {
    let root = root.clone();
    Operation::custom(
        "Abort interactive rebase",
        Affected::Root(root.clone()),
        move |v| history_editor::abort_to_backup(v, root.as_path()),
    )
}
