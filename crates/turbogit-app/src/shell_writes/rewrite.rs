use turbogit_domain::model::RootId;
use turbogit_services::history_editor;

use crate::operation::Operation;
use crate::root_caches::Affected;
use crate::state::RewriteUndoVerb;

pub fn undo_label(verb: RewriteUndoVerb) -> &'static str {
    match verb {
        RewriteUndoVerb::Drop => "Undo drop",
        RewriteUndoVerb::Reword => "Undo commit message edit",
    }
}

/// Restore `root`'s history from the rewrite backup, discarding the spent ref.
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
