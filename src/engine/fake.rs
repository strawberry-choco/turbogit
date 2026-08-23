//! In-memory [`GitExecutor`] adapter for tests.
//!
//! One adapter at the engine seam is hypothetical; two make it real — the CLI
//! executor in production, this fake in tests (ADR-0001). It records every
//! mutating call so tests can assert call sequences ("stage then commit the
//! index") without spawning git or touching a real repository.

#![cfg(test)]

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One recorded engine call (mutating operations only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Add(Vec<PathBuf>),
    CommitAll,
    CommitIndex,
    Push {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
    },
    PushDryRun {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
    },
}

/// In-memory fake. Configure per-test state through the public fields before
/// handing `&FakeExecutor` to core services.
pub struct FakeExecutor {
    /// Paths that answer `is_repo() == true` (test-only mutation via interior mutability).
    pub repos: Mutex<Vec<PathBuf>>,
    /// Working-tree file contents served by `show_file` (`:<n>` revs).
    pub files: Mutex<HashMap<PathBuf, String>>,
    /// Branches returned per repo path.
    pub branches: HashMap<PathBuf, Vec<Branch>>,
    /// Current branch per repo path (`None` = detached).
    pub current_branch: HashMap<PathBuf, Option<String>>,
    /// Remotes per repo path.
    pub remotes: HashMap<PathBuf, Vec<Remote>>,
    /// Status per repo path.
    pub status: HashMap<PathBuf, RootStatus>,
    /// Force-push to these branch names fails with `TgError::Other`.
    pub reject_force_branches: Vec<String>,
    /// Recorded mutating calls, in order.
    pub calls: Mutex<Vec<Call>>,
}

impl Default for FakeExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeExecutor {
    pub fn new() -> Self {
        Self {
            repos: Mutex::new(Vec::new()),
            files: Mutex::new(HashMap::new()),
            branches: HashMap::new(),
            current_branch: HashMap::new(),
            remotes: HashMap::new(),
            status: HashMap::new(),
            reject_force_branches: Vec::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn local_branch(&self, root: &Path) -> Option<Branch> {
        let cur = self.current_branch.get(root).cloned().flatten()?;
        Some(Branch {
            name: cur,
            kind: BranchKind::Local,
            tracking: None,
            favorite: false,
            protected: false,
            exists: true,
        })
    }
}

impl GitExecutor for FakeExecutor {
    // ---- read ----

    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        Ok(self.status.get(root).cloned().unwrap_or_default())
    }

    fn log(&self, _root: &Path, _opts: &LogOpts) -> TgResult<Vec<Commit>> {
        Ok(Vec::new())
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        let mut out = self.branches.get(root).cloned().unwrap_or_default();
        if let Some(cur) = self.local_branch(root) {
            if !out
                .iter()
                .any(|b| b.kind == BranchKind::Local && b.name == cur.name)
            {
                out.push(cur);
            }
        }
        Ok(out)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        Ok(self.current_branch.get(root).cloned().flatten())
    }

    fn ahead_behind(
        &self,
        _root: &Path,
        _branch: &str,
        _upstream: &str,
    ) -> TgResult<(usize, usize)> {
        Ok((0, 0))
    }

    fn outgoing_commits(
        &self,
        _root: &Path,
        _branch: &str,
        _upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        Ok(Vec::new())
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        Ok(self.remotes.get(root).cloned().unwrap_or_default())
    }

    fn stash_list(&self, _root: &Path) -> TgResult<Vec<Stash>> {
        Ok(Vec::new())
    }

    fn worktree_list(&self, _root: &Path) -> TgResult<Vec<Worktree>> {
        Ok(Vec::new())
    }

    fn submodule_paths(&self, _root: &Path) -> TgResult<Vec<PathBuf>> {
        Ok(Vec::new())
    }

    fn config_get(&self, _root: &Path, _key: &str) -> TgResult<Option<String>> {
        Ok(None)
    }

    // ---- mutating ----

    fn init(&self, root: &Path) -> TgResult<()> {
        std::fs::create_dir_all(root.join(".git"))?;
        self.repos.lock().unwrap().push(root.to_path_buf());
        Ok(())
    }

    fn clone(&self, _url: &str, dest: &Path, _depth: Option<usize>) -> TgResult<()> {
        std::fs::create_dir_all(dest.join(".git"))?;
        self.repos.lock().unwrap().push(dest.to_path_buf());
        Ok(())
    }

    fn add_remote(&self, _root: &Path, _name: &str, _url: &str) -> TgResult<()> {
        Ok(())
    }

    fn fetch(&self, _root: &Path, _remote: Option<&str>) -> TgResult<()> {
        Ok(())
    }

    fn pull(&self, _root: &Path, _rebase: bool) -> TgResult<()> {
        Ok(())
    }

    fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()> {
        if force && self.reject_force_branches.iter().any(|p| p == branch) {
            return Err(crate::error::TgError::Other(format!(
                "Refusing force-push to protected branch '{branch}'"
            )));
        }
        self.calls.lock().unwrap().push(Call::Push {
            root: root.to_path_buf(),
            remote: remote.to_string(),
            branch: branch.to_string(),
            force,
        });
        Ok(())
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String> {
        self.calls.lock().unwrap().push(Call::PushDryRun {
            root: root.to_path_buf(),
            remote: remote.to_string(),
            branch: branch.to_string(),
            force,
        });
        Ok(String::new())
    }

    fn commit(&self, _root: &Path, _message: &str, _amend: bool) -> TgResult<CommitId> {
        self.calls.lock().unwrap().push(Call::CommitAll);
        Ok("aaaa".into())
    }

    fn commit_index(&self, _root: &Path, _message: &str, _amend: bool) -> TgResult<CommitId> {
        self.calls.lock().unwrap().push(Call::CommitIndex);
        Ok("bbbb".into())
    }

    fn merge(&self, _root: &Path, _target: &str, _opts: &MergeOpts) -> TgResult<()> {
        Ok(())
    }

    fn rebase(&self, _root: &Path, _onto: &str, _opts: &RebaseOpts) -> TgResult<()> {
        Ok(())
    }

    fn cherry_pick(&self, _root: &Path, _commit: &str) -> TgResult<()> {
        Ok(())
    }

    fn abort(&self, _root: &Path, _op: &str) -> TgResult<()> {
        Ok(())
    }

    fn continue_op(&self, _root: &Path, _op: &str) -> TgResult<()> {
        Ok(())
    }

    fn rebase_interactive(&self, _root: &Path, _plan: &[RebasePlanEntry]) -> TgResult<()> {
        Ok(())
    }

    fn stash_push(&self, _root: &Path, _message: &str, _keep_index: bool) -> TgResult<()> {
        Ok(())
    }

    fn stash_pop(&self, _root: &Path, _index: usize) -> TgResult<()> {
        Ok(())
    }

    fn stash_drop(&self, _root: &Path, _index: usize) -> TgResult<()> {
        Ok(())
    }

    fn worktree_add(&self, _root: &Path, _path: &Path, _branch: &str) -> TgResult<()> {
        Ok(())
    }

    // ---- staging / working tree ----

    fn add(&self, _root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls.lock().unwrap().push(Call::Add(paths.to_vec()));
        Ok(())
    }

    fn add_all(&self, _root: &Path) -> TgResult<()> {
        Ok(())
    }

    fn unstage(&self, _root: &Path, _paths: &[PathBuf]) -> TgResult<()> {
        Ok(())
    }

    fn restore(&self, _root: &Path, _paths: &[PathBuf]) -> TgResult<()> {
        Ok(())
    }

    fn apply_patch_to_index(&self, _root: &Path, _patch: &str) -> TgResult<()> {
        Ok(())
    }

    // ---- branches ----

    fn branch_create(
        &self,
        _root: &Path,
        _name: &str,
        _checkout: bool,
        _start_point: Option<&str>,
    ) -> TgResult<()> {
        Ok(())
    }

    fn branch_checkout(&self, _root: &Path, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn branch_delete(&self, _root: &Path, _name: &str, _force: bool) -> TgResult<()> {
        Ok(())
    }

    fn branch_delete_remote(&self, _root: &Path, _remote: &str, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn branch_rename(&self, _root: &Path, _old: &str, _new: &str) -> TgResult<()> {
        Ok(())
    }

    // ---- tags ----

    fn tag_create(&self, _root: &Path, _name: &str, _message: Option<&str>) -> TgResult<()> {
        Ok(())
    }

    fn tag_list(&self, _root: &Path) -> TgResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn tag_checkout(&self, _root: &Path, _name: &str) -> TgResult<()> {
        Ok(())
    }

    fn tag_push(
        &self,
        _root: &Path,
        _remote: &str,
        _name: Option<&str>,
        _all: bool,
    ) -> TgResult<()> {
        Ok(())
    }

    // ---- diff / blame / history ----

    fn diff(&self, _root: &Path, _opts: &DiffOpts) -> TgResult<String> {
        Ok(String::new())
    }

    fn blame(&self, _root: &Path, _path: &Path, _rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        Ok(Vec::new())
    }

    fn show_file(&self, _root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        // Conflict reads use index revs ":1"/":2"/":3"; the fake serves one
        // content per path regardless of rev — enough for resolution tests.
        let _ = rev;
        Ok(self
            .files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .unwrap_or_default())
    }

    // ---- revert / undo ----

    fn revert(&self, _root: &Path, _commit: &str) -> TgResult<()> {
        Ok(())
    }

    fn undo_last_commit(&self, _root: &Path) -> TgResult<()> {
        Ok(())
    }

    fn stash_apply(&self, _root: &Path, _index: usize) -> TgResult<()> {
        Ok(())
    }
}
