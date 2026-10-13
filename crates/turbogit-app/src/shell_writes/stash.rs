//! The stash writes, from the stash dialog and from the commit window's
//! stash list.
//!
//! Four entries rather than two: the dialog's `Pop latest` always names index
//! `0` and says `Stash pop`, while the commit window names the row's own
//! `stash_ref` and index and says `Pop stash {ref}`. The verbs differ in the
//! entry they touch and in what the toast says, so they stay apart — unlike
//! the conflict pair, which is one verb said twice.

use turbogit_domain::model::RootId;

use crate::operation::Operation;
use crate::root_caches::Affected;

/// `git stash push` the whole working tree under `msg`.
pub fn push(root: &RootId, msg: &str, keep_index: bool) -> Operation {
    let msg = msg.to_string();
    let root = root.clone();
    Operation::custom("Stash", Affected::Root(root.clone()), move |v| {
        v.stash_push(root.as_path(), &msg, keep_index)
    })
}

/// `git stash pop` the newest entry — the dialog's `Pop latest`.
pub fn pop_latest(root: &RootId) -> Operation {
    let root = root.clone();
    Operation::custom("Stash pop", Affected::Root(root.clone()), move |v| {
        v.stash_pop(root.as_path(), 0)
    })
}

/// `git stash pop` the entry at `index`, labelled with the row's `stash_ref`.
pub fn pop_at(root: &RootId, stash_ref: &str, index: usize) -> Operation {
    let work_root = root.clone();
    Operation::custom(
        format!("Pop stash {stash_ref}"),
        Affected::Root(root.clone()),
        move |v| v.stash_pop(work_root.as_path(), index),
    )
}

/// `git stash apply` the entry at `index`, leaving it in the list.
pub fn apply_at(root: &RootId, stash_ref: &str, index: usize) -> Operation {
    let work_root = root.clone();
    Operation::custom(
        format!("Apply stash {stash_ref}"),
        Affected::Root(root.clone()),
        move |v| v.stash_apply(work_root.as_path(), index),
    )
}
