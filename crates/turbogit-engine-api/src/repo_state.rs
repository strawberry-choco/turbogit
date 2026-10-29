use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedOperation {
    CherryPick,
    Rebase,
    Merge,
    Revert,
}

impl OwnedOperation {
    pub fn label(self) -> &'static str {
        match self {
            Self::CherryPick => "cherry-pick",
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::Revert => "revert",
        }
    }
}

/// `<root>/.git` for an ordinary repository, but a linked worktree or a
/// submodule checkout has a `.git` FILE whose `gitdir:` line points at the real
/// directory, and HEAD, `MERGE_HEAD`, `CHERRY_PICK_HEAD` and the rebase
/// directories all live at THAT path. Reading `<worktree>/.git/…` instead would
/// report every worktree as idle, which is the cross-root half of a multi-root
/// gating bug: the app would rewrite inside a repository that is mid-operation.
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

/// The marker each state leaves behind is git's, not ours, and was measured
/// against git 2.50 rather than assumed: a held cherry-pick leaves
/// `CHERRY_PICK_HEAD`, a held revert `REVERT_HEAD`, a held merge `MERGE_HEAD`, a
/// held rebase the `rebase-merge` directory (or `rebase-apply` with `--apply`,
/// plus `REBASE_HEAD`). They are disjoint on that version, which is why each is
/// asked on its own and why a refusal can name the owner instead of saying
/// "busy".
/// The question order is still load-bearing, because a *sequence* also holds
/// commits: the rebase directory outranks the holder files it may be stopped on
/// (`CHERRY_PICK_HEAD` inside a rebase is the sequencer re-using the pick
/// machinery, the pick is not the operation the developer started), and
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
