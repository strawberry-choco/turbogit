#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "harness")]
pub mod wcag;

pub mod git_seed;

pub mod srcscan;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{
    BlameLine, Branch, Change, ChangeQuestion, ChangeStats, Commit, CommitId, CommitRef,
    ConflictVersions, DiffOpts, LogOpts, MergeOpts, Patch, PatchOrigin, RebaseOpts,
    RebasePlanEntry, RecentCheckout, Remote, RootStatus, Stash, Submodule, TagSpec, Upstream,
    Worktree,
};
use turbogit_engine_api::{GitExecutor, OwnedOperation};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCall {
    Push {
        root: PathBuf,
        remote: String,
        branch: String,
        force: bool,
        tags: bool,
        no_verify: bool,
        set_upstream: bool,
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
    ApplyPatchToWorktree,
    AddIntentToAdd(Vec<PathBuf>),
    Add(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    CommitAll,
    CommitIndex,
    Fetch {
        root: PathBuf,
        remote: Option<String>,
    },
    Merge {
        root: PathBuf,
        target: String,
        opts: MergeOpts,
    },
    Rebase {
        root: PathBuf,
        onto: String,
        opts: RebaseOpts,
    },
    RebaseInteractive {
        root: PathBuf,
        plan: Vec<RebasePlanEntry>,
    },
    TagCreate {
        root: PathBuf,
        spec: TagSpec,
    },
    TagPush {
        root: PathBuf,
        remote: String,
        name: Option<String>,
    },
    AddRemote {
        root: PathBuf,
        name: String,
        url: String,
    },
    SetRemoteUrl {
        root: PathBuf,
        name: String,
        fetch_url: Option<String>,
        push_url: Option<String>,
    },
    RenameRemote {
        root: PathBuf,
        old: String,
        new: String,
    },
    RemoveRemote {
        root: PathBuf,
        name: String,
    },
    SetBranchUpstream {
        root: PathBuf,
        branch: String,
        upstream: String,
    },
    UnsetBranchUpstream {
        root: PathBuf,
        branch: String,
    },
}

pub struct RecordingExecutor {
    pub inner: Arc<dyn GitExecutor>,
    calls: Mutex<Vec<RecordedCall>>,
    ref_calls: Mutex<usize>,
    stats_calls: Mutex<usize>,
    log_opts: Mutex<Vec<LogOpts>>,
    patch_reads: Mutex<Vec<(PatchOrigin, bool)>>,
}

impl RecordingExecutor {
    pub fn new(inner: Arc<dyn GitExecutor>) -> Self {
        Self {
            inner,
            calls: Mutex::new(Vec::new()),
            ref_calls: Mutex::new(0),
            stats_calls: Mutex::new(0),
            log_opts: Mutex::new(Vec::new()),
            patch_reads: Mutex::new(Vec::new()),
        }
    }

    pub fn log_call_count(&self) -> usize {
        self.log_opts.lock().expect("log opts mutex").len()
    }

    pub fn log_opts(&self) -> Vec<LogOpts> {
        self.log_opts.lock().expect("log opts mutex").clone()
    }

    pub fn patch_reads(&self) -> Vec<(PatchOrigin, bool)> {
        self.patch_reads.lock().expect("patch reads mutex").clone()
    }

    pub fn ref_call_count(&self) -> usize {
        *self.ref_calls.lock().expect("ref mutex")
    }

    pub fn stats_call_count(&self) -> usize {
        *self.stats_calls.lock().expect("stats mutex")
    }

    pub fn recorded(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("calls mutex").clone()
    }

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
        self.log_opts
            .lock()
            .expect("log opts mutex")
            .push(opts.clone());
        self.inner.log(root, opts)
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        self.inner.branches(root)
    }

    fn recent_checkouts(&self, root: &Path) -> TgResult<Vec<RecentCheckout>> {
        self.inner.recent_checkouts(root)
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

    fn change_stats(&self, root: &Path, question: &ChangeQuestion) -> TgResult<ChangeStats> {
        *self.stats_calls.lock().expect("stats mutex") += 1;
        self.inner.change_stats(root, question)
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

    fn commit_count_between(&self, root: &Path, from: &str, to: &str) -> TgResult<usize> {
        self.inner.commit_count_between(root, from, to)
    }

    fn resolve_revision(&self, root: &Path, name: &str) -> TgResult<CommitId> {
        self.inner.resolve_revision(root, name)
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

    fn unset_branch_upstream(&self, root: &Path, branch: &str) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::UnsetBranchUpstream {
                root: root.to_path_buf(),
                branch: branch.to_string(),
            });
        self.inner.unset_branch_upstream(root, branch)
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

    fn holding_operation(&self, root: &Path) -> Option<OwnedOperation> {
        self.inner.holding_operation(root)
    }

    fn conflict_versions(&self, root: &Path, path: &Path) -> TgResult<ConflictVersions> {
        self.inner.conflict_versions(root, path)
    }

    fn branch_upstream(&self, root: &Path, branch: &str) -> TgResult<Option<Upstream>> {
        self.inner.branch_upstream(root, branch)
    }

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        self.inner.config_get(root, key)
    }

    fn init(&self, root: &Path) -> TgResult<()> {
        self.inner.init(root)
    }

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()> {
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

    fn commit(
        &self,
        root: &Path,
        message: &str,
        amend: bool,
        no_verify: bool,
    ) -> TgResult<CommitId> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::CommitAll);
        self.inner.commit(root, message, amend, no_verify)
    }

    fn commit_index(
        &self,
        root: &Path,
        message: &str,
        amend: bool,
        no_verify: bool,
    ) -> TgResult<CommitId> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::CommitIndex);
        self.inner.commit_index(root, message, amend, no_verify)
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

    fn abort(&self, root: &Path, op: OwnedOperation) -> TgResult<()> {
        self.inner.abort(root, op)
    }

    fn continue_op(&self, root: &Path, op: OwnedOperation) -> TgResult<()> {
        self.inner.continue_op(root, op)
    }

    fn rebase_skip(&self, root: &Path) -> TgResult<()> {
        self.inner.rebase_skip(root)
    }

    fn rewrite_backup_ref(&self) -> &str {
        self.inner.rewrite_backup_ref()
    }

    fn save_rewrite_backup(&self, root: &Path) -> TgResult<()> {
        self.inner.save_rewrite_backup(root)
    }

    fn restore_rewrite_backup(&self, root: &Path) -> TgResult<()> {
        self.inner.restore_rewrite_backup(root)
    }

    fn discard_rewrite_backup(&self, root: &Path) -> TgResult<()> {
        self.inner.discard_rewrite_backup(root)
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

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::Unstage(paths.to_vec()));
        self.inner.unstage(root, paths)
    }

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.inner.restore(root, paths)
    }

    fn apply_patch_to_index(
        &self,
        _root: &Path,
        _patch: &Patch,
        direction: turbogit_engine_api::ApplyDirection,
    ) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::ApplyPatch { direction });
        self.inner.apply_patch_to_index(_root, _patch, direction)
    }

    fn apply_patch_to_worktree(&self, root: &Path, patch: &Patch) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::ApplyPatchToWorktree);
        self.inner.apply_patch_to_worktree(root, patch)
    }

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.calls
            .lock()
            .expect("calls mutex")
            .push(RecordedCall::AddIntentToAdd(paths.to_vec()));
        self.inner.add_intent_to_add(root, paths)
    }

    fn check_patch(&self, root: &Path, patch: &Patch) -> TgResult<()> {
        self.inner.check_patch(root, patch)
    }

    fn exclude_dir(&self, root: &Path, dir: &Path) -> TgResult<()> {
        self.inner.exclude_dir(root, dir)
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

    fn checkout_detached(&self, root: &Path, name: &str) -> TgResult<()> {
        self.inner.checkout_detached(root, name)
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

    fn diff_patch(&self, root: &Path, opts: &DiffOpts) -> TgResult<Patch> {
        self.inner.diff_patch(root, opts)
    }

    fn patch_bytes(&self, root: &Path, origin: &PatchOrigin, reverse: bool) -> TgResult<Vec<u8>> {
        self.patch_reads
            .lock()
            .expect("patch reads mutex")
            .push((origin.clone(), reverse));
        self.inner.patch_bytes(root, origin, reverse)
    }

    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        self.inner.blame(root, path, rev)
    }

    fn index_file_bytes(&self, root: &Path, path: &Path) -> TgResult<Vec<u8>> {
        self.inner.index_file_bytes(root, path)
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

    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()> {
        self.inner.stash_apply(root, index)
    }

    fn is_repo(&self, path: &Path) -> bool {
        self.inner.is_repo(path)
    }

    fn run_raw(&self, root: &Path, args: &[String]) -> TgResult<String> {
        self.inner.run_raw(root, args)
    }
}

/// A throwaway fixture root under the system temp dir, keyed by this test
/// binary's path: two checkouts' runs can never share fixture paths (a
/// `config.lock` race, not a test), and a rebuilt binary simply orphans the
/// old root for the OS temp cleaner to reap. The caller owns creation and
/// cleanup, as with [`git_seed`].
pub fn scratch_root(leaf: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    std::env::current_exe()
        .expect("test binary path")
        .hash(&mut hasher);
    std::env::temp_dir().join(format!("turbogit-uitests-{:016x}/{leaf}", hasher.finish()))
}

/// The predicate is checked **before** the deadline, so a check landing on the
/// final instant still counts. That ordering is what lets a spawned git worker
/// that finishes just inside its budget pass; swapping the two checks is the
/// busy-machine flake class AGENTS.md records.
///
/// Returns `false` on timeout rather than panicking: what a timeout means
/// differs per suite, so that wording is the caller's.
pub fn wait_until<F: FnMut() -> bool>(ms: u64, mut f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
