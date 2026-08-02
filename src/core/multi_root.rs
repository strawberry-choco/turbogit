//! Helpers that operate on the [`MultiRootManager`] model: discovery, building
//! `Root` snapshots, registration, and aggregated status rollups.

use crate::core::vcs_manager::VcsManager;
use crate::error::TgResult;
use crate::model::*;
use std::path::{Path, PathBuf};

/// Discover candidate repo roots beneath `project_dir`.
pub fn discover_roots(vcs: &VcsManager, project_dir: &Path) -> Vec<PathBuf> {
    vcs.scan_for_roots(project_dir)
}

/// Build a single [`Root`] snapshot for `path`.
pub fn build_root(vcs: &VcsManager, path: &Path) -> TgResult<Root> {
    vcs.root_snapshot(path)
}

/// Register `root` into `mgr` unless a root with the same id already exists.
pub fn register(mgr: &mut MultiRootManager, root: Root) {
    if !mgr.roots.iter().any(|r| r.id == root.id) {
        mgr.roots.push(root);
    }
}

/// Build & register a root for each `paths` entry. Returns one result per path.
pub fn register_all(
    vcs: &VcsManager,
    mgr: &mut MultiRootManager,
    paths: &[PathBuf],
) -> Vec<TgResult<()>> {
    paths
        .iter()
        .map(|p| {
            let root = build_root(vcs, p)?;
            register(mgr, root);
            Ok(())
        })
        .collect()
}

/// Aggregate per-root change counts: `(id, modified_count, unversioned_count)`.
pub fn roots_status(mgr: &MultiRootManager) -> Vec<(RootId, usize, usize)> {
    mgr.roots
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                r.status.modified(),
                r.status.unversioned(),
            )
        })
        .collect()
}
