//! The commit window's writes.
//!
//! Three verbs, and what separates them is *what the developer asked for*:
//! the commit takes the index as it stands, while stage and un-stage each take
//! one whole file and hand it straight to the port. None of them re-derives
//! anything from the repository — the window computed the message, the amend
//! flag and the path, and the granular selection is already in the index, which
//! is why the commit no longer has to be told about it.

use std::path::Path;

use turbogit_domain::model::RootId;
use turbogit_services::changes;

use crate::operation::Operation;
use crate::root_caches::Affected;

/// Commit the index of `root` under `message`.
///
/// The service returns the new commit's id and it is **discarded**: what the
/// completion reports is the operation's own outcome, and substituting the id
/// would change what the toast says.
pub fn commit(root: &RootId, message: &str, amend: bool) -> Operation {
    let msg = message.to_string();
    let root = root.clone();
    Operation::custom("Commit", Affected::Root(root.clone()), move |v| {
        let _ = changes::commit_index(v, root.as_path(), &msg, amend)?;
        Ok(())
    })
}

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
