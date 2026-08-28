//! The engine port: the only contract between the application and git
//! execution (ADR-0001).
//!
//! [`GitExecutor`] is the single trait that talks to git; adapters live in
//! `turbogit-engine`, consumers (services, app, UI) depend only on this
//! crate. Leaf crate — depends only on `turbogit-domain`.

use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;

/// Direction for [`GitExecutor::apply_patch_to_index`]: stage a patch into
/// the index (forward) or unstage it by reverse-applying against the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyDirection {
    /// Apply the patch as-is (`git apply --cached`).
    Forward,
    /// Reverse-apply the patch (`git apply --cached --reverse`).
    Reverse,
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

    /// Ref decorations per commit: every local branch, remote-tracking branch
    /// and tag with the full SHA it points at (issue #12 ref chips). The
    /// default returns an empty list for engines that cannot answer.
    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        let _ = root;
        Ok(Vec::new())
    }

    /// Files touched by one commit as `(status, path)` pairs
    /// (`git diff-tree --name-status`; issue #12 changed-files pane).
    /// The default returns an empty list for engines that cannot answer.
    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        let _ = (root, commit);
        Ok(Vec::new())
    }

    /// `git branch -vv` (+ remotes) for a root.
    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>>;

    /// Current branch name (or `None` in detached HEAD).
    fn current_branch(&self, root: &Path) -> TgResult<Option<String>>;

    /// Ahead/behind counts of `branch` relative to `upstream`
    /// (e.g. `"origin/main"`), via `git rev-list --left-right --count`.
    /// Returns `(ahead, behind)`.
    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)>;

    /// Commits on local `branch` that are missing from `upstream`
    /// (e.g. `"origin/main"`), newest-first (`git rev-list upstream..branch`).
    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>>;

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

    /// `git push --dry-run`: report what a push would do without mutating the
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

    /// Stage or unstage a patch against the index (`git apply --cached`,
    /// optionally `--reverse` per `direction`). Used for partial (hunk/line)
    /// commits.
    fn apply_patch_to_index(
        &self,
        root: &Path,
        patch: &str,
        direction: ApplyDirection,
    ) -> TgResult<()>;

    /// Mark paths as intent-to-add (`git add -N -- <paths>`), recording them
    /// in the index without their content so partial patches can be applied
    /// to previously untracked files.
    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()>;

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

    /// Raw bytes of a file at a revision (`git show <rev>:<path>`), captured
    /// binary-safe: no UTF-8 conversion is applied, so images and other
    /// non-text blobs survive intact (R8 image/binary diffs).
    fn show_file_bytes(&self, root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>>;

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
