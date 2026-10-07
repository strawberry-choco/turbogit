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

/// The operation that currently owns a repository — the one a rewrite verb must
/// wait for.
///
/// This is a *named* state rather than a boolean on purpose: a refusal that says
/// only "busy" cannot be acted on, and the four owners need different fixes
/// (finish the cherry-pick, continue the rebase, resolve the merge). One
/// repository has one owner, which is why [`owning_operation`] answers with a
/// single `Option` and never a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedOperation {
    /// `.git/CHERRY_PICK_HEAD` — a pick is held, usually mid-conflict.
    CherryPick,
    /// `.git/rebase-merge` / `.git/rebase-apply` — a rebase (plain or
    /// interactive) is running.
    Rebase,
    /// `.git/MERGE_HEAD` — a merge is held uncommitted.
    Merge,
    /// `.git/REVERT_HEAD` — a revert is held, usually mid-conflict.
    Revert,
}

impl OwnedOperation {
    /// The one word a refusal names the owner with. Lowercase: it rides inside
    /// a sentence that already starts with the repository's state.
    pub fn label(self) -> &'static str {
        match self {
            Self::CherryPick => "cherry-pick",
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::Revert => "revert",
        }
    }

    /// The argv token git's own recovery flags hang off: `git <this> --abort`,
    /// `git <this> --continue`.
    ///
    /// Spelled out per case rather than returned as [`Self::label`], even though
    /// the four strings are identical today: `label` is prose owned by the
    /// refusal sentences, and a reworded one ("Rebasing") would otherwise
    /// silently change what runs. The operation's identity, not its wording, is
    /// what the engine is addressed by.
    pub fn git_subcommand(self) -> &'static str {
        match self {
            Self::CherryPick => "cherry-pick",
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::Revert => "revert",
        }
    }
}

/// Where git keeps its operation markers for `root`.
///
/// `<root>/.git` for an ordinary repository — but a linked worktree or a
/// submodule checkout has a `.git` FILE whose `gitdir:` line points at the real
/// directory, and HEAD, `MERGE_HEAD`, `CHERRY_PICK_HEAD` and the rebase
/// directories all live at THAT path. Reading `<worktree>/.git/…` instead would
/// report every worktree as idle, which is the cross-root half of a multi-root
/// gating bug: the app would rewrite inside a repository that is mid-operation.
///
/// A bare repository answers `<root>/.git` (no `.git` entry at all) and so reads
/// as idle here, exactly as it did before this function existed.
fn markers_dir(root: &Path) -> PathBuf {
    let dot_git = root.join(".git");
    let Ok(text) = std::fs::read_to_string(&dot_git) else {
        return dot_git;
    };
    let Some(pointer) = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|target| !target.is_empty())
    else {
        return dot_git;
    };
    let target = Path::new(pointer);
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        root.join(target)
    }
}

/// Which operation owns the repository at `root`, if any.
///
/// The marker each state leaves behind is git's, not ours, and was measured
/// against git 2.50 rather than assumed: a held cherry-pick leaves
/// `CHERRY_PICK_HEAD`, a held revert `REVERT_HEAD`, a held merge `MERGE_HEAD`, a
/// held rebase the `rebase-merge` directory (or `rebase-apply` with `--apply`,
/// plus `REBASE_HEAD`). They are disjoint on that version, which is why each is
/// asked on its own and why a refusal can name the owner instead of saying
/// "busy".
///
/// The question order is still load-bearing, because a *sequence* also holds
/// commits: the rebase directory outranks the holder files it may be stopped on
/// (`CHERRY_PICK_HEAD` inside a rebase is the sequencer re-using the pick
/// machinery — the pick is not the operation the developer started), and
/// `MERGE_HEAD` is last because the merge machinery is what the other three run
/// while they resolve. Ask in the other order and the refusal blames the wrong
/// operation.
pub fn owning_operation(root: &Path) -> Option<OwnedOperation> {
    let git = markers_dir(root);
    if git.join("rebase-merge").exists() || git.join("rebase-apply").exists() {
        return Some(OwnedOperation::Rebase);
    }
    if git.join("CHERRY_PICK_HEAD").exists() {
        return Some(OwnedOperation::CherryPick);
    }
    if git.join("REVERT_HEAD").exists() {
        return Some(OwnedOperation::Revert);
    }
    if git.join("MERGE_HEAD").exists() {
        return Some(OwnedOperation::Merge);
    }
    None
}

/// Whether a merge / rebase / cherry-pick is currently in progress at `root`.
///
/// **Not `owning_operation(root).is_some()`**, and deliberately: a held revert
/// answers `true` to the latter, and the one caller that recovers from this
/// ([`crate::history_editor::abort_to_backup`]) reads it to decide whether to run
/// `git rebase --abort` — a rebase abort in a revert state is a second failure on
/// top of the one being recovered. So this stays the three states it was written
/// for, and the revert state is available through [`owning_operation`] to
/// anything that asks the wider question.
pub fn in_progress(root: &Path) -> bool {
    matches!(
        owning_operation(root),
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
