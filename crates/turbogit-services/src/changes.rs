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

/// Stage the selected changes' paths.
pub fn stage_selected(vcs: &dyn GitExecutor, root: &Path, changes: &[Change]) -> TgResult<()> {
    let paths = paths_of(changes);
    if paths.is_empty() {
        return Ok(());
    }
    vcs.add(root, &paths)
}

/// Commit changes.
///
/// With no selection at all, commits everything tracked (`vcs.commit`). With
/// a selection of untouched files, stages those paths then commits the index;
/// files with active partial staging (`partial_changes`) are never re-staged —
/// they commit as-is from their index state so granular selections survive
/// (ADR-0013). `amend` passes through in every path.
pub fn commit_selected(
    vcs: &dyn GitExecutor,
    root: &Path,
    message: &str,
    changes: &[Change],
    partial_changes: &[Change],
    amend: bool,
) -> TgResult<CommitId> {
    if changes.is_empty() && partial_changes.is_empty() {
        vcs.commit(root, message, amend)
    } else {
        stage_selected(vcs, root, changes)?;
        vcs.commit_index(root, message, amend)
    }
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

    fn change(path: &str, staged: bool) -> Change {
        Change {
            path: PathBuf::from(path),
            status: ChangeStatus::Modified,
            chunks: vec![],
            staged,
            unstaged: false,
            orig_path: None,
        }
    }

    #[test]
    fn commit_selected_stages_then_commits_index_for_partial_selection() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");
        let selected = [change("a.txt", false), change("b.txt", false)];

        let id = commit_selected(&engine, &root, "msg", &selected, &[], false).unwrap();
        assert_eq!(id, "bbbb", "partial commits go through commit_index");
        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [
                Call::Add(vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")]),
                Call::CommitIndex,
            ],
            "stage first, then commit the index"
        );
    }

    #[test]
    fn commit_without_selection_commits_everything_tracked() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");

        let id = commit_selected(&engine, &root, "msg", &[], &[], true).unwrap();
        assert_eq!(id, "aaaa");
        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::CommitAll],
            "empty selection uses -a commit; amend flag passes through"
        );
    }

    #[test]
    fn commit_with_partial_selection_commits_index_without_restaging() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");
        let partial = [change("part.txt", false)];

        let id = commit_selected(&engine, &root, "msg", &[], &partial, false).unwrap();

        assert_eq!(id, "bbbb", "partial commits go through commit_index");
        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::CommitIndex],
            "a partially staged file must commit as-is from the index — \
             no whole-file re-stage may blow away the granular selection"
        );
    }

    #[test]
    fn commit_mixed_selection_stages_only_untouched_files() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");
        let untouched = [change("normal.txt", false)];
        let partial = [change("part.txt", false)];

        commit_selected(&engine, &root, "msg", &untouched, &partial, false).unwrap();

        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [
                Call::Add(vec![PathBuf::from("normal.txt")]),
                Call::CommitIndex,
            ],
            "untouched files keep stage-then-commit; the partially staged \
             file must not appear in the whole-file Add"
        );
    }
}
