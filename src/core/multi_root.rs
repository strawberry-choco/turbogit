//! Multi-root project model: root discovery (the Root scanner), `Root`
//! snapshot construction, registration, and aggregated status rollups.
//!
//! Root discovery and snapshots are the deep part of the old pass-through
//! tier — they live here, behind the engine seam (`GitExecutor`), per ADR-0001.

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::{Path, PathBuf};

/// Maximum directory depth to walk while scanning for repo roots.
const SCAN_MAX_DEPTH: usize = 3;

// ------------------------------------------------------- root scanner ----

/// Discover candidate repo roots beneath `project_dir`.
///
/// Includes `project_dir` itself if it is a repo, then walks up to
/// [`SCAN_MAX_DEPTH`] levels deep looking for `.git` markers. IO errors during
/// the walk are silently skipped. Result is sorted & deduplicated.
pub fn scan_for_roots(engine: &dyn GitExecutor, dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();

    if engine.is_repo(dir) {
        found.push(dir.to_path_buf());
    }
    scan_dir(engine, dir, 0, &mut found);

    found.sort();
    found.dedup();
    found
}

fn scan_dir(engine: &dyn GitExecutor, dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth >= SCAN_MAX_DEPTH {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = match entry.file_type() {
            Ok(ft) => ft.is_dir(),
            Err(_) => continue,
        };
        if !is_dir {
            continue;
        }
        // A directory containing `.git` is a repo root.
        if path.join(".git").exists() {
            found.push(path.clone());
        }
        scan_dir(engine, &path, depth + 1, found);
    }
}

/// Build a full [`Root`] snapshot for `path` through the engine seam.
pub fn root_snapshot(engine: &dyn GitExecutor, path: &Path) -> TgResult<Root> {
    Ok(Root {
        id: RootId(path.to_path_buf()),
        path: path.to_path_buf(),
        remotes: engine.remotes(path)?,
        branches: engine.branches(path)?,
        current_branch: engine.current_branch(path)?,
        head: None,
        status: engine.status(path)?,
    })
}

// -------------------------------------------------------- registration ----

/// Discover candidate repo roots under `project_dir` via the Root scanner.
pub fn discover_roots(engine: &dyn GitExecutor, project_dir: &Path) -> Vec<PathBuf> {
    scan_for_roots(engine, project_dir)
}

/// Build a single [`Root`] snapshot for `path`.
pub fn build_root(engine: &dyn GitExecutor, path: &Path) -> TgResult<Root> {
    root_snapshot(engine, path)
}

/// Register `root` into `mgr` unless a root with the same id already exists.
pub fn register(mgr: &mut MultiRootManager, root: Root) {
    if !mgr.roots.iter().any(|r| r.id == root.id) {
        mgr.roots.push(root);
    }
}

/// Build & register a root for each `paths` entry. Returns one result per path.
pub fn register_all(
    engine: &dyn GitExecutor,
    mgr: &mut MultiRootManager,
    paths: &[PathBuf],
) -> Vec<TgResult<()>> {
    paths
        .iter()
        .map(|p| {
            let root = build_root(engine, p)?;
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
