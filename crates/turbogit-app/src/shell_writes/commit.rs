//! The commit window's writes.
//!
//! Stage and un-stage are **one entry for two directions**: each takes one
//! whole file and hands it straight to the port, and the label is spelled here
//! rather than at the click so the two toasts cannot drift. The window's other
//! verb is a named
//! [`Operation::Commit`](crate::operation::Operation::Commit), dispatched
//! through [`AppState::dispatch_commit`](crate::state::AppState::dispatch_commit)
//! — named `Operation` variants are
//! [`crate::shell_writes`](crate::shell_writes)'s stated exclusion, not an
//! entry here.

use std::path::Path;

use turbogit_domain::model::RootId;
use turbogit_services::changes;

use crate::operation::Operation;
use crate::root_caches::Affected;

/// Stage or un-stage one whole file in `root`.
///
/// One entry for both directions because the two are the same decision read
/// off the same box, and the label is spelled here rather than at the click so
/// the toast a stage writes and the toast an un-stage writes cannot drift.
pub fn set_staged(root: &RootId, path: &Path, stage: bool) -> Operation {
    let name = path.display();
    let label = if stage {
        format!("Stage {name}")
    } else {
        format!("Unstage {name}")
    };
    let p = path.to_path_buf();
    let root = root.clone();
    Operation::custom(label, Affected::Root(root.clone()), move |v| {
        let paths = [p];
        if stage {
            changes::stage_paths(v, root.as_path(), &paths)
        } else {
            changes::unstage_paths(v, root.as_path(), &paths)
        }
    })
}
