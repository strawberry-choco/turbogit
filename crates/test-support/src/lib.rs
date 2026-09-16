//! Shared headless test doubles (DDD split issue 09).
//!
//! The recording executor below lives in this crate's root module so the
//! integration suites can inject it at the executor boundary. Kept
//! dependency-light on purpose: it needs only the engine port and the domain
//! model, so `turbogit-app`'s tests can use it without ever pulling in egui
//! or the UI crate.
//!
//! The egui shell harness (`harness::`) drives `turbogit_ui::render` through
//! `egui_kittest`. It is gated behind the `harness` feature so consumers
//! that only need the recording executor never compile the egui stack; the
//! UI crate activates it through its own dev-dependencies.

#[cfg(feature = "harness")]
pub mod harness;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{
    BlameLine, Branch, Change, Commit, CommitId, CommitRef, DiffOpts, LogOpts, MergeOpts,
    RebaseOpts, RebasePlanEntry, Remote, RootStatus, Stash, Submodule, TagSpec, Worktree,
};
use turbogit_engine_api::GitExecutor;
// `engine::fake` is unit-test-only (`#[cfg(test)]`), so integration tests
// assert flag selection at the executor boundary through this transparent
// wrapper: every call delegates to a real inner engine while push /
// push-dry-run invocations are recorded verbatim (remote, branch, force).

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCall {
    Push {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
        /// Push tags (`--tags`) flag (issue #24).
        tags: bool,
        /// Skip pre-push hooks (`--no-verify`) flag (issue #24).
        no_verify: bool,
        /// Set upstream tracking ref (`--set-upstream`) flag (issue #24).
        set_upstream: bool,
        /// Oldest selected commit SHA, when the user pushed a subset
        /// (issue #24). The full outgoing list is pushed when this is
        /// `None`.
        selected_oldest: Option<String>,
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
    /// A remote fetch (`root`, remote name or `None` for all remotes)
    /// (issue #27).
    Fetch {
        root: PathBuf,
        remote: Option<String>,
    },
    /// A merge invocation with its full options (issue 28).
    Merge {
        root: PathBuf,
        target: String,
        opts: MergeOpts,
    },
    /// A rebase invocation with its full options (issue 29).
    Rebase {
        root: PathBuf,
        onto: String,
        opts: RebaseOpts,
    },
    /// An interactive plan replay (issue 29).
    RebaseInteractive {
        root: PathBuf,
        plan: Vec<RebasePlanEntry>,
    },
    /// A tag creation with its full spec (issue 31).
    TagCreate {
        root: PathBuf,
        spec: TagSpec,
    },
    /// A tag push (`remote`, tag name or `None` for `--follow-tags`)
    /// (issue 31).
    TagPush {
        root: PathBuf,
        remote: String,
        name: Option<String>,
    },
    /// Add a remote (issue 33).
    AddRemote {
        root: PathBuf,
        name: String,
        url: String,
    },
    /// Set a remote's fetch/push URL (issue 33).
    SetRemoteUrl {
        root: PathBuf,
        name: String,
        fetch_url: Option<String>,
        push_url: Option<String>,
    },
    /// Rename a remote (issue 33).
    RenameRemote {
        root: PathBuf,
        old: String,
        new: String,
    },
    /// Remove a remote (issue 33).
    RemoveRemote {
        root: PathBuf,
        name: String,
    },
    /// Set a branch's upstream tracking (issue 33).
    SetBranchUpstream {
        root: PathBuf,
        branch: String,
        upstream: String,
    },
}

/// Delegating [`GitExecutor`] that records push / dry-run calls.
pub struct RecordingExecutor {
    pub inner: Arc<dyn GitExecutor>,
    calls: Mutex<Vec<RecordedCall>>,
    /// `log` invocation counter (log-open perf: in-flight-guard tests assert
    /// exactly one worker per fetch, without relying on `recorded()` which
    /// other suites compare strictly).
    log_calls: Mutex<usize>,
    /// `ref_decorations` invocation counter (log-open perf: the refs
    /// in-flight guard releases on Ok and Err alike).
    ref_calls: Mutex<usize>,
}

impl RecordingExecutor {
    pub fn new(inner: Arc<dyn GitExecutor>) -> Self {
        Self {
            inner,
            calls: Mutex::new(Vec::new()),
            log_calls: Mutex::new(0),
            ref_calls: Mutex::new(0),
        }
    }

    /// How many `log` invocations have been made so far.
    pub fn log_call_count(&self) -> usize {
        *self.log_calls.lock().expect("log mutex")
    }

    /// How many `ref_decorations` invocations have been made so far.
    pub fn ref_call_count(&self) -> usize {
        *self.ref_calls.lock().expect("ref mutex")
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

    /// True once a `Push` with the given flag subset has been recorded
    /// (issue #24). `selected_oldest` matches when equal to the recorded
    /// value (use `None` to match a full-list push).
    pub fn contains_push_selected(
        &self,
        remote: &str,
        branch: &str,
        force: bool,
        selected_oldest: Option<&str>,
    ) -> bool {
        self.recorded().iter().any(|c| match c {
            RecordedCall::Push {
                remote: r,
                branch: b,
                force: f,
                selected_oldest: sel,
                ..
            } => {
                r == remote
                    && b == branch
                    && *f == force
                    && *sel == selected_oldest.map(|s| s.to_string())
            }
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

    /// Every recorded `fetch` invocation as `(root, remote)` (issue #27).
    pub fn fetches(&self) -> Vec<(PathBuf, Option<String>)> {
        self.recorded()
            .iter()
            .filter_map(|c| match c {
                RecordedCall::Fetch { root, remote } => Some((root.clone(), remote.clone())),
                _ => None,
            })
            .collect()
    }
}

impl GitExecutor for RecordingExecutor {
    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        self.inner.status(root)
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        *self.log_calls.lock().expect("log mutex") += 1;
        self.inner.log(root, opts)
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        self.inner.branches(root)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        self.inner.current_branch(root)
    }

    fn ahead_behind(&self, root: &Path, local: &str, upstream: &str) -> TgResult<(usize, usize)> {
        self.inner.ahead_behind(root, local, upstream)
    }

    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        *self.ref_calls.lock().expect("ref mutex") += 1;
        self.inner.ref_decorations(root)
    }

    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        self.inner.commit_files(root, commit)
    }

    fn push(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
        tags: bool,
        no_verify: bool,
        set_upstream: bool,
        selected_oldest: Option<&str>,
    ) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Push {
                root: root.to_path_buf(),
                remote: remote.to_string(),
                branch: branch.to_string(),
                force,
                tags,
                no_verify,
                set_upstream,
                selected_oldest: selected_oldest.map(|s| s.to_string()),
            });
        self.inner.push(
            root,
            remote,
            branch,
            force,
            tags,
            no_verify,
            set_upstream,
            selected_oldest,
        )
    }

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        self.inner.outgoing_commits(root, branch, upstream)
    }

    fn is_ancestor(&self, root: &Path, upstream: &str, branch: &str) -> TgResult<bool> {
        self.inner.is_ancestor(root, upstream, branch)
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        self.inner.remotes(root)
    }

    fn set_remote_url(
        &self,
        root: &Path,
        name: &str,
        fetch_url: Option<&str>,
        push_url: Option<&str>,
    ) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::SetRemoteUrl {
                root: root.to_path_buf(),
                name: name.to_string(),
                fetch_url: fetch_url.map(str::to_string),
                push_url: push_url.map(str::to_string),
            });
        self.inner.set_remote_url(root, name, fetch_url, push_url)
    }

    fn rename_remote(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::RenameRemote {
                root: root.to_path_buf(),
                old: old.to_string(),
                new: new.to_string(),
            });
        self.inner.rename_remote(root, old, new)
    }

    fn remove_remote(&self, root: &Path, name: &str) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::RemoveRemote {
                root: root.to_path_buf(),
                name: name.to_string(),
            });
        self.inner.remove_remote(root, name)
    }

    fn set_branch_upstream(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::SetBranchUpstream {
                root: root.to_path_buf(),
                branch: branch.to_string(),
                upstream: upstream.to_string(),
            });
        self.inner.set_branch_upstream(root, branch, upstream)
    }

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>> {
        self.inner.stash_list(root)
    }

    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>> {
        self.inner.worktree_list(root)
    }

    fn worktree_dirty(&self, path: &Path) -> TgResult<bool> {
        self.inner.worktree_dirty(path)
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
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::AddRemote {
                root: root.to_path_buf(),
                name: name.to_string(),
                url: url.to_string(),
            });
        self.inner.add_remote(root, name, url)
    }

    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Fetch {
                root: root.to_path_buf(),
                remote: remote.map(str::to_string),
            });
        self.inner.fetch(root, remote)
    }

    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()> {
        self.inner.pull(root, rebase)
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
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Merge {
                root: root.to_path_buf(),
                target: target.to_string(),
                opts: opts.clone(),
            });
        self.inner.merge(root, target, opts)
    }

    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Rebase {
                root: root.to_path_buf(),
                onto: onto.to_string(),
                opts: opts.clone(),
            });
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

    fn merge_auto_merged_files(
        &self,
        root: &Path,
        conflicted: &[PathBuf],
    ) -> TgResult<Vec<PathBuf>> {
        self.inner.merge_auto_merged_files(root, conflicted)
    }

    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::RebaseInteractive {
                root: root.to_path_buf(),
                plan: plan.to_vec(),
            });
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

    fn worktree_add(&self, root: &Path, path: &Path, branch: &str, create: bool) -> TgResult<()> {
        self.inner.worktree_add(root, path, branch, create)
    }

    fn worktree_remove(&self, root: &Path, path: &Path, force: bool) -> TgResult<()> {
        self.inner.worktree_remove(root, path, force)
    }

    fn submodule_status(&self, root: &Path) -> TgResult<Vec<Submodule>> {
        self.inner.submodule_status(root)
    }

    fn submodule_update(&self, root: &Path, path: &Path, init: bool) -> TgResult<()> {
        self.inner.submodule_update(root, path, init)
    }

    fn submodule_deinit(&self, root: &Path, path: &Path, force: bool) -> TgResult<()> {
        self.inner.submodule_deinit(root, path, force)
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

    fn tag_create(&self, root: &Path, spec: &TagSpec) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::TagCreate {
                root: root.to_path_buf(),
                spec: spec.clone(),
            });
        self.inner.tag_create(root, spec)
    }

    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>> {
        self.inner.tag_list(root)
    }

    fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        self.inner.tag_checkout(root, name)
    }

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::TagPush {
                root: root.to_path_buf(),
                remote: remote.to_string(),
                name: name.map(str::to_string),
            });
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

    fn run_raw(&self, root: &Path, args: &[String]) -> TgResult<String> {
        // Transparent wrapper: the merge preview (issue 28) reads its
        // numstat through the raw path, and the wrapped engine must answer.
        self.inner.run_raw(root, args)
    }
}
