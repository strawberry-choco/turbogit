use turbogit_domain::model::RootId;

use crate::operation::Operation;
use crate::root_caches::Affected;

pub fn push(root: &RootId, msg: &str, keep_index: bool) -> Operation {
    let msg = msg.to_string();
    let root = root.clone();
    Operation::custom("Stash", Affected::Root(root.clone()), move |v| {
        v.stash_push(root.as_path(), &msg, keep_index)
    })
}

pub fn pop_latest(root: &RootId) -> Operation {
    let root = root.clone();
    Operation::custom("Stash pop", Affected::Root(root.clone()), move |v| {
        v.stash_pop(root.as_path(), 0)
    })
}

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
