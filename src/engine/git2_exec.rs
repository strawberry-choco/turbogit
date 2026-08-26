//! libgit2-backed [`GitExecutor`] (library-migration plan Phase L2).
//!
//! Composition over the CLI engine: an inner [`CliExecutor`] answers every
//! operation that is not migrated yet, so this backend is a strict subset of
//! the CLI backend's behavior — identical results, identical error shapes for
//! everything below except the overridden methods.
//!
//! Migrated here (real `git2` implementations):
//! - branches: create (+ checkout / start point), checkout, delete
//!   (`-d`/`-D` semantics), rename
//! - tags: create (lightweight + annotated), list
//! - stash: push, pop, drop, apply
//! - staging: add, add-all, unstage (`reset_default` against HEAD)
//! - partial staging: `apply_patch_to_index` in the forward direction via
//!   [`git2::Repository::apply`] + [`git2::ApplyLocation::Index`]
//!
//! Deliberately still on the CLI (see the method comments): reverse patch
//! application and intent-to-add — libgit2 1.9 (as bound by git2 0.21) has no
//! equivalent for either.

use crate::engine::cli::CliExecutor;
use crate::engine::{ApplyDirection, GitExecutor};
use crate::error::{TgError, TgResult};
use crate::model::*;
use git2::{ApplyLocation, BranchType, ErrorCode, IndexAddOption, StashFlags};
use std::path::{Path, PathBuf};

/// Executor that performs supported operations in-process through libgit2
/// and falls back to the system `git` CLI for everything else.
pub struct Git2Executor {
    /// Fallback engine: every non-migrated operation delegates here.
    pub cli: CliExecutor,
}

impl Git2Executor {
    /// Wrap a constructed [`CliExecutor`] as the delegation fallback.
    pub fn new(cli: CliExecutor) -> Self {
        Self { cli }
    }

    /// Open the repository at `root`.
    fn open(&self, root: &Path) -> TgResult<git2::Repository> {
        git2::Repository::open(root).map_err(|_| TgError::NotARepo(root.display().to_string()))
    }
}

/// Map a `git2` error into [`TgError`]. The error type has no libgit2 variant
/// yet (outside this phase's scope), so failures surface as
/// [`TgError::Other`] carrying the full libgit2 message.
fn err(e: git2::Error) -> TgError {
    TgError::Other(format!("libgit2: {e}"))
}

/// Repo-relative, forward-slash path form libgit2 expects for index paths.
fn rel(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Is `tip` fully merged into `base`? Mirrors the reachability check behind
/// `git branch -d`. [`git2::Repository::graph_descendant_of`] answers
/// strictly (equal SHAs are not descendants), so equality counts as merged.
fn merged_into(
    repo: &git2::Repository,
    base: git2::Oid,
    tip: git2::Oid,
) -> Result<bool, git2::Error> {
    Ok(base == tip || repo.graph_descendant_of(base, tip)?)
}

impl GitExecutor for Git2Executor {
    // ---------------------------------------------------------------- read ----
    // Reads stay on the CLI until the read-only migration phases land.

    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        self.cli.status(root)
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        self.cli.log(root, opts)
    }

    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        self.cli.ref_decorations(root)
    }

    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        self.cli.commit_files(root, commit)
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        self.cli.branches(root)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        self.cli.current_branch(root)
    }

    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)> {
        self.cli.ahead_behind(root, branch, upstream)
    }

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        self.cli.outgoing_commits(root, branch, upstream)
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        self.cli.remotes(root)
    }

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>> {
        self.cli.stash_list(root)
    }

    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>> {
        self.cli.worktree_list(root)
    }

    fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>> {
        self.cli.submodule_paths(root)
    }

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        self.cli.config_get(root, key)
    }

    // ---------------------------------------------------------- mutating ----

    fn init(&self, root: &Path) -> TgResult<()> {
        self.cli.init(root)
    }

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()> {
        self.cli.clone(url, dest, depth)
    }

    fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()> {
        self.cli.add_remote(root, name, url)
    }

    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()> {
        self.cli.fetch(root, remote)
    }

    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()> {
        self.cli.pull(root, rebase)
    }

    fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()> {
        self.cli.push(root, remote, branch, force)
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String> {
        self.cli.push_dry_run(root, remote, branch, force)
    }

    fn commit(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        self.cli.commit(root, message, amend)
    }

    fn commit_index(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        self.cli.commit_index(root, message, amend)
    }

    fn merge(&self, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()> {
        self.cli.merge(root, target, opts)
    }

    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()> {
        self.cli.rebase(root, onto, opts)
    }

    fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()> {
        self.cli.cherry_pick(root, commit)
    }

    fn abort(&self, root: &Path, op: &str) -> TgResult<()> {
        self.cli.abort(root, op)
    }

    fn continue_op(&self, root: &Path, op: &str) -> TgResult<()> {
        self.cli.continue_op(root, op)
    }

    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
        self.cli.rebase_interactive(root, plan)
    }

    fn worktree_add(&self, root: &Path, path: &Path, branch: &str) -> TgResult<()> {
        self.cli.worktree_add(root, path, branch)
    }

    // --------------------------------------------------- staging / worktree ----

    fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let repo = self.open(root)?;
        let mut index = repo.index().map_err(err)?;
        for p in paths {
            let spec = rel(p);
            if root.join(p).exists() {
                index.add_path(Path::new(&spec)).map_err(err)?;
            } else {
                // `git add <deleted-path>` records the deletion; add_bypath
                // cannot (the file is gone), so drop the entry instead.
                index.remove_path(Path::new(&spec)).map_err(err)?;
            }
        }
        index.write().map_err(err)?;
        Ok(())
    }

    fn add_all(&self, root: &Path) -> TgResult<()> {
        let repo = self.open(root)?;
        let mut index = repo.index().map_err(err)?;
        // DEFAULT skips ignored files like `git add -A`; deletions are picked
        // up too (the index is updated to match the working tree).
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .map_err(err)?;
        index.write().map_err(err)?;
        Ok(())
    }

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        let repo = self.open(root)?;
        // `restore --staged` resets against HEAD; on an unborn branch both
        // git and libgit2 fail to resolve HEAD, which propagates here.
        let head_commit = repo.head().map_err(err)?.peel_to_commit().map_err(err)?;
        let head = head_commit.as_object();
        let specs: Vec<String> = if paths.is_empty() {
            // CLI maps empty to `restore --staged .` (everything). libgit2's
            // reset_default rejects an empty pathspec (count > 0 assert), so
            // enumerate the staged entries explicitly instead. Index entry
            // paths are already forward-slash separated.
            let index = repo.index().map_err(err)?;
            let specs: Vec<String> = index
                .iter()
                .map(|e| String::from_utf8_lossy(&e.path).into_owned())
                .collect();
            if specs.is_empty() {
                return Ok(());
            }
            specs
        } else {
            paths.iter().map(|p| rel(p)).collect()
        };
        repo.reset_default(Some(head), specs.iter().map(String::as_str))
            .map_err(err)?;
        Ok(())
    }

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.cli.restore(root, paths)
    }

    fn apply_patch_to_index(
        &self,
        root: &Path,
        patch: &str,
        direction: ApplyDirection,
    ) -> TgResult<()> {
        // Forward maps 1:1 onto libgit2's `git apply --cached` emulation:
        // [`ApplyLocation::Index`] applies to the index only and commits the
        // index write itself (git_apply uses an index writer internally).
        //
        // Reverse cannot migrate yet: libgit2 1.9 (bound by git2 0.21) has no
        // reverse flag on `git_apply` — its only option flag is
        // GIT_APPLY_CHECK — so the exact
        // `git apply --cached --reverse --recount` behavior stays on the CLI.
        if direction == ApplyDirection::Reverse {
            return self.cli.apply_patch_to_index(root, patch, direction);
        }
        if patch.trim().is_empty() {
            // `git apply` succeeds silently on empty input.
            return Ok(());
        }
        let repo = self.open(root)?;
        let diff = git2::Diff::from_buffer(patch.as_bytes()).map_err(err)?;
        repo.apply(&diff, ApplyLocation::Index, None).map_err(err)?;
        Ok(())
    }

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        // Delegated to the CLI: libgit2 1.9 cannot *create* intent-to-add
        // entries. `IndexEntryExtendedFlag::INTENT_TO_ADD` exists only for
        // reading such entries; no `git_index_add*` API produces them, and a
        // hand-built zero-OID entry would corrupt later index operations.
        // Keep the exact `git add -N` behavior on the CLI.
        self.cli.add_intent_to_add(root, paths)
    }

    // ----------------------------------------------------------- branches ----

    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()> {
        let repo = self.open(root)?;
        let obj = repo
            .revparse_single(start_point.unwrap_or("HEAD"))
            .map_err(err)?;
        let commit = obj.peel_to_commit().map_err(err)?;
        // force=false fails on an existing name, matching plain `git branch`.
        let branch = repo.branch(name, &commit, false).map_err(err)?;
        if checkout {
            // Tree first, HEAD second — the standard libgit2 switch order.
            // SAFE strategy carries uncommitted changes over or refuses,
            // like `git checkout -b`.
            repo.checkout_tree(commit.as_object(), None).map_err(err)?;
            let refname = branch.get().name().map_err(err)?.to_string();
            repo.set_head(&refname).map_err(err)?;
        }
        Ok(())
    }

    fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        let repo = self.open(root)?;
        match repo.find_branch(name, BranchType::Local) {
            Ok(branch) => {
                let commit = branch.get().peel_to_commit().map_err(err)?;
                repo.checkout_tree(commit.as_object(), None).map_err(err)?;
                let refname = branch.get().name().map_err(err)?.to_string();
                repo.set_head(&refname).map_err(err)?;
                Ok(())
            }
            // `git switch` also accepts remote-tracking refs (DWIM) and other
            // commit-ish spellings; keep those rare inputs on the CLI
            // verbatim instead of approximating them here.
            Err(_) => self.cli.branch_checkout(root, name),
        }
    }

    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()> {
        let repo = self.open(root)?;
        let mut branch = repo.find_branch(name, BranchType::Local).map_err(err)?;
        if !force {
            // Mirror `git branch -d`: refuse unless the tip is reachable from
            // the upstream when one is configured, else from HEAD. libgit2's
            // own delete has no unmerged check, so it is done here.
            let tip = branch.get().peel_to_commit().map_err(err)?.id();
            let head_tip = repo
                .head()
                .ok()
                .and_then(|h| h.peel_to_commit().ok())
                .map(|c| c.id());
            let merged = match branch.upstream().ok() {
                Some(upstream) => {
                    let base = upstream.get().peel_to_commit().map_err(err)?.id();
                    merged_into(&repo, base, tip)
                }
                None => match head_tip {
                    Some(head) => merged_into(&repo, head, tip),
                    None => Ok(false),
                },
            }
            .map_err(err)?;
            if !merged {
                return Err(TgError::Other(format!(
                    "the branch '{name}' is not fully merged;\nuse force delete to discard its commits"
                )));
            }
        }
        // Refuses the checked-out branch, matching the CLI failure mode.
        branch.delete().map_err(err)?;
        Ok(())
    }

    fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()> {
        self.cli.branch_delete_remote(root, remote, name)
    }

    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        let repo = self.open(root)?;
        let mut branch = repo.find_branch(old, BranchType::Local).map_err(err)?;
        // force=false matches `git branch -m` (non-forced rename).
        branch.rename(new, false).map_err(err)?;
        Ok(())
    }

    // --------------------------------------------------------------- tags ----

    fn tag_create(&self, root: &Path, name: &str, message: Option<&str>) -> TgResult<()> {
        let repo = self.open(root)?;
        let obj = repo.revparse_single("HEAD").map_err(err)?;
        match message {
            Some(m) => {
                // Same identity source as `git tag -a`: user.name/email from
                // config; unset identity fails exactly like the CLI does.
                let sig = repo.signature().map_err(err)?;
                repo.tag_annotation_create(name, &obj, &sig, m)
                    .map_err(err)?;
            }
            None => {
                repo.tag_lightweight(name, &obj, false).map_err(err)?;
            }
        }
        Ok(())
    }

    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>> {
        let repo = self.open(root)?;
        let names = repo.tag_names(None).map_err(err)?;
        Ok(names
            .iter_bytes()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .filter(|n| !n.is_empty())
            .collect())
    }

    fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        self.cli.tag_checkout(root, name)
    }

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()> {
        self.cli.tag_push(root, remote, name, all)
    }

    // ------------------------------------------------------- diff / blame ----

    fn diff(&self, root: &Path, opts: &DiffOpts) -> TgResult<String> {
        self.cli.diff(root, opts)
    }

    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        self.cli.blame(root, path, rev)
    }

    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        self.cli.show_file(root, rev, path)
    }

    fn show_file_bytes(&self, root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>> {
        self.cli.show_file_bytes(root, rev, path)
    }

    // ------------------------------------------------------ revert / undo ----

    fn revert(&self, root: &Path, commit: &str) -> TgResult<()> {
        self.cli.revert(root, commit)
    }

    fn undo_last_commit(&self, root: &Path) -> TgResult<()> {
        self.cli.undo_last_commit(root)
    }

    fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
        let mut repo = self.open(root)?;
        // Same identity source as `git stash push`: config user.name/email.
        let sig = repo.signature().map_err(err)?;
        let mut flags = StashFlags::empty();
        if keep_index {
            flags.insert(StashFlags::KEEP_INDEX);
        }
        match repo.stash_save2(&sig, Some(message), Some(flags)) {
            Ok(_) => Ok(()),
            // `git stash push` exits 0 with "No local changes to save";
            // libgit2 reports nothing-to-stash as NotFound. Map it to
            // success so callers see identical behavior.
            Err(e) if e.code() == ErrorCode::NotFound => Ok(()),
            Err(e) => Err(err(e)),
        }
    }

    fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()> {
        let mut repo = self.open(root)?;
        repo.stash_pop(index, None).map_err(err)?;
        Ok(())
    }

    fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()> {
        let mut repo = self.open(root)?;
        repo.stash_drop(index).map_err(err)?;
        Ok(())
    }

    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()> {
        let mut repo = self.open(root)?;
        // Default options do not reinstate the index, matching
        // `git stash apply` without `--index`.
        repo.stash_apply(index, None).map_err(err)?;
        Ok(())
    }
}
