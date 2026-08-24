//! Branch operations, favorites, and synchronous branch control across roots.
//!
//! Thin orchestration layer over [`GitExecutor`] plus helpers that mutate the
//! in-memory [`MultiRootManager`] (favorites / protected flags) and compute
//! multi-root aggregates (common branches, synchronous create/checkout).

#![allow(dead_code)]

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::Path;

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

/// Rename a branch in one root.
pub fn rename(vcs: &dyn GitExecutor, root: &Path, old: &str, new: &str) -> TgResult<()> {
    vcs.branch_rename(root, old, new)
}

/// Delete a local branch in one root.
pub fn delete(vcs: &dyn GitExecutor, root: &Path, name: &str, force: bool) -> TgResult<()> {
    vcs.branch_delete(root, name, force)
}

/// Delete a remote-tracking branch in one root.
pub fn delete_remote(vcs: &dyn GitExecutor, root: &Path, remote: &str, name: &str) -> TgResult<()> {
    vcs.branch_delete_remote(root, remote, name)
}

/// Toggle the `favorite` flag on a branch within the given root.
pub fn toggle_favorite(mgr: &mut MultiRootManager, root: &RootId, name: &str) {
    if let Some(r) = mgr.roots.iter_mut().find(|r| &r.id == root)
        && let Some(b) = r.branches.iter_mut().find(|b| b.name == name)
    {
        b.favorite = !b.favorite;
    }
}

/// Set the `protected` flag on a branch within the given root.
pub fn set_protected(mgr: &mut MultiRootManager, root: &RootId, name: &str, protected: bool) {
    if let Some(r) = mgr.roots.iter_mut().find(|r| &r.id == root)
        && let Some(b) = r.branches.iter_mut().find(|b| b.name == name)
    {
        b.protected = protected;
    }
}

/// Diff the working tree against `name` (branch vs working tree).
pub fn compare(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<String> {
    vcs.diff(
        root,
        &DiffOpts {
            left: Some(name.to_string()),
            ..Default::default()
        },
    )
}

/// Diff the working tree against `name` (alias of [`compare`]).
pub fn compare_working(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<String> {
    vcs.diff(
        root,
        &DiffOpts {
            left: Some(name.to_string()),
            ..Default::default()
        },
    )
}

/// Local branch names present in EVERY root.
pub fn common_branches(mgr: &MultiRootManager) -> Vec<String> {
    let mut iter = mgr.roots.iter();
    let mut common: Vec<String> = match iter.next() {
        Some(first) => first
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local)
            .map(|b| b.name.clone())
            .collect(),
        None => return Vec::new(),
    };
    for root in iter {
        let local: Vec<String> = root
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local)
            .map(|b| b.name.clone())
            .collect();
        common.retain(|n| local.contains(n));
    }
    common
}

/// Create `name` (with checkout) in every root and refresh each root's branches.
pub fn create_all(
    vcs: &dyn GitExecutor,
    mgr: &mut MultiRootManager,
    name: &str,
    start_point: Option<&str>,
) -> Vec<TgResult<()>> {
    let ids: Vec<RootId> = mgr.roots.iter().map(|r| r.id.clone()).collect();
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        let path = id.as_path().to_path_buf();
        let res = match vcs.branch_create(&path, name, true, start_point) {
            Ok(()) => {
                if let Ok(branches) = vcs.branches(&path)
                    && let Some(r) = mgr.roots.iter_mut().find(|r| r.id == id)
                {
                    r.branches = branches;
                }
                Ok(())
            }
            Err(e) => Err(e),
        };
        results.push(res);
    }
    results
}

/// Check out `name` in every root and refresh each root's branches.
pub fn checkout_all(
    vcs: &dyn GitExecutor,
    mgr: &mut MultiRootManager,
    name: &str,
) -> Vec<TgResult<()>> {
    let ids: Vec<RootId> = mgr.roots.iter().map(|r| r.id.clone()).collect();
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        let path = id.as_path().to_path_buf();
        let res = match vcs.branch_checkout(&path, name) {
            Ok(()) => {
                if let Ok(branches) = vcs.branches(&path)
                    && let Some(r) = mgr.roots.iter_mut().find(|r| r.id == id)
                {
                    r.branches = branches;
                }
                Ok(())
            }
            Err(e) => Err(e),
        };
        results.push(res);
    }
    results
}
