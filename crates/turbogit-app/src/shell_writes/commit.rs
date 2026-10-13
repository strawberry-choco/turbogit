//! The commit window's one write.
//!
//! The commit is a selection, not a whole tree: the untouched files, the
//! granular (partially staged) files and the amend flag are three arguments the
//! window computes, and the entry passes them through rather than re-deriving
//! them — the granular selection cannot be rebuilt from the repository after
//! the fact.

use turbogit_domain::model::{Change, RootId};
use turbogit_services::changes;

use crate::operation::Operation;
use crate::root_caches::Affected;

/// Commit `untouched` plus `partial` in `root` under `message`.
///
/// The service returns the new commit's id and it is **discarded**: what the
/// completion reports is the operation's own outcome, and substituting the id
/// would change what the toast says.
pub fn commit(
    root: &RootId,
    message: &str,
    untouched: &[Change],
    partial: &[Change],
    amend: bool,
) -> Operation {
    let msg = message.to_string();
    let untouched = untouched.to_vec();
    let partial = partial.to_vec();
    let root = root.clone();
    Operation::custom("Commit", Affected::Root(root.clone()), move |v| {
        let _ = changes::commit_selected(v, root.as_path(), &msg, &untouched, &partial, amend)?;
        Ok(())
    })
}
