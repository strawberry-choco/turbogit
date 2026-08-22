//! Git engine layer.
//!
//! The `GitExecutor` trait is the **only** thing that talks to git. The
//! primary implementation, [`cli::CliExecutor`], shells out to the system `git`
//! binary. An optional [`gix_reader`] (feature `gix-reader`) implements the
//! read-only methods in-process. All mutating ops always go through the CLI.
//!
//! See `product-spec.md` §10 and `execution-plan.md` §3.

pub mod cli;
#[cfg(test)]
pub mod fake;
#[cfg(feature = "gix-reader")]
pub mod gix_reader;

use crate::error::TgResult;
use crate::model::*;
use std::path::{Path, PathBuf};

/// Events posted from worker threads back to the UI thread over a channel.
///
/// The app drains these in `update()` and calls `ctx.request_repaint()`.
#[derive(Debug)]
pub enum AppEvent {
    /// A status scan for one root completed (or failed).
    StatusScanned {
        root: RootId,
        status: TgResult<RootStatus>,
    },
    /// Roots were (re)discovered.
    RootsDetected(Vec<RootId>),
    /// Branches for a root were loaded.
    BranchesLoaded {
        root: RootId,
        branches: TgResult<Vec<Branch>>,
    },
    /// Log for a root was loaded.
    LogLoaded {
        root: RootId,
        commits: TgResult<Vec<Commit>>,
    },
    /// Generic asynchronous completion (e.g. push/pull finished).
    OpCompleted {
        label: String,
        result: TgResult<()>,
    },
    /// Fatal / unexpected error to surface in the UI.
    Error(String),
    /// App is ready (roots initialized, first scan dispatched).
    Ready,
    /// An asynchronously-computed diff is ready (keyed to avoid races).
    DiffReady {
        key: String,
        result: TgResult<String>,
    },
    /// Ahead/behind counts for a root's current branch were computed.
    AheadBehind {
        root: RootId,
        ahead: usize,
        behind: usize,
    },
}

/// The git engine abstraction. All methods are synchronous; callers run them on
/// worker threads (the engine never blocks the UI thread).
///
/// `Send + Sync` so an `Arc<dyn GitExecutor>` can be shared with worker threads.
pub trait GitExecutor: Send + Sync {
    // ---- read ----
    /// `git status` for a root (porcelain v2 parsed into `RootStatus`).
    fn status(&self, root: &Path) -> TgResult<RootStatus>;

    /// `git log` for a root.
    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>>;

    /// `git branch -vv` (+ remotes) for a root.
    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>>;

    /// Current branch name (or `None` in detached HEAD).
    fn current_branch(&self, root: &Path) -> TgResult<Option<String>>;

    /// Ahead/behind counts of `branch` relative to `upstream`
    /// (e.g. `"origin/main"`), via `git rev-list --left-right --count`.
    /// Returns `(ahead, behind)`.
    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)>;

    /// Configured remotes.
    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>>;

    /// `git stash list`.
    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>>;

    /// `git worktree list` (excluding the main worktree).
    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>>;

    /// Submodule paths registered at this root.
    fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>>;

    /// Read a git config value (e.g. `user.name`, `core.autocrlf`).
    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>>;

    // ---- mutating (CLI only) ----
    /// `git init` (or `git init --initial-branch`).
    fn init(&self, root: &Path) -> TgResult<()>;

    /// `git clone` (optionally shallow with `--depth`).
    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()>;

    /// Add a remote (`git remote add`).
    fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()>;

    /// `git fetch` (one remote or all).
    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()>;

    /// `git pull` (merge or rebase).
    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()>;

    /// `git push` (optionally `--force-with-lease`).
    fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()>;

    /// `git commit` (optionally `--amend`).
    fn commit(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId>;

    /// Commit only what is already staged in the index (no `-a`). Used for
    /// partial (selected-files / selected-hunks) commits.
    fn commit_index(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId>;

    /// `git merge`.
    fn merge(&self, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()>;

    /// `git rebase`.
    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()>;

    /// `git cherry-pick`.
    fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()>;

    /// Abort an in-progress `merge` / `rebase` / `cherry-pick`
    /// (`git <op> --abort`).
    fn abort(&self, root: &Path, op: &str) -> TgResult<()>;

    /// Continue an in-progress `merge` / `rebase` / `cherry-pick`
    /// (`git <op> --continue`).
    fn continue_op(&self, root: &Path, op: &str) -> TgResult<()>;

    /// Run an interactive rebase from a pre-built plan (`git rebase -i` with a
    /// synthetic sequence editor that materializes `plan` as the todo list).
    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()>;

    /// `git stash push` (optionally keep index).
    fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()>;

    /// `git stash pop`.
    fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()>;

    /// `git stash drop`.
    fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()>;

    /// `git worktree add`.
    fn worktree_add(&self, root: &Path, path: &Path, branch: &str) -> TgResult<()>;

    // ---- staging / working tree ----
    /// Stage specific paths (`git add <paths>`).
    fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    /// Stage everything (`git add -A`).
    fn add_all(&self, root: &Path) -> TgResult<()>;

    /// Unstage paths (`git restore --staged <paths>`).
    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    /// Discard working-tree changes to paths (`git checkout -- <paths>`).
    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

    /// Stage a patch into the index (`git apply --cached`). Used for partial
    /// (hunk/line) commits.
    fn apply_patch_to_index(&self, root: &Path, patch: &str) -> TgResult<()>;

    // ---- branches ----
    /// Create a branch (`git branch [<start>]`), optionally checking it out.
    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()>;

    /// Check out a branch or ref (`git switch <name>` / `git checkout <ref>`).
    fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()>;

    /// Delete a local branch (`git branch -d` / `-D`).
    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()>;

    /// Delete a remote branch (`git push <remote> --delete <name>`).
    fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()>;

    /// Rename a branch (`git branch -m <old> <new>`).
    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()>;

    // ---- tags ----
    /// Create a tag (`git tag [-a -m] <name>`).
    fn tag_create(&self, root: &Path, name: &str, message: Option<&str>) -> TgResult<()>;

    /// List tags (`git tag -l`).
    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>>;

    /// Check out a tag (detached HEAD).
    fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()>;

    /// Push tags (`git push [--tags | <name>]`).
    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()>;

    // ---- diff / blame / history ----
    /// Produce a diff (`git diff` with the given options).
    fn diff(&self, root: &Path, opts: &DiffOpts) -> TgResult<String>;

    /// `git blame` for a path (optionally at a revision).
    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>>;

    /// Content of a file at a revision (`git show <rev>:<path>`).
    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String>;

    // ---- revert / undo ----
    /// `git revert <commit>` (inverse commit).
    fn revert(&self, root: &Path, commit: &str) -> TgResult<()>;

    /// Undo the most recent commit, keeping the working tree
    /// (`git reset --soft HEAD~1`).
    fn undo_last_commit(&self, root: &Path) -> TgResult<()>;

    /// `git stash apply <index>` (keeps the stash).
    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()>;

    // ---- convenience helpers built on the above ----
    /// Is `path` inside a git work tree? (used by auto-detect / scanner).
    fn is_repo(&self, path: &Path) -> bool {
        self.current_branch(path).is_ok()
    }
}
