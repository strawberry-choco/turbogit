//! Branch operations and favorites across roots.
//!
//! Thin orchestration layer over [`GitExecutor`] plus a helper that mutates the
//! in-memory [`MultiRootManager`]'s favorites.

use std::path::Path;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// Create a branch in one root.
pub fn create(
    vcs: &dyn GitExecutor,
    root: &Path,
    name: &str,
    start_point: Option<&str>,
    checkout: bool,
) -> TgResult<()> {
    vcs.branch_create(root, name, checkout, start_point)
}

/// Check out a branch in one root.
pub fn checkout(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<()> {
    vcs.branch_checkout(root, name)
}

/// Rename a branch in one root, clearing its upstream tracking only when asked.
///
/// The unset runs **after** the rename and names the **new** branch. Two orders
/// compile and one of them is wrong: `git branch --unset-upstream` with no
/// argument targets the current branch rather than this one, and the old name
/// stops resolving the moment `-m` moves the ref — so an unset placed before
/// the rename, or handed `old`, silently clears some other branch's tracking.
pub fn rename(
    vcs: &dyn GitExecutor,
    root: &Path,
    old: &str,
    new: &str,
    unset_upstream: bool,
) -> TgResult<()> {
    vcs.branch_rename(root, old, new)?;
    if unset_upstream {
        vcs.unset_branch_upstream(root, new)?;
    }
    Ok(())
}

/// Delete a local branch in one root.
pub fn delete(vcs: &dyn GitExecutor, root: &Path, name: &str, force: bool) -> TgResult<()> {
    vcs.branch_delete(root, name, force)
}

/// Toggle the `favorite` flag on a branch within the given root.
pub fn toggle_favorite(mgr: &mut MultiRootManager, root: &RootId, name: &str) {
    if let Some(r) = mgr.roots.iter_mut().find(|r| &r.id == root)
        && let Some(b) = r.branches.iter_mut().find(|b| b.name == name)
    {
        b.favorite = !b.favorite;
    }
}
