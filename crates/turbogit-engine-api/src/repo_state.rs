//! Which operation holds a repository, as the git boundary answers it.
//!
//! [`OwnedOperation`] is the value [`GitExecutor::holding_operation`] answers
//! with, and [`holding_operation_at`] is the read behind that answer: git's own
//! marker files, resolved through the repository's real git directory.
//!
//! The probe lives here rather than in an adapter because both adapters and the
//! path-shaped read that marker-planting fixtures use have to agree; two copies
//! of the ranking is how a refusal and a status-bar item come to blame different
//! operations for one repository. It is a `stat` read of a bounded set of paths —
//! never a git process — which is what lets a surface ask it every frame.

use std::path::{Path, PathBuf};

/// The operation that currently owns a repository — the one a rewrite verb must
/// wait for.
///
/// This is a *named* state rather than a boolean on purpose: a refusal that says
/// only "busy" cannot be acted on, and the four owners need different fixes
/// (finish the cherry-pick, continue the rebase, resolve the merge). One
/// repository has one owner, which is why the port's read answers with a single
/// `Option` and never a set.
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

/// Which operation owns the repository at `root`, if any — read from git's own
/// markers rather than from a flag the application could have set itself.
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
pub fn holding_operation_at(root: &Path) -> Option<OwnedOperation> {
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
