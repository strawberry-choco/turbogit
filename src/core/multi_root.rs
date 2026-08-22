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

fn scan_dir(_engine: &dyn GitExecutor, dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
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
        scan_dir(_engine, &path, depth + 1, found);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeExecutor;

    fn repo_at(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
    }

    #[test]
    fn scanner_finds_repos_within_depth_and_skips_deeper_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        // depth 1 and depth 2 repos exist; a depth-3 nested repo must be skipped
        let d1 = base.join("one");
        let d2 = base.join("a").join("two");
        let d3 = base.join("a").join("b").join("c").join("three");
        repo_at(&d1);
        repo_at(&d2);
        repo_at(&d3);

        let engine = FakeExecutor::new();
        let mut found = scan_for_roots(&engine, base);
        found.retain(|p| p != base); // drop the tempdir root itself

        assert!(found.contains(&d1), "depth-1 repo should be found");
        assert!(found.contains(&d2), "depth-2 repo should be found");
        assert!(!found.contains(&d3), "beyond SCAN_MAX_DEPTH must not be walked");
    }

    #[test]
    fn snapshot_populates_fields_through_the_seam() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        repo_at(&repo);
        let mut engine = FakeExecutor::new();
        let root_id = RootId(repo.clone());
        engine.branches.insert(repo.clone(), vec![Branch {
            name: "main".into(),
            kind: BranchKind::Local,
            tracking: Some("origin/main".into()),
            favorite: false,
            protected: false,
            exists: true,
        }]);
        engine.current_branch.insert(repo.clone(), Some("main".into()));

        let snap = root_snapshot(&engine, &repo).unwrap();
        assert_eq!(snap.id, root_id);
        assert_eq!(snap.current_branch.as_deref(), Some("main"));
        assert_eq!(snap.branches.len(), 1);
        assert!(snap.head.is_none());
    }

    #[test]
    fn register_all_is_idempotent_per_root_id() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        repo_at(&repo);
        let engine = FakeExecutor::new();
        let mut mgr = MultiRootManager::default();

        let paths = vec![repo.clone(), repo.clone()];
        let results = register_all(&engine, &mut mgr, &paths);
        assert!(results.iter().all(|r| r.is_ok()));
        assert_eq!(mgr.roots.len(), 1, "same id registered once");
    }
}
