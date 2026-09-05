//! Integration service: high-level merge / rebase / cherry-pick orchestration
//! plus abort/continue and a "smart merge" that stashes a dirty tree first.

#![allow(dead_code)]

use crate::sync_service::is_protected;
use std::path::Path;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// The pre-merge preview (issue 28, screen 14's preview box): plain data
/// computed by [`merge_preview`] before anything runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MergePreview {
    /// Merge commits the strategy will create (0 for Fast-forward or an
    /// already-merged target).
    pub merge_commits: usize,
    /// Files the merge would change (git's three-dot diff).
    pub files: usize,
    /// Line insertions across those files.
    pub insertions: usize,
    /// Line deletions across those files.
    pub deletions: usize,
}

/// Merge `target` into the current branch at `root`.
pub fn merge(vcs: &dyn GitExecutor, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()> {
    vcs.merge(root, target, opts)
}

/// Map the dialog's STRATEGY selection plus the option rows onto the
/// executor-facing flags (issue 28, screen 14). The strategy is the
/// segmented control: it decides which mutually exclusive merge mode runs
/// (`--no-commit` / plain / `--squash` / `--ff-only`); the options layer on
/// top of it. `no_ff` is meaningless under Fast-forward, so it is dropped —
/// a strategy always wins over a leftover option toggle.
pub fn merge_flags(
    strategy: MergeStrategy,
    no_ff: bool,
    verify_signatures: bool,
    allow_unrelated: bool,
) -> MergeOpts {
    let mut opts = MergeOpts {
        no_ff,
        verify_signatures,
        allow_unrelated,
        ..Default::default()
    };
    match strategy {
        // --no-commit alone cannot stop a fast-forward (git would still move
        // the branch), so the strategy always carries --no-ff with it.
        MergeStrategy::NoCommit => {
            opts.no_commit = true;
            opts.no_ff = true;
        }
        MergeStrategy::Commit => {}
        MergeStrategy::Squash => opts.squash = true,
        MergeStrategy::FastForward => {
            opts.ff_only = true;
            opts.no_ff = false;
        }
    }
    opts
}

/// What a merge will do, computed before it runs (issue 28, screen 14's
/// preview box): the merge-commit count for the strategy and the
/// file/insertion/deletion totals of what merging `target` brings in.
///
/// The totals are git's own three-dot diff (`diff --numstat
/// HEAD...<target>`): merge-base to `target`, exactly the changes a merge
/// would apply. A target already merged into HEAD reads as up to date —
/// zero commits, zero changes — regardless of what the three-dot diff
/// would still show.
pub fn merge_preview(
    vcs: &dyn GitExecutor,
    root: &Path,
    target: &str,
    strategy: MergeStrategy,
) -> TgResult<MergePreview> {
    // An already-merged target brings nothing in (and a Fast-forward of it
    // is a no-op), so short-circuit before any diff is computed.
    if vcs.is_ancestor(root, target, "HEAD")? {
        return Ok(MergePreview {
            merge_commits: 0,
            files: 0,
            insertions: 0,
            deletions: 0,
        });
    }
    let numstat = vcs.run_raw(
        root,
        &[
            "diff".to_string(),
            "--numstat".to_string(),
            format!("HEAD...{target}"),
        ],
    )?;
    let mut files = 0usize;
    let mut insertions = 0usize;
    let mut deletions = 0usize;
    for line in numstat.lines() {
        // `added\tdeleted\tpath`; binary rows render `-` for both counts and
        // still count as one changed file.
        let mut cols = line.split('\t');
        let (Some(added), Some(deleted)) = (cols.next(), cols.next()) else {
            continue;
        };
        files += 1;
        insertions += added.parse::<usize>().unwrap_or(0);
        deletions += deleted.parse::<usize>().unwrap_or(0);
    }
    let merge_commits = match strategy {
        MergeStrategy::FastForward => 0,
        MergeStrategy::NoCommit | MergeStrategy::Commit | MergeStrategy::Squash => 1,
    };
    Ok(MergePreview {
        merge_commits,
        files,
        insertions,
        deletions,
    })
}

/// Other roots in the project that currently have the same branch checked
/// out as the focused root (issue 28's cascade banner): a merge on one of a
/// set of sibling repos implies the same merge on the others. Detached
/// heads and the focused root itself never count.
pub fn cascade_siblings(roots: &[Root], focused: &RootId) -> Vec<RootId> {
    let Some(focused) = roots.iter().find(|r| &r.id == focused) else {
        return Vec::new();
    };
    let Some(branch) = focused.current_branch.as_deref() else {
        return Vec::new();
    };
    roots
        .iter()
        .filter(|r| r.id != focused.id)
        .filter(|r| r.current_branch.as_deref() == Some(branch))
        .map(|r| r.id.clone())
        .collect()
}

/// Other roots in the project affected by rewriting the focused root's
/// current branch (issue 29, screen 15's cross-repo banner): a sibling is
/// affected when it has the same branch checked out, or when a local branch
/// of the same name tracks the same upstream — either way the rewrite
/// diverges its history. Detached heads and the focused root itself never
/// count.
pub fn rebase_affected(roots: &[Root], focused: &RootId) -> Vec<RootId> {
    let Some(focused) = roots.iter().find(|r| &r.id == focused) else {
        return Vec::new();
    };
    let Some(branch) = focused.current_branch.clone() else {
        return Vec::new();
    };
    let tracking = focused
        .branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == branch)
        .and_then(|b| b.tracking.clone());
    roots
        .iter()
        .filter(|r| r.id != focused.id)
        .filter(|r| {
            r.current_branch.as_deref() == Some(branch.as_str())
                || tracking.as_deref().is_some_and(|up| {
                    r.branches.iter().any(|b| {
                        b.kind == BranchKind::Local
                            && b.name == branch
                            && b.tracking.as_deref() == Some(up)
                    })
                })
        })
        .map(|r| r.id.clone())
        .collect()
}

/// Guard (issue 29, product spec §I safety note): history-rewriting actions
/// are blocked on protected branches. Rebase rewrites the current branch,
/// so a protected current branch refuses before git is touched.
/// `pub(crate)`: the interactive rebase editor's guarded dispatch
/// ([`history_editor::execute_with_backup`]) must guard *before* it writes
/// the backup ref, not after.
pub(crate) fn ensure_rebase_allowed(settings: &VcsSettings, branch: &str) -> TgResult<()> {
    if is_protected(settings, branch) {
        return Err(TgError::Other(format!(
            "'{branch}' is a protected branch — rebasing it is not allowed"
        )));
    }
    Ok(())
}

/// Rebase the current branch `branch` onto `onto` (issue 29's guarded
/// dialog dispatch). Refuses a protected current branch.
pub fn rebase_current(
    vcs: &dyn GitExecutor,
    root: &Path,
    onto: &str,
    opts: &RebaseOpts,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    ensure_rebase_allowed(settings, branch)?;
    vcs.rebase(root, onto, opts)
}

/// Replay the interactive plan for the current branch `branch` (issue 29's
/// guarded MODE=Interactive dispatch). Refuses a protected current branch.
pub fn rebase_plan(
    vcs: &dyn GitExecutor,
    root: &Path,
    plan: &[RebasePlanEntry],
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    ensure_rebase_allowed(settings, branch)?;
    vcs.rebase_interactive(root, plan)
}

/// Map the dialog's MODE selection plus the option rows onto the
/// executor-facing flags (issue 29, screen 15). The mode is the segmented
/// control: Autosquash always folds fixup!/squash! commits — the mode wins
/// over a leftover option toggle, the same rule `merge_flags` applies to a
/// strategy. Interactive mode runs through the plan path
/// ([`history_editor`] via the guarded [`rebase_plan`]) and never reaches
/// this mapping; if it somehow does, it maps like Standard.
pub fn rebase_mode_opts(
    mode: RebaseMode,
    update_refs: bool,
    keep_empty: bool,
    autosquash_fixups: bool,
) -> RebaseOpts {
    RebaseOpts {
        update_refs,
        keep_empty,
        autosquash: match mode {
            RebaseMode::Autosquash => true,
            RebaseMode::Interactive | RebaseMode::Standard => autosquash_fixups,
        },
        ..Default::default()
    }
}

/// Apply `commit` on top of the current branch.
pub fn cherry_pick(vcs: &dyn GitExecutor, root: &Path, commit: &str) -> TgResult<()> {
    vcs.cherry_pick(root, commit)
}

/// Apply `commit` onto the branch `target` (issue 15 log commit action):
/// check out the target, cherry-pick, then return to the branch that was
/// checked out before. Guarded: a protected target branch and a dirty
/// working tree are refused before git is touched.
pub fn cherry_pick_to(
    vcs: &dyn GitExecutor,
    root: &Path,
    commit: &str,
    target: &str,
    settings: &VcsSettings,
) -> TgResult<()> {
    if is_protected(settings, target) {
        return Err(TgError::Other(format!(
            "Refusing to cherry-pick onto protected branch '{target}'"
        )));
    }
    if !vcs.status(root)?.changes.is_empty() {
        return Err(TgError::Other(
            "Working tree is dirty — commit or shelve your changes before cherry-picking".into(),
        ));
    }
    let original = vcs.current_branch(root)?;
    vcs.branch_checkout(root, target)?;
    match vcs.cherry_pick(root, commit) {
        Ok(()) => {
            if let Some(branch) = original {
                vcs.branch_checkout(root, &branch)?;
            }
            Ok(())
        }
        Err(e) => {
            // Best-effort return to the original branch even on failure, so
            // the user is not stranded on the target branch mid-pick.
            if let Some(branch) = original {
                let _ = vcs.branch_checkout(root, &branch);
            }
            Err(e)
        }
    }
}

/// Revert `commit` with an inverse commit on the current branch (issue 15
/// log commit action). Guarded: a protected current branch and a dirty
/// working tree are refused before git is touched.
pub fn revert_commit(
    vcs: &dyn GitExecutor,
    root: &Path,
    commit: &str,
    settings: &VcsSettings,
) -> TgResult<()> {
    if let Some(branch) = vcs.current_branch(root)?
        && is_protected(settings, &branch)
    {
        return Err(TgError::Other(format!(
            "'{branch}' is a protected branch — reverting commits on it is not allowed"
        )));
    }
    if !vcs.status(root)?.changes.is_empty() {
        return Err(TgError::Other(
            "Working tree is dirty — commit or shelve your changes before reverting".into(),
        ));
    }
    vcs.revert(root, commit)
}

/// Abort an in-progress `op` (merge / rebase / cherry-pick).
pub fn abort(vcs: &dyn GitExecutor, root: &Path, op: &str) -> TgResult<()> {
    vcs.abort(root, op)
}

/// Continue an in-progress `op` after resolving conflicts.
pub fn cont(vcs: &dyn GitExecutor, root: &Path, op: &str) -> TgResult<()> {
    vcs.continue_op(root, op)
}

/// Merge `target` while automatically stashing a dirty tree first.
///
/// If the working tree is dirty it is stashed (stash is used for both the
/// `Stash` and `Shelve` methods). The stash is popped afterwards; a pop failure
/// is ignored so the stash is left in place. A merge failure still triggers a
/// best-effort pop before returning the merge error.
pub fn smart_merge(
    vcs: &dyn GitExecutor,
    root: &Path,
    target: &str,
    opts: &MergeOpts,
    clean: CleanTreeMethod,
) -> TgResult<()> {
    let dirty = !vcs.status(root)?.changes.is_empty();
    if dirty {
        // Use stash for both methods; ignore a failing stash push.
        let _ = vcs.stash_push(
            root,
            "TurboGit smart merge",
            clean == CleanTreeMethod::Shelve,
        );
    }

    match vcs.merge(root, target, opts) {
        Ok(()) => {
            if dirty {
                // Ignore pop errors: leave the stash in place for the user.
                let _ = vcs.stash_pop(root, 0);
            }
            Ok(())
        }
        Err(e) => {
            if dirty {
                let _ = vcs.stash_pop(root, 0);
            }
            Err(e)
        }
    }
}

/// Whether a merge / rebase / cherry-pick is currently in progress at `root`.
pub fn in_progress(root: &Path) -> bool {
    let git = Path::new(root).join(".git");
    git.join("MERGE_HEAD").exists()
        || git.join("rebase-merge").exists()
        || git.join("rebase-apply").exists()
        || git.join("CHERRY_PICK_HEAD").exists()
}
