//! Working-tree change orchestration for TurboGit.
//!
//! This module orchestrates stage/commit/discard flows over working-tree
//! [`Change`]s on top of [`GitExecutor`].

use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// Default changelist holding tracked local changes.
pub const DEFAULT_CHANGELIST: &str = "Local Changes";

/// Changelist holding newly-unversioned files.
pub const UNVERSIONED_CHANGELIST: &str = "Unversioned Files";

/// Collect the paths of the supplied changes.
fn paths_of(changes: &[Change]) -> Vec<PathBuf> {
    changes.iter().map(|c| c.path.clone()).collect()
}

/// Stage whole files (`vcs.add`) — the plain route, with no diff cache in the
/// middle, so a staging click always reaches the index.
pub fn stage_paths(vcs: &dyn GitExecutor, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    vcs.add(root, paths)
}

/// Un-stage whole files (`vcs.unstage`), the mirror of [`stage_paths`].
pub fn unstage_paths(vcs: &dyn GitExecutor, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    vcs.unstage(root, paths)
}

/// Commit the index and nothing else.
///
/// One call, one meaning: what the developer staged is what lands. The stage-
/// then-commit arm this replaced swept in every tracked change whenever the
/// window's own selection was empty, and the partial-staging partition existed
/// only to protect a granular selection — which is already in the index, and is
/// therefore this function's only input now.
pub fn commit_index(
    vcs: &dyn GitExecutor,
    root: &Path,
    message: &str,
    amend: bool,
) -> TgResult<CommitId> {
    vcs.commit_index(root, message, amend)
}

/// Discard working-tree edits for the selected changes via `vcs.restore`.
pub fn discard_changes(vcs: &dyn GitExecutor, root: &Path, changes: &[Change]) -> TgResult<()> {
    let paths = paths_of(changes);
    if paths.is_empty() {
        return Ok(());
    }
    vcs.restore(root, &paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_engine::fake::{Call, FakeExecutor};

    #[test]
    fn committing_stages_nothing_and_takes_the_index() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        let id = commit_index(&engine, &root, "msg", false).unwrap();

        assert_eq!(id, "bbbb", "the index is committed through commit_index");
        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::CommitIndex],
            "a commit stages nothing first: the index is already the decision"
        );
    }

    #[test]
    fn an_empty_index_still_commits_through_the_index_path() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        commit_index(&engine, &root, "msg", false).unwrap();

        let calls = engine.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![Call::CommitIndex],
            "an empty selection must not fall back to `git commit -a`, which \
             sweeps every tracked change — that surprise is what this removed"
        );
        assert!(
            !calls.contains(&Call::CommitAll),
            "the whole-tree commit arm is gone"
        );
    }

    #[test]
    fn amend_passes_straight_through_to_the_index_commit() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        commit_index(&engine, &root, "msg", true).unwrap();

        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::CommitIndex],
            "amend rewrites the index commit; it never selects a different call"
        );
    }

    #[test]
    fn a_whole_file_stage_reaches_the_index_with_no_diff_cache_in_between() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");
        let paths = [PathBuf::from("src/app.rs")];

        stage_paths(&engine, &root, &paths).unwrap();
        // Empty inputs are a no-op rather than an empty `git add`.
        stage_paths(&engine, &root, &[]).unwrap();

        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::Add(vec![PathBuf::from("src/app.rs")])],
            "one plain Add for one click — the granular pipeline's peek on the \
             diff cache is the thing that made a staging click a silent no-op"
        );
    }
}
