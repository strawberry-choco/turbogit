use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

const SCAN_MAX_DEPTH: usize = 3;

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
    scan_dir(engine, dir, 0, SCAN_MAX_DEPTH, &mut found);

    found.sort();
    found.dedup();
    found
}

/// Discover candidate repo roots beneath `dir` at **any** depth (no depth
/// cap). Used by the Welcome "Attach workspace root" flow (issue #34), which
/// registers every repository in a chosen directory tree, unlike
/// [`scan_for_roots`] which stops at [`SCAN_MAX_DEPTH`].
pub fn scan_deep(engine: &dyn GitExecutor, dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();

    if engine.is_repo(dir) {
        found.push(dir.to_path_buf());
    }
    scan_dir(engine, dir, 0, usize::MAX, &mut found);

    found.sort();
    found.dedup();
    found
}

fn scan_dir(
    _engine: &dyn GitExecutor,
    dir: &Path,
    depth: usize,
    max_depth: usize,
    found: &mut Vec<PathBuf>,
) {
    if depth >= max_depth {
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
        scan_dir(_engine, &path, depth + 1, max_depth, found);
        if path.join(".git").exists() {
            found.push(path);
        }
    }
}

/// Is `dir`, or any ancestor of it, a git work tree?
///
/// The marker test is [`scan_dir`]'s, `.git` present, as a directory in an
/// ordinary checkout and as a *file* in a worktree or a submodule. It is a
/// filesystem question rather than an engine call on purpose:
/// [`GitExecutor::is_repo`] is backend-dependent, and the git2 adapter's
/// `Repository::open` does not search upward, so it answers "is this exactly a
/// repo root" where this answers git's own "am I inside one". The two are
/// different questions and the callers that need the second must not be handed
/// the first.
///
/// `GIT_DIR` / `core.worktree` in a parent are not consulted: the marker walk
/// is what the scanner already trusts, and a `.git` file covers the cases that
/// matter (worktrees, submodules).
pub fn is_inside_work_tree(dir: &Path) -> bool {
    dir.ancestors().any(|a| a.join(".git").exists())
}

pub fn root_snapshot(engine: &dyn GitExecutor, path: &Path) -> TgResult<Root> {
    Ok(Root {
        id: RootId(path.into()),
        path: path.to_path_buf(),
        remotes: engine.remotes(path)?,
        branches: engine.branches(path)?,
        current_branch: engine.current_branch(path)?,
        head: None,
        status: engine.status(path)?,
    })
}

pub fn discover_roots(engine: &dyn GitExecutor, project_dir: &Path) -> Vec<PathBuf> {
    scan_for_roots(engine, project_dir)
}

pub fn build_root(engine: &dyn GitExecutor, path: &Path) -> TgResult<Root> {
    root_snapshot(engine, path)
}

/// Register `root` into `mgr`, replacing any existing snapshot with the same
/// id so re-registration after an operation refreshes branches / HEAD /
/// status instead of leaving stale data behind.
pub fn register(mgr: &mut MultiRootManager, root: Root) {
    if let Some(existing) = mgr.roots.iter_mut().find(|r| r.id == root.id) {
        *existing = root;
    } else {
        mgr.roots.push(root);
    }
}

/// Build & register a root for each `paths` entry. Returns one result per path.
pub fn register_all<P: AsRef<Path>>(
    engine: &dyn GitExecutor,
    mgr: &mut MultiRootManager,
    paths: &[P],
) -> Vec<TgResult<()>> {
    paths
        .iter()
        .map(|p| {
            let root = build_root(engine, p.as_ref())?;
            register(mgr, root);
            Ok(())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_engine::fake::FakeExecutor;

    fn repo_at(dir: &Path) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
    }

    fn root_snapshot_at(path: &Path) -> Root {
        Root {
            id: RootId(path.into()),
            path: path.to_path_buf(),
            remotes: Vec::new(),
            branches: Vec::new(),
            current_branch: None,
            head: None,
            status: RootStatus::default(),
        }
    }

    #[test]
    fn is_inside_work_tree_walks_up_to_the_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        repo_at(&repo);
        let nested = repo.join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        assert!(is_inside_work_tree(&repo), "the root itself");
        assert!(
            is_inside_work_tree(&nested),
            "an ancestor carries the marker"
        );

        let outside = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        assert!(!is_inside_work_tree(&outside), "no marker anywhere above");

        let linked = tmp.path().join("linked");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join(".git"), "gitdir: ../repo/.git/worktrees/w\n").unwrap();
        assert!(
            is_inside_work_tree(&linked.join("sub")),
            "a worktree's `.git` is a file, and it still marks the tree"
        );
    }

    #[test]
    fn scan_deep_finds_repos_beyond_the_bounded_scan_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        let deep = base.join("a").join("b").join("c").join("d").join("deep");
        repo_at(&deep);

        let engine = FakeExecutor::new();
        let bounded = scan_for_roots(&engine, base);
        assert!(
            !bounded.contains(&deep),
            "bounded scan must NOT see the deep repo"
        );

        let mut deep_result = scan_deep(&engine, base);
        deep_result.retain(|p| p != base);
        assert!(
            deep_result.contains(&deep),
            "deep scan must find repos beyond SCAN_MAX_DEPTH; got {deep_result:?}"
        );
    }

    #[test]
    fn scanner_finds_repos_within_depth_and_skips_deeper_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        let d1 = base.join("one");
        let d2 = base.join("a").join("two");
        let d3 = base.join("a").join("b").join("c").join("three");
        repo_at(&d1);
        repo_at(&d2);
        repo_at(&d3);

        let engine = FakeExecutor::new();
        let mut found = scan_for_roots(&engine, base);
        found.retain(|p| p != base);
        assert!(found.contains(&d1), "depth-1 repo should be found");
        assert!(found.contains(&d2), "depth-2 repo should be found");
        assert!(
            !found.contains(&d3),
            "beyond SCAN_MAX_DEPTH must not be walked"
        );
    }

    #[test]
    fn snapshot_populates_fields_through_the_seam() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        repo_at(&repo);
        let mut engine = FakeExecutor::new();
        let root_id = RootId(repo.clone().into());
        engine.branches.insert(
            repo.clone(),
            vec![Branch {
                name: "main".into(),
                kind: BranchKind::Local,
                tracking: Upstream::from_git_ref("origin/main"),
                favorite: false,
                protected: false,
                exists: true,
                ahead: 0,
                behind: 0,
                gone: false,
                last_touched: None,
                tip: None,
                remote: None,
            }],
        );
        engine
            .current_branch
            .insert(repo.clone(), Some("main".into()));

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

    #[test]
    fn register_replaces_the_earlier_snapshot_of_the_same_root_id() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("r");
        repo_at(&repo);

        let mut mgr = MultiRootManager::default();
        register(
            &mut mgr,
            Root {
                current_branch: Some("first".into()),
                ..root_snapshot_at(&repo)
            },
        );
        register(
            &mut mgr,
            Root {
                current_branch: Some("second".into()),
                ..root_snapshot_at(&repo)
            },
        );

        assert_eq!(mgr.roots.len(), 1, "same id registered once");
        assert_eq!(
            mgr.roots[0].current_branch.as_deref(),
            Some("second"),
            "the later snapshot must replace the earlier one",
        );
    }
}
