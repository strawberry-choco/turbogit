use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;

pub mod patch;
pub mod repo_state;

pub use repo_state::OwnedOperation;

/// Direction for [`GitExecutor::apply_patch_to_index`]: stage a patch into
/// the index (forward) or unstage it by reverse-applying against the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyDirection {
    Forward,
    Reverse,
}

pub trait GitExecutor: Send + Sync {
    fn status(&self, root: &Path) -> TgResult<RootStatus>;

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>>;

    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        let _ = root;
        Ok(Vec::new())
    }

    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        let _ = (root, commit);
        Ok(Vec::new())
    }

    /// "N files · +X −Y". Paths are exactly the ones [`GitExecutor::commit_files`]
    /// reports, rename targets included, so the changed-files pane can join
    /// the two lists by path. A **Binary change** carries `None` in both count
    /// columns; the value's own totals are where that becomes a number. Rename
    /// detection follows git's own defaults, unpinned.
    fn change_stats(&self, root: &Path, question: &ChangeQuestion) -> TgResult<ChangeStats>;

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>>;

    /// Branches HEAD's reflog says were checked out in `root`, newest first,
    /// with no reflog yet answers empty, absence is a normal answer, not an
    /// error.
    fn recent_checkouts(&self, root: &Path) -> TgResult<Vec<RecentCheckout>>;

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>>;

    /// Returns `(ahead, behind)`.
    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)>;

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>>;

    fn is_ancestor(&self, root: &Path, upstream: &str, branch: &str) -> TgResult<bool>;

    fn commit_count_between(&self, root: &Path, from: &str, to: &str) -> TgResult<usize>;

    fn resolve_revision(&self, root: &Path, name: &str) -> TgResult<CommitId>;

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>>;

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>>;

    /// `git worktree list` (excluding the main worktree). Cheap: answers
    /// path + checked-out branch without any working-tree scan; the returned
    /// entries carry an unset dirty flag. Probe one worktree's dirtiness on
    /// demand via [`GitExecutor::worktree_dirty`].
    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>>;

    /// Dirty probe for one linked worktree: does `path`, a linked worktree's
    /// directory, as listed by [`GitExecutor::worktree_list`], hold any tracked
    /// change, any non-ignored untracked file, or any conflict? Strictly
    /// cheaper than a full `git status`: a tracked diff that stops at the first
    /// whole-working-tree porcelain scan. A prunable (missing) work tree
    /// answers clean.
    fn worktree_dirty(&self, path: &Path) -> TgResult<bool>;

    /// One owner, ranked: a sequence stopped on a pick answers `Rebase`, and a
    /// pick resolved by the merge machinery answers `CherryPick`, so a refusal
    /// and an announcement cannot blame different operations for one repository.
    /// Not a `TgResult`: a directory that cannot be read answers `None`, the same
    /// answer an unheld repository gives, because a reader that could not tell
    /// must not claim a holder.
    fn holding_operation(&self, root: &Path) -> Option<OwnedOperation>;

    fn submodule_status(&self, root: &Path) -> TgResult<Vec<Submodule>>;

    /// resolver shows. A side a merge left no version of (an add/add's base, a
    /// one-sided delete's side) answers `None`, which is not an empty side.
    /// Errors when `path` holds no conflict at all.
    fn conflict_versions(&self, root: &Path, path: &Path) -> TgResult<ConflictVersions>;

    fn branch_upstream(&self, root: &Path, branch: &str) -> TgResult<Option<Upstream>>;

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>>;

    fn init(&self, root: &Path) -> TgResult<()>;

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()>;

    fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()>;

    /// Every provided side is updated; at least one must be `Some`.
    fn set_remote_url(
        &self,
        root: &Path,
        name: &str,
        fetch_url: Option<&str>,
        push_url: Option<&str>,
    ) -> TgResult<()>;

    fn rename_remote(&self, root: &Path, old: &str, new: &str) -> TgResult<()>;

    fn remove_remote(&self, root: &Path, name: &str) -> TgResult<()>;

    fn set_branch_upstream(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<()>;

    /// The branch argument is not optional here even though git accepts one:
    /// `--unset-upstream` without it silently targets the **current** branch,
    /// so a rename-then-unset on a non-checked-out branch would clear the
    /// tracking of whatever branch the developer happens to be standing on.
    fn unset_branch_upstream(&self, root: &Path, branch: &str) -> TgResult<()>;

    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()>;

    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()>;

    #[allow(clippy::too_many_arguments)]
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
    ) -> TgResult<()>;
    /// remote. Returns the verbatim git report (captured from stderr) on
    /// success; a rejected push (e.g. non-fast-forward) surfaces as
    /// [`turbogit_domain::error::TgError::Cli`] carrying the verbatim stderr.
    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String>;

    fn commit(
        &self,
        root: &Path,
        message: &str,
        amend: bool,
        no_verify: bool,
    ) -> TgResult<CommitId>;

    /// Commit only what is already staged in the index (no `-a`). Used for
    /// partial (selected-files / selected-hunks) commits.
    fn commit_index(
        &self,
        root: &Path,
        message: &str,
        amend: bool,
        no_verify: bool,
    ) -> TgResult<CommitId>;

    fn merge(&self, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()>;

    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()>;

    fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()>;

    fn abort(&self, root: &Path, op: OwnedOperation) -> TgResult<()>;

    fn continue_op(&self, root: &Path, op: OwnedOperation) -> TgResult<()>;

    /// `git rebase --skip`: abandon the commit a paused rebase stopped on and
    /// replay the rest. Rebase-only by construction, so it is its own method
    fn rebase_skip(&self, root: &Path) -> TgResult<()>;

    fn rewrite_backup_ref(&self) -> &str;

    fn save_rewrite_backup(&self, root: &Path) -> TgResult<()>;

    /// no backup was saved, restoring nothing must not read as a success.
    fn restore_rewrite_backup(&self, root: &Path) -> TgResult<()>;

    fn discard_rewrite_backup(&self, root: &Path) -> TgResult<()>;

    /// `conflicted`. Returns an empty list when no merge is in progress.
    fn merge_auto_merged_files(
        &self,
        root: &Path,
        conflicted: &[PathBuf],
    ) -> TgResult<Vec<PathBuf>>;

    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()>;

    fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()>;

    fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()>;

    fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()>;

    /// worktree (`-b`); otherwise `branch` must already exist.
    fn worktree_add(&self, root: &Path, path: &Path, branch: &str, create: bool) -> TgResult<()>;

    fn worktree_remove(&self, root: &Path, path: &Path, force: bool) -> TgResult<()>;

    fn submodule_update(&self, root: &Path, path: &Path, init: bool) -> TgResult<()>;

    fn submodule_deinit(&self, root: &Path, path: &Path, force: bool) -> TgResult<()>;

    fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    fn apply_patch_to_index(
        &self,
        root: &Path,
        patch: &Patch,
        direction: ApplyDirection,
    ) -> TgResult<()>;

    fn apply_patch_to_worktree(&self, root: &Path, patch: &Patch) -> TgResult<()>;

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    /// Would `patch` apply cleanly? (`git apply --check`.) Mutates nothing. A
    /// patch that does not apply surfaces as
    /// [`turbogit_domain::error::TgError::Cli`] carrying the verbatim stderr.
    /// The default rejects, engines that cannot run git (the git2 adapter)
    /// cannot answer.
    fn check_patch(&self, root: &Path, patch: &Patch) -> TgResult<()> {
        let _ = (root, patch);
        Err(turbogit_domain::error::TgError::Other(
            "patch checks are not supported by this git engine".into(),
        ))
    }

    /// List `dir` in `info/exclude`, the ignore file a clone keeps for itself:
    /// the one mechanism that is local to this checkout, never committed, and
    /// never a tracked file. This is not `.gitignore` editing and must not
    /// become it, a tracked ignore file is a rule imposed on everyone who
    /// clones, which is not this app's to write.
    ///
    /// - The file read is the **common** directory's. A linked worktree has its
    ///   own `$GIT_DIR` (`.git/worktrees/<name>`), and git never reads an
    ///   `info/exclude` there, measured against git 2.54, where a pattern
    ///   written in a worktree's own gitdir leaves the directory listed as
    ///   untracked. So the write goes to `--git-common-dir`, and the one entry
    ///   then covers every work tree of the clone.
    fn exclude_dir(&self, root: &Path, dir: &Path) -> TgResult<()>;

    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()>;

    fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()>;

    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()>;

    fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()>;

    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()>;

    fn tag_create(&self, root: &Path, spec: &TagSpec) -> TgResult<()>;

    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>>;

    fn checkout_detached(&self, root: &Path, name: &str) -> TgResult<()>;

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()>;

    fn diff_patch(&self, root: &Path, opts: &DiffOpts) -> TgResult<Patch>;

    /// Bytes and not a `String`, deliberately and for two reasons that are one
    /// fact: a patch's body is its file's content, which git does not guarantee
    /// is valid UTF-8, and `Patch File Settings`' encoding field is a statement
    /// about the bytes on disk, a value already decoded to `String` could no
    /// longer be written verbatim, which is the default this surface ships. Path
    fn patch_bytes(&self, root: &Path, origin: &PatchOrigin, reverse: bool) -> TgResult<Vec<u8>>;

    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>>;

    /// Errors when the index holds no such path; never answers empty.
    fn index_file_bytes(&self, root: &Path, path: &Path) -> TgResult<Vec<u8>>;

    /// Content of a file **at a commit**, `rev` names the commit whose version
    /// of `path` is wanted, and is not a place an index stage may be passed.
    /// A conflicted path's sides come from [`GitExecutor::conflict_versions`]
    /// and its staged bytes from [`GitExecutor::index_file_bytes`].
    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String>;

    fn show_file_bytes(&self, root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>>;

    fn revert(&self, root: &Path, commit: &str) -> TgResult<()>;

    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()>;

    /// Run arbitrary git in `root` (`git <args>` verbatim, no shell): the
    /// port's **one** raw escape, and the caller that owns it is the bulk
    /// operations grid's user-typed custom command
    /// Returns stdout on success; a non-zero exit surfaces as
    /// [`turbogit_domain::error::TgError::Cli`] with the verbatim stderr, which
    fn run_raw(&self, root: &Path, args: &[String]) -> TgResult<String> {
        let _ = root;
        Err(turbogit_domain::error::TgError::Other(format!(
            "custom commands are not supported by this git engine (asked to run {args:?})"
        )))
    }

    fn is_repo(&self, path: &Path) -> bool {
        self.current_branch(path).is_ok()
    }
}
