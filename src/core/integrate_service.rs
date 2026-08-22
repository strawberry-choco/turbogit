//! Integration service: high-level merge / rebase / cherry-pick orchestration
//! plus abort/continue and a "smart merge" that stashes a dirty tree first.

#![allow(dead_code)]

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::Path;

/// Merge `target` into the current branch at `root`.
pub fn merge(
    vcs: &dyn GitExecutor,
    root: &Path,
    target: &str,
    opts: &MergeOpts,
) -> TgResult<()> {
    vcs.merge(root, target, opts)
}

/// Rebase the current branch onto `onto`.
pub fn rebase(
    vcs: &dyn GitExecutor,
    root: &Path,
    onto: &str,
    opts: &RebaseOpts,
) -> TgResult<()> {
    vcs.rebase(root, onto, opts)
}

/// Apply `commit` on top of the current branch.
pub fn cherry_pick(vcs: &dyn GitExecutor, root: &Path, commit: &str) -> TgResult<()> {
    vcs.cherry_pick(root, commit)
}

/// Abort an in-progress `op` (merge / rebase / cherry-pick).
pub fn abort(vcs: &dyn GitExecutor, root: &Path, op: &str) -> TgResult<()> {
    vcs.abort(root, op)
}

/// Continue an in-progress `op` after resolving conflicts.
pub fn cont(vcs: &dyn GitExecutor, root: &Path, op: &str) -> TgResult<()> {
    vcs.continue_op(root, op)
}

/// Merge `target` while automatically stashing a dirty tree first.
///
/// If the working tree is dirty it is stashed (stash is used for both the
/// `Stash` and `Shelve` methods). The stash is popped afterwards; a pop failure
/// is ignored so the stash is left in place. A merge failure still triggers a
/// best-effort pop before returning the merge error.
pub fn smart_merge(
    vcs: &dyn GitExecutor,
    root: &Path,
    target: &str,
    opts: &MergeOpts,
    clean: CleanTreeMethod,
) -> TgResult<()> {
    let dirty = !vcs.status(root)?.changes.is_empty();
    if dirty {
        // Use stash for both methods; ignore a failing stash push.
        let _ = vcs.stash_push(root, "TurboGit smart merge", clean == CleanTreeMethod::Shelve);
    }

    match vcs.merge(root, target, opts) {
        Ok(()) => {
            if dirty {
                // Ignore pop errors: leave the stash in place for the user.
                let _ = vcs.stash_pop(root, 0);
            }
            Ok(())
        }
        Err(e) => {
            if dirty {
                let _ = vcs.stash_pop(root, 0);
            }
            Err(e)
        }
    }
}

/// Whether a merge / rebase / cherry-pick is currently in progress at `root`.
pub fn in_progress(root: &Path) -> bool {
    let git = Path::new(root).join(".git");
    git.join("MERGE_HEAD").exists()
        || git.join("rebase-merge").exists()
        || git.join("rebase-apply").exists()
        || git.join("CHERRY_PICK_HEAD").exists()
}
