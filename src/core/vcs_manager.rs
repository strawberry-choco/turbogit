//! Central VCS service: owns an executor and exposes thin pass-throughs plus
//! root discovery / snapshot helpers built on top of `GitExecutor`.

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Owns the git engine. All concrete git work goes through `executor`.
pub struct VcsManager {
    pub executor: Arc<dyn GitExecutor>,
    pub settings: VcsSettings,
}

/// Maximum directory depth to walk while scanning for repo roots.
const SCAN_MAX_DEPTH: usize = 3;

impl VcsManager {
    /// Build a `VcsManager` backed by the default CLI executor.
    pub fn new(settings: VcsSettings) -> Self {
        Self {
            executor: Arc::new(crate::engine::cli::CliExecutor {
                settings: settings.clone(),
            }),
            settings,
        }
    }

    /// Build a `VcsManager` around a caller-supplied executor (testing / DI).
    pub fn with_executor(executor: Arc<dyn GitExecutor>, settings: VcsSettings) -> Self {
        Self { executor, settings }
    }

    // ---- pass-throughs mirroring `GitExecutor` exactly ----

    pub fn status(&self, root: &Path) -> TgResult<RootStatus> {
        self.executor.status(root)
    }

    pub fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        self.executor.branches(root)
    }

    pub fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        self.executor.log(root, opts)
    }

    pub fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        self.executor.current_branch(root)
    }

    pub fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)> {
        self.executor.ahead_behind(root, branch, upstream)
    }

    pub fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        self.executor.remotes(root)
    }

    pub fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        self.executor.config_get(root, key)
    }

    pub fn init(&self, root: &Path) -> TgResult<()> {
        self.executor.init(root)
    }

    pub fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()> {
        // `Arc::clone` would shadow the trait method, so call it explicitly.
        GitExecutor::clone(&*self.executor, url, dest, depth)
    }

    pub fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()> {
        self.executor.add_remote(root, name, url)
    }

    // ---- mutating pass-throughs (used by tests + later phases) ----
    // `Arc::clone` would shadow the trait `clone`, so mutating ops are called
    // through the `GitExecutor` trait explicitly.

    pub fn commit(&self, root: &Path, message: &str, amend: bool) -> TgResult<crate::model::CommitId> {
        GitExecutor::commit(&*self.executor, root, message, amend)
    }
    pub fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()> {
        GitExecutor::fetch(&*self.executor, root, remote)
    }
    pub fn pull(&self, root: &Path, rebase: bool) -> TgResult<()> {
        GitExecutor::pull(&*self.executor, root, rebase)
    }
    pub fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()> {
        GitExecutor::push(&*self.executor, root, remote, branch, force)
    }
    pub fn merge(&self, root: &Path, target: &str, opts: &crate::model::MergeOpts) -> TgResult<()> {
        GitExecutor::merge(&*self.executor, root, target, opts)
    }
    pub fn rebase(&self, root: &Path, onto: &str, opts: &crate::model::RebaseOpts) -> TgResult<()> {
        GitExecutor::rebase(&*self.executor, root, onto, opts)
    }
    pub fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()> {
        GitExecutor::cherry_pick(&*self.executor, root, commit)
    }
    pub fn commit_index(&self, root: &Path, message: &str, amend: bool) -> TgResult<crate::model::CommitId> {
        GitExecutor::commit_index(&*self.executor, root, message, amend)
    }
    pub fn abort(&self, root: &Path, op: &str) -> TgResult<()> {
        GitExecutor::abort(&*self.executor, root, op)
    }
    pub fn continue_op(&self, root: &Path, op: &str) -> TgResult<()> {
        GitExecutor::continue_op(&*self.executor, root, op)
    }
    pub fn rebase_interactive(
        &self,
        root: &Path,
        plan: &[crate::model::RebasePlanEntry],
    ) -> TgResult<()> {
        GitExecutor::rebase_interactive(&*self.executor, root, plan)
    }
    pub fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
        GitExecutor::stash_push(&*self.executor, root, message, keep_index)
    }
    pub fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()> {
        GitExecutor::stash_pop(&*self.executor, root, index)
    }
    pub fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()> {
        GitExecutor::stash_drop(&*self.executor, root, index)
    }
    pub fn worktree_add(&self, root: &Path, path: &Path, branch: &str) -> TgResult<()> {
        GitExecutor::worktree_add(&*self.executor, root, path, branch)
    }
    pub fn stash_list(&self, root: &Path) -> TgResult<Vec<crate::model::Stash>> {
        GitExecutor::stash_list(&*self.executor, root)
    }
    pub fn worktree_list(&self, root: &Path) -> TgResult<Vec<crate::model::Worktree>> {
        GitExecutor::worktree_list(&*self.executor, root)
    }
    pub fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>> {
        GitExecutor::submodule_paths(&*self.executor, root)
    }

    // ---- staging / working tree ----
    pub fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        GitExecutor::add(&*self.executor, root, paths)
    }
    pub fn add_all(&self, root: &Path) -> TgResult<()> {
        GitExecutor::add_all(&*self.executor, root)
    }
    pub fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        GitExecutor::unstage(&*self.executor, root, paths)
    }
    pub fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        GitExecutor::restore(&*self.executor, root, paths)
    }
    pub fn apply_patch_to_index(&self, root: &Path, patch: &str) -> TgResult<()> {
        GitExecutor::apply_patch_to_index(&*self.executor, root, patch)
    }

    // ---- branches ----
    pub fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()> {
        GitExecutor::branch_create(&*self.executor, root, name, checkout, start_point)
    }
    pub fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        GitExecutor::branch_checkout(&*self.executor, root, name)
    }
    pub fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()> {
        GitExecutor::branch_delete(&*self.executor, root, name, force)
    }
    pub fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()> {
        GitExecutor::branch_delete_remote(&*self.executor, root, remote, name)
    }
    pub fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        GitExecutor::branch_rename(&*self.executor, root, old, new)
    }

    // ---- tags ----
    pub fn tag_create(&self, root: &Path, name: &str, message: Option<&str>) -> TgResult<()> {
        GitExecutor::tag_create(&*self.executor, root, name, message)
    }
    pub fn tag_list(&self, root: &Path) -> TgResult<Vec<String>> {
        GitExecutor::tag_list(&*self.executor, root)
    }
    pub fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        GitExecutor::tag_checkout(&*self.executor, root, name)
    }
    pub fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()> {
        GitExecutor::tag_push(&*self.executor, root, remote, name, all)
    }

    // ---- diff / blame / history ----
    pub fn diff(&self, root: &Path, opts: &crate::model::DiffOpts) -> TgResult<String> {
        GitExecutor::diff(&*self.executor, root, opts)
    }
    pub fn blame(
        &self,
        root: &Path,
        path: &Path,
        rev: Option<&str>,
    ) -> TgResult<Vec<crate::model::BlameLine>> {
        GitExecutor::blame(&*self.executor, root, path, rev)
    }
    pub fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        GitExecutor::show_file(&*self.executor, root, rev, path)
    }

    // ---- revert / undo ----
    pub fn revert(&self, root: &Path, commit: &str) -> TgResult<()> {
        GitExecutor::revert(&*self.executor, root, commit)
    }
    pub fn undo_last_commit(&self, root: &Path) -> TgResult<()> {
        GitExecutor::undo_last_commit(&*self.executor, root)
    }
    pub fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()> {
        GitExecutor::stash_apply(&*self.executor, root, index)
    }

    /// Collect candidate repo roots under `dir`.
    ///
    /// Includes `dir` itself if it is a repo, then walks up to
    /// [`SCAN_MAX_DEPTH`] levels deep looking for `.git` markers. IO errors
    /// during the walk are silently skipped. Result is sorted & deduplicated.
    pub fn scan_for_roots(&self, dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = Vec::new();

        if self.executor.is_repo(dir) {
            found.push(dir.to_path_buf());
        }
        self.scan_dir(dir, 0, &mut found);

        found.sort();
        found.dedup();
        found
    }

    fn scan_dir(&self, dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
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
            self.scan_dir(&path, depth + 1, found);
        }
    }

    /// Build a full [`Root`] snapshot for `path`.
    pub fn root_snapshot(&self, path: &Path) -> TgResult<Root> {
        Ok(Root {
            id: RootId(path.to_path_buf()),
            path: path.to_path_buf(),
            remotes: self.remotes(path)?,
            branches: self.branches(path)?,
            current_branch: self.current_branch(path)?,
            head: None,
            status: self.status(path)?,
        })
    }
}
