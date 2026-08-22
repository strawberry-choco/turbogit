//! IntelliJ-style changelists, staging-area splitting, and partial-commit
//! orchestration for TurboGit.
//!
//! This module groups working-tree [`Change`]s into named changelists, splits
//! them by their `staged` flag, and orchestrates stage/commit/discard flows on
//! top of [`GitExecutor`].

#![allow(dead_code)]

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::{Path, PathBuf};

/// Default changelist holding tracked local changes.
pub const DEFAULT_CHANGELIST: &str = "Local Changes";

/// Changelist holding newly-unversioned files.
pub const UNVERSIONED_CHANGELIST: &str = "Unversioned Files";

/// Whether a change is tracked (i.e. not unversioned/ignored).
fn is_tracked(status: ChangeStatus) -> bool {
    !matches!(status, ChangeStatus::Unversioned | ChangeStatus::Ignored)
}

/// Build the two canonical changelists from a [`RootStatus`].
///
/// "Local Changes" (active) contains every tracked change; "Unversioned Files"
/// (inactive) contains the unversioned ones. Each change keeps its own `staged`
/// flag untouched.
pub fn build_changelists(status: &RootStatus, root: &RootId) -> Vec<Changelist> {
    let tracked: Vec<Change> = status
        .changes
        .iter()
        .filter(|c| is_tracked(c.status))
        .cloned()
        .collect();
    let unversioned: Vec<Change> = status
        .changes
        .iter()
        .filter(|c| c.status == ChangeStatus::Unversioned)
        .cloned()
        .collect();

    vec![
        Changelist {
            name: DEFAULT_CHANGELIST.to_string(),
            active: true,
            changes: tracked,
            root: root.clone(),
        },
        Changelist {
            name: UNVERSIONED_CHANGELIST.to_string(),
            active: false,
            changes: unversioned,
            root: root.clone(),
        },
    ]
}

/// Split changes into (staged, unstaged) according to `change.staged`.
pub fn split_by_staging(status: &RootStatus, _root: &RootId) -> (Vec<Change>, Vec<Change>) {
    let staged: Vec<Change> = status
        .changes
        .iter()
        .filter(|c| c.staged)
        .cloned()
        .collect();
    let unstaged: Vec<Change> = status
        .changes
        .iter()
        .filter(|c| !c.staged)
        .cloned()
        .collect();
    (staged, unstaged)
}

/// Move changes whose path is in `paths` from changelist `from` to `to`.
pub fn move_changes(changelists: &mut [Changelist], from: &str, to: &str, paths: &[PathBuf]) {
    // Find the destination index up-front so we can keep borrowing disjoint.
    let dest_idx = match changelists.iter().position(|cl| cl.name == to) {
        Some(i) => i,
        None => return,
    };

    let mut moved: Vec<Change> = Vec::new();
    if let Some(src) = changelists.iter_mut().find(|cl| cl.name == from) {
        let mut i = 0;
        while i < src.changes.len() {
            if paths.contains(&src.changes[i].path) {
                moved.push(src.changes.remove(i));
            } else {
                i += 1;
            }
        }
    }

    changelists[dest_idx].changes.extend(moved);
}

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

/// Unstage the selected changes' paths.
pub fn unstage_selected(vcs: &dyn GitExecutor, root: &Path, changes: &[Change]) -> TgResult<()> {
    let paths = paths_of(changes);
    if paths.is_empty() {
        return Ok(());
    }
    vcs.unstage(root, &paths)
}

/// Commit changes.
///
/// With no selected changes, commits everything tracked (`vcs.commit`). With a
/// selection, stages the chosen paths then commits the index
/// (`vcs.commit_index`); a partial commit never amends.
pub fn commit_selected(
    vcs: &dyn GitExecutor,
    root: &Path,
    message: &str,
    changes: &[Change],
    amend: bool,
) -> TgResult<CommitId> {
    if changes.is_empty() {
        vcs.commit(root, message, amend)
    } else {
        stage_selected(vcs, root, changes)?;
        vcs.commit_index(root, message, false)
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
    use crate::engine::fake::{Call, FakeExecutor};

    fn change(path: &str, staged: bool) -> Change {
        Change {
            path: PathBuf::from(path),
            status: ChangeStatus::Modified,
            chunks: vec![],
            staged,
        }
    }

    #[test]
    fn commit_selected_stages_then_commits_index_for_partial_selection() {
        let engine = FakeExecutor::new();
        let root = PathBuf::from("/repo");
        let selected = [change("a.txt", false), change("b.txt", false)];

        let id = commit_selected(&engine, &root, "msg", &selected, false).unwrap();
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

        let id = commit_selected(&engine, &root, "msg", &[], true).unwrap();
        assert_eq!(id, "aaaa");
        assert_eq!(
            engine.calls.lock().unwrap().as_slice(),
            [Call::CommitAll],
            "empty selection uses -a commit; amend flag passes through"
        );
    }
}
