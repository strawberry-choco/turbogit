//! Integration service: high-level merge / rebase / cherry-pick orchestration
//! plus abort/continue and a "smart merge" that stashes a dirty tree first.

use crate::sync_service::is_protected;
use std::path::{Path, PathBuf};
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
/// The totals come from one **Git engine** question — `ChangeQuestion::
/// MergeIntoHead`, whose merge-base range is the engine's to spell. A target
/// already merged into HEAD reads as up to date — zero commits, zero changes —
/// regardless what that comparison would still show.
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
    let stats = vcs.change_stats(
        root,
        &ChangeQuestion::MergeIntoHead {
            target: target.to_string(),
        },
    )?;
    let merge_commits = match strategy {
        MergeStrategy::FastForward => 0,
        MergeStrategy::NoCommit | MergeStrategy::Commit | MergeStrategy::Squash => 1,
    };
    Ok(MergePreview {
        merge_commits,
        files: stats.file_count(),
        insertions: stats.insertions(),
        deletions: stats.deletions(),
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
                || tracking.as_ref().is_some_and(|up| {
                    r.branches.iter().any(|b| {
                        b.kind == BranchKind::Local
                            && b.name == branch
                            && b.tracking.as_ref() == Some(up)
                    })
                })
        })
        .map(|r| r.id.clone())
        .collect()
}

/// Guard (issue 29, product spec §I safety note): history-rewriting actions
/// are blocked on protected branches. Rebase rewrites the current branch,
/// so a protected current branch refuses before git is touched.
/// `pub`: every guarded rewrite calls it — the dialog dispatches here, the
/// interactive rebase editor's [`history_editor::execute_with_backup`] must
/// guard *before* it writes the backup ref, and
/// [`crate::sync_service::pull_guarded`] guards the pull that rebases. A surface
/// that answers the same question before
/// it even dispatches (the checkout half of a `Checkout and pull`) asks here too,
/// because one refusal text is the point: a rewrite that refuses differently from
/// another is two rules wearing one name.
pub fn ensure_rebase_allowed(settings: &VcsSettings, branch: &str) -> TgResult<()> {
    if is_protected(settings, branch) {
        return Err(TgError::ProtectedBranch {
            branch: branch.to_string(),
            consequence: "rebasing it is not allowed",
        });
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

/// `git rebase --onto <new base> <old base>`: move only the sequence ABOVE the
/// old base onto the new base (the dialog's `--onto` mode). Guarded exactly like
/// [`rebase_current`], because it rewrites exactly the same branch.
///
/// The two bases land on git's argv in DIFFERENT places and swapping them
/// rebases the wrong commits: the new base is the `--onto` flag's value
/// ([`RebaseOpts::onto`]) while the old base is the positional upstream `rebase`
/// reads the cutoff from. That asymmetry is why this is one named call rather
/// than two arguments at the caller.
pub fn rebase_with_onto(
    vcs: &dyn GitExecutor,
    root: &Path,
    new_base: &str,
    old_base: &str,
    opts: &RebaseOpts,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    ensure_rebase_allowed(settings, branch)?;
    let with_flag = RebaseOpts {
        onto: Some(new_base.to_string()),
        ..opts.clone()
    };
    vcs.rebase(root, old_base, &with_flag)
}

/// The operation that holds a repository, and the read that names it.
///
/// Both live at the git boundary (`turbogit_engine_api::repo_state`), because
/// marker files are git's on-disk state: the code that interprets them sits in
/// the port's own crate, and nothing above the port reaches `std::fs` for them.
/// Production asks [`GitExecutor::holding_operation`]; these two names carry the
/// same answer for a caller that holds a path and no executor — the suites that
/// plant git's markers by hand to state a repository's condition.
pub use turbogit_engine_api::OwnedOperation;
pub use turbogit_engine_api::repo_state::holding_operation_at as owning_operation;

/// Whether a merge / rebase / cherry-pick is currently in progress at `root`.
///
/// A caller that has the engine asks [`operation_in_progress`] with the owner the
/// port named; this path form is the same answer for a caller that has a
/// repository and no executor.
pub fn in_progress(root: &Path) -> bool {
    operation_in_progress(owning_operation(root))
}

/// Whether `owner` is an operation the recovery path can abort as a rebase.
///
/// **Not `owner.is_some()`**, and deliberately: a held revert answers `true` to
/// the wider question, and the one caller that recovers from this
/// ([`crate::history_editor::abort_to_backup`]) reads it to decide whether to run
/// `git rebase --abort` — a rebase abort in a revert state is a second failure on
/// top of the one being recovered. So this stays the three states it was written
/// for, and the revert state is available through [`GitExecutor::holding_operation`]
/// to anything that asks the wider question.
pub fn operation_in_progress(owner: Option<OwnedOperation>) -> bool {
    matches!(
        owner,
        Some(OwnedOperation::CherryPick | OwnedOperation::Rebase | OwnedOperation::Merge)
    )
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
/// checked out before. Guarded, in this order, before git is touched: the
/// branch the work is standing on is not a destination at all (the commit is
/// already on it), a protected target may not be written to, and a dirty
/// working tree entangles the pick. The first guard is the one the branch
/// picker states on the row it disables, so a bypass of that dialog is refused
/// for the reason the developer was shown.
pub fn cherry_pick_to(
    vcs: &dyn GitExecutor,
    root: &Path,
    commit: &str,
    target: &str,
    settings: &VcsSettings,
) -> TgResult<()> {
    let original = vcs.current_branch(root)?;
    if original.as_deref() == Some(target) {
        return Err(TgError::Other(format!(
            "Refusing to cherry-pick onto the current branch '{target}'"
        )));
    }
    if is_protected(settings, target) {
        return Err(TgError::Other(format!(
            "Refusing to cherry-pick onto the protected branch '{target}'"
        )));
    }
    if !vcs.status(root)?.changes.is_empty() {
        return Err(TgError::Other(
            "Working tree is dirty — commit or shelve your changes before cherry-picking".into(),
        ));
    }
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
        return Err(TgError::ProtectedBranch {
            branch,
            consequence: "reverting commits on it is not allowed",
        });
    }
    if !vcs.status(root)?.changes.is_empty() {
        return Err(TgError::Other(
            "Working tree is dirty — commit or shelve your changes before reverting".into(),
        ));
    }
    vcs.revert(root, commit)
}

// ------------------------------------------------- verb 5: Get from Branch ---
//
// The reference's fifth integration verb is not a history operation at all: it
// compares ONE FILE
// across two sides and copies the other side's whole contents into the working
// tree. Neither `cherry-pick` nor `merge` covers "take that one file from over
// there", and it is the only verb that can bring across a file the target branch
// does not have.

/// One rung of the **change-kind grammar** a file-level comparison reads: the
/// same three-way rule the diff viewer states (Green = Added, Blue = Modified,
/// Gray = Deleted), which is why it is a design-system rule rather than a
/// per-call-site choice.
///
/// The two one-sided rungs are defined against the comparison's **base side**, and
/// that is what makes `Swap Branches` a re-reading rather than a re-colouring: the
/// set of differing paths never changes, but which side is the base does, so a row
/// that was "on the other side only" becomes "on the base side only" and moves to
/// the other end of the list. Spell the two sides literally as *branch* and
/// *working tree* instead, and swapping them needs a second git read whose file set
/// can disagree with the first one's.
///
/// The derived `Ord` is the list order — grey, then green, then blue. A swap
/// exchanges the two one-sided rows' **positions** because of that order, which is
/// the fact the parity suite asserts (a swap that left the rows in place and only
/// re-tinted them would have inverted the grammar: grey would mean "missing from
/// the base" rather than "missing from the branch you are standing on").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BranchFileStatus {
    /// Grey — the path exists on the base side and is absent on the other.
    Deleted,
    /// Green — the path exists on the other side and is absent on the base.
    Added,
    /// Blue — the path exists on both sides and its content differs.
    Modified,
}

/// One row of that list: which rung it sits on, and the repository-relative path.
///
/// **Field order is the sort key.** `BranchFileStatus`'s own derived `Ord` is its
/// declaration order — grey, green, blue — and declaring `status` ahead of `path`
/// is what makes `Ord` for this row mean "rung first, path within the rung". The
/// list the window paints is that order, and the two one-sided rows exchanging
/// **positions** under `Swap Branches` is a fact about the ordering, not about the
/// tint; reordering these fields would silently re-sort the window by path and turn
/// the swap into a re-colouring, which is the wrong answer: it discloses nothing
/// about which side lost the path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BranchFileRow {
    pub status: BranchFileStatus,
    pub path: PathBuf,
}

/// The file-level list behind `Show Diff with Working Tree`: every path that
/// differs between `branch` and the working tree of `root`, each on one of the
/// three rungs.
///
/// One git read answers the whole window — `git diff <branch>` is literally the
/// reference's "diff with working tree" (its left side is the branch, its right
/// side is the untouched commit), and git's own `new file` / `deleted file`
/// headers are what say which side a path is missing from. `base_is_branch` is
/// the window's posture after `Swap Branches`: `true` reads the branch as the base
/// (the flow's opening posture, where grey means "the branch has it, you do not"),
/// `false` reads the working tree as the base and exchanges which paths land on
/// which one-sided rung. Nothing else about the answer changes — the path set is
/// the same either way, which is what makes the swap a re-reading of one fact
/// rather than a second question to git.
pub fn branch_file_changes(
    vcs: &dyn GitExecutor,
    root: &Path,
    branch: &str,
    base_is_branch: bool,
) -> TgResult<Vec<BranchFileRow>> {
    let patch = vcs.diff_patch(
        root,
        &DiffOpts {
            left: Some(branch.to_string()),
            ..Default::default()
        },
    )?;
    let mut rows: Vec<BranchFileRow> = patch
        .files
        .iter()
        .map(|file| {
            // git spells the two one-sided cases as headers on the section, so
            // the rung is read off them and never re-derived from hunk counts —
            // a mode-only change has hunks and is `Modified`, a rename has none
            // and is `Modified` too, and both readings are right.
            let status = if file.deleted_file() {
                // Present on the branch side, gone from the worktree.
                if base_is_branch {
                    BranchFileStatus::Deleted
                } else {
                    BranchFileStatus::Added
                }
            } else if file.new_file() {
                // Present in the worktree, unknown to the branch.
                if base_is_branch {
                    BranchFileStatus::Added
                } else {
                    BranchFileStatus::Deleted
                }
            } else {
                BranchFileStatus::Modified
            };
            // A deleted path has no `b/` side to name it by, and a one-sided row
            // is exactly the case that has to be named correctly.
            let name = if file.deleted_file() {
                &file.old_path
            } else {
                &file.new_path
            };
            BranchFileRow {
                path: PathBuf::from(name),
                status,
            }
        })
        .collect();
    rows.sort();
    Ok(rows)
}

/// `Get from Branch`: replace `path` in the working tree with **the entire
/// contents it has on `branch`** — the reference's own description of the verb
/// ("will copy the entire contents of the file to the current branch").
///
/// The read is the port's [`GitExecutor::show_file_bytes`], binary-safe, so an
/// image or a CRLF-sensitive file crosses intact rather than through a UTF-8
/// round trip. The write is a plain worktree write, which is where the verb's
/// shape lands: the file arrives as an ordinary local change the developer then
/// commits, rather than as a checked-out index entry the window had to explain.
/// That is also why the surface offers this verb on the **blue** rung only — on a
/// green row the branch has no copy of the file at all, and taking "the branch's
/// contents" would be a delete wearing a take's label.
pub fn get_file_from_branch(
    vcs: &dyn GitExecutor,
    root: &Path,
    branch: &str,
    path: &Path,
) -> TgResult<()> {
    let bytes = vcs.show_file_bytes(root, branch, path)?;
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, bytes)?;
    Ok(())
}

/// Abort an in-progress `op` (merge / rebase / cherry-pick).
pub fn abort(vcs: &dyn GitExecutor, root: &Path, op: &str) -> TgResult<()> {
    vcs.abort(root, op)
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
