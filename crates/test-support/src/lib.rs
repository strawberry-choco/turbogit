//! Shared headless test doubles (DDD split issue 09).
//!
//! The recording executor that the integration suites inject at the executor
//! boundary. Kept dependency-light on purpose: it needs only the engine port
//! and the domain model, so `turbogit-app`'s tests can use it without ever
//! pulling in egui or the UI crate.
//!
//! Harness helpers that drive `turbogit_ui::render` through `egui_kittest`
//! live in each UI-facing test crate instead — they need the egui stack and
//! would drag it onto every consumer of this crate.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{
    BlameLine, Branch, Change, Commit, CommitId, CommitRef, DiffOpts, LogOpts, MergeOpts,
    RebaseOpts, RebasePlanEntry, Remote, RootStatus, Stash, Worktree,
};
use turbogit_engine_api::GitExecutor;

//
// `engine::fake` is unit-test-only (`#[cfg(test)]`), so integration tests
// assert flag selection at the executor boundary through this transparent
// wrapper: every call delegates to a real inner engine while push /
// push-dry-run invocations are recorded verbatim (remote, branch, force).

/// One recorded mutating call at the executor boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCall {
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
    ApplyPatch {
        direction: turbogit_engine_api::ApplyDirection,
    },
    AddIntentToAdd(Vec<PathBuf>),
    Add(Vec<PathBuf>),
    CommitAll,
    CommitIndex,
}

/// Delegating [`GitExecutor`] that records push / dry-run calls.
pub struct RecordingExecutor {
    pub inner: Arc<dyn GitExecutor>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl RecordingExecutor {
    pub fn new(inner: Arc<dyn GitExecutor>) -> Self {
        Self {
            inner,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Snapshot of every recorded call so far, in order.
    pub fn recorded(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("calls mutex").clone()
    }

    /// True once a `Push` with exactly these fields has been recorded.
    pub fn contains_push(&self, remote: &str, branch: &str, force: bool) -> bool {
        self.recorded().iter().any(|c| match c {
            RecordedCall::Push {
                remote: r,
                branch: b,
                force: f,
                ..
            } => r == remote && b == branch && *f == force,
            _ => false,
        })
    }

    /// True once a `PushDryRun` with exactly these fields has been recorded.
    pub fn contains_dry_run(&self, remote: &str, branch: &str, force: bool) -> bool {
        self.recorded().iter().any(|c| match c {
            RecordedCall::PushDryRun {
                remote: r,
                branch: b,
                force: f,
                ..
            } => r == remote && b == branch && *f == force,
            _ => false,
        })
    }
}

impl GitExecutor for RecordingExecutor {
    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        self.inner.status(root)
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        self.inner.log(root, opts)
    }

    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        self.inner.ref_decorations(root)
    }

    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        self.inner.commit_files(root, commit)
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        self.inner.branches(root)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        self.inner.current_branch(root)
    }

    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)> {
        self.inner.ahead_behind(root, branch, upstream)
    }

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        self.inner.outgoing_commits(root, branch, upstream)
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        self.inner.remotes(root)
    }

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>> {
        self.inner.stash_list(root)
    }

    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>> {
        self.inner.worktree_list(root)
    }

    fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>> {
        self.inner.submodule_paths(root)
    }

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        self.inner.config_get(root, key)
    }

    fn init(&self, root: &Path) -> TgResult<()> {
        self.inner.init(root)
    }

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()> {
        // `clone` collides with `Clone::clone`; disambiguate via the trait.
        GitExecutor::clone(&*self.inner, url, dest, depth)
    }

    fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()> {
        self.inner.add_remote(root, name, url)
    }

    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()> {
        self.inner.fetch(root, remote)
    }

    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()> {
        self.inner.pull(root, rebase)
    }

    fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Push {
                root: root.to_path_buf(),
                remote: remote.to_string(),
                branch: branch.to_string(),
                force,
            });
        self.inner.push(root, remote, branch, force)
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::PushDryRun {
                root: root.to_path_buf(),
                remote: remote.to_string(),
                branch: branch.to_string(),
                force,
            });
        self.inner.push_dry_run(root, remote, branch, force)
    }

    fn commit(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::CommitAll);
        self.inner.commit(root, message, amend)
    }

    fn commit_index(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::CommitIndex);
        self.inner.commit_index(root, message, amend)
    }

    fn merge(&self, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()> {
        self.inner.merge(root, target, opts)
    }

    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()> {
        self.inner.rebase(root, onto, opts)
    }

    fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()> {
        self.inner.cherry_pick(root, commit)
    }

    fn abort(&self, root: &Path, op: &str) -> TgResult<()> {
        self.inner.abort(root, op)
    }

    fn continue_op(&self, root: &Path, op: &str) -> TgResult<()> {
        self.inner.continue_op(root, op)
    }

    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
        self.inner.rebase_interactive(root, plan)
    }

    fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
        self.inner.stash_push(root, message, keep_index)
    }

    fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()> {
        self.inner.stash_pop(root, index)
    }

    fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()> {
        self.inner.stash_drop(root, index)
    }

    fn worktree_add(&self, root: &Path, path: &Path, branch: &str) -> TgResult<()> {
        self.inner.worktree_add(root, path, branch)
    }

    fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Add(paths.to_vec()));
        self.inner.add(root, paths)
    }

    fn add_all(&self, root: &Path) -> TgResult<()> {
        self.inner.add_all(root)
    }

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.inner.unstage(root, paths)
    }

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.inner.restore(root, paths)
    }

    fn apply_patch_to_index(
        &self,
        _root: &Path,
        _patch: &str,
        direction: turbogit_engine_api::ApplyDirection,
    ) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::ApplyPatch { direction });
        self.inner.apply_patch_to_index(_root, _patch, direction)
    }

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::AddIntentToAdd(paths.to_vec()));
        self.inner.add_intent_to_add(root, paths)
    }

    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()> {
        self.inner.branch_create(root, name, checkout, start_point)
    }

    fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        self.inner.branch_checkout(root, name)
    }

    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()> {
        self.inner.branch_delete(root, name, force)
    }

    fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()> {
        self.inner.branch_delete_remote(root, remote, name)
    }

    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        self.inner.branch_rename(root, old, new)
    }

    fn tag_create(&self, root: &Path, name: &str, message: Option<&str>) -> TgResult<()> {
        self.inner.tag_create(root, name, message)
    }

    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>> {
        self.inner.tag_list(root)
    }

    fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        self.inner.tag_checkout(root, name)
    }

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()> {
        self.inner.tag_push(root, remote, name, all)
    }

    fn diff(&self, root: &Path, opts: &DiffOpts) -> TgResult<String> {
        self.inner.diff(root, opts)
    }

    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        self.inner.blame(root, path, rev)
    }

    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        self.inner.show_file(root, rev, path)
    }

    fn show_file_bytes(&self, root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>> {
        self.inner.show_file_bytes(root, rev, path)
    }

    fn revert(&self, root: &Path, commit: &str) -> TgResult<()> {
        self.inner.revert(root, commit)
    }

    fn undo_last_commit(&self, root: &Path) -> TgResult<()> {
        self.inner.undo_last_commit(root)
    }

    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()> {
        self.inner.stash_apply(root, index)
    }

    fn is_repo(&self, path: &Path) -> bool {
        self.inner.is_repo(path)
    }
}
