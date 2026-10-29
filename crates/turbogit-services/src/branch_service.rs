use std::path::Path;
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

pub fn create(
    vcs: &dyn GitExecutor,
    root: &Path,
    name: &str,
    start_point: Option<&str>,
    checkout: bool,
) -> TgResult<()> {
    vcs.branch_create(root, name, checkout, start_point)
}

pub fn checkout(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<()> {
    vcs.branch_checkout(root, name)
}

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

pub fn delete(vcs: &dyn GitExecutor, root: &Path, name: &str, force: bool) -> TgResult<()> {
    vcs.branch_delete(root, name, force)
}

pub fn toggle_favorite(mgr: &mut MultiRootManager, root: &RootId, name: &str) {
    if let Some(r) = mgr.roots.iter_mut().find(|r| &r.id == root)
        && let Some(b) = r.branches.iter_mut().find(|b| b.name == name)
    {
        b.favorite = !b.favorite;
    }
}
