//! 3-way merge conflict model and resolution helpers.
//!
//! A merge conflict in Git leaves up to three versions of a file in the index:
//! the common ancestor (`:1`), "ours" (`:2`), and "theirs" (`:3`). This module
//! reads those versions through the [`GitExecutor`] and offers helpers to stage
//! a resolved file or to apply the simplest automatic strategy.

#![allow(dead_code)]

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::{Path, PathBuf};

/// The three sides of a 3-way conflict for a single path.
///
/// Each field holds the full text content of that side at conflict time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictVersions {
    pub base: String,
    pub ours: String,
    pub theirs: String,
}

/// Return the list of conflicted paths reported by `git status`.
///
/// The engine already populates [`RootStatus::conflicted`]; this is a thin
/// accessor used by the UI and the batch helpers below.
pub fn detect(status: &RootStatus) -> Vec<PathBuf> {
    status.conflicted.clone()
}

/// Read the three sides of a conflicted file from the index.
///
/// Delegates to `git show` via [`GitExecutor::show_file`], which builds the
/// `<rev>:<path>` refspec: `:1` = base, `:2` = ours, `:3` = theirs.
pub fn read_versions(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
) -> TgResult<ConflictVersions> {
    let base = vcs.show_file(root, ":1", path)?;
    let ours = vcs.show_file(root, ":2", path)?;
    let theirs = vcs.show_file(root, ":3", path)?;
    Ok(ConflictVersions { base, ours, theirs })
}

/// Write `content` to the working tree copy and stage it as resolved.
pub fn write_resolution(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
    content: &str,
) -> TgResult<()> {
    std::fs::write(root.join(path), content)?;
    vcs.add(root, &[path.to_path_buf()])
}

/// Resolve a conflict by taking our side verbatim.
pub fn accept_ours(vcs: &dyn GitExecutor, root: &Path, path: &Path) -> TgResult<()> {
    let versions = read_versions(vcs, root, path)?;
    write_resolution(vcs, root, path, &versions.ours)
}

/// Resolve a conflict by taking their side verbatim.
pub fn accept_theirs(vcs: &dyn GitExecutor, root: &Path, path: &Path) -> TgResult<()> {
    let versions = read_versions(vcs, root, path)?;
    write_resolution(vcs, root, path, &versions.theirs)
}

/// Try to resolve every conflicted file with the cheapest safe strategy.
///
/// A "simple" conflict is one where both sides made the identical change
/// (`ours == theirs`); in that case either side is correct, so we write one and
/// stage it. Paths that genuinely differ are left untouched (still reported as
/// `Ok` so the caller can iterate; the unresolved count is unchanged).
pub fn resolve_all_simple(
    vcs: &dyn GitExecutor,
    root: &Path,
    status: &RootStatus,
) -> Vec<(PathBuf, TgResult<()>)> {
    status
        .conflicted
        .iter()
        .map(|path| {
            let result = read_versions(vcs, root, path).and_then(|v| {
                if v.ours == v.theirs {
                    write_resolution(vcs, root, path, &v.ours)
                } else {
                    Ok(())
                }
            });
            (path.clone(), result)
        })
        .collect()
}

/// Number of conflicts still awaiting resolution.
pub fn unresolved(status: &RootStatus) -> usize {
    status.conflicted.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeExecutor;

    #[test]
    fn write_resolution_writes_file_then_stages_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let engine = FakeExecutor::new();

        write_resolution(&engine, &repo, Path::new("src/main.rs"), "resolved!").unwrap();

        let written = std::fs::read_to_string(repo.join("src").join("main.rs")).unwrap();
        assert_eq!(written, "resolved!");
        let calls = engine.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "one stage call recorded");
    }
}
