use std::collections::HashSet;

use turbogit_app::integrate_service::OwnedOperation;
use turbogit_domain::model::{Commit, RootId};

use crate::ui::commit_menu::{CommitFacts, reason};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriteVerb {
    Drop,
    Reword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Own,
    Several,
    External,
}

pub fn scope_of(selection: &HashSet<RootId>, root: &RootId) -> Scope {
    if selection.is_empty() {
        return Scope::Own;
    }
    if selection.len() > 1 {
        return Scope::Several;
    }
    if selection.contains(root) {
        return Scope::Own;
    }
    Scope::External
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Owned(OwnedOperation),
    MergeCommit,
    RootCommit,
    OffCurrentBranch,
    SeveralInScope,
    OutsideScope,
    Protected,
}

mod text {
    pub(super) const CHERRY_PICK: &str = "Cannot rewrite history during a cherry-pick";
    pub(super) const REBASE: &str = "Cannot rewrite history during a rebase";
    pub(super) const MERGE: &str = "Cannot rewrite history during a merge";
    pub(super) const REVERT: &str = "Cannot rewrite history during a revert";
    pub(super) const MERGE_DROP: &str = "a merge commit cannot be dropped";
    pub(super) const MERGE_REWORD: &str = "a merge commit cannot be reworded";
    pub(super) const ROOT_DROP: &str = "a root commit cannot be dropped";
    pub(super) const ROOT_REWORD: &str = "a root commit cannot be reworded";
    pub(super) const OFF_BRANCH: &str = "not on the current branch";
    pub(super) const SEVERAL: &str = "the rewrite runs in one repository; several are in scope";
    pub(super) const EXTERNAL: &str = "the commit is in a repository outside the ones in scope";
}

impl Block {
    pub fn stated(self, verb: RewriteVerb) -> &'static str {
        match self {
            Self::Owned(OwnedOperation::CherryPick) => text::CHERRY_PICK,
            Self::Owned(OwnedOperation::Rebase) => text::REBASE,
            Self::Owned(OwnedOperation::Merge) => text::MERGE,
            Self::Owned(OwnedOperation::Revert) => text::REVERT,
            Self::MergeCommit => match verb {
                RewriteVerb::Drop => text::MERGE_DROP,
                RewriteVerb::Reword => text::MERGE_REWORD,
            },
            Self::RootCommit => match verb {
                RewriteVerb::Drop => text::ROOT_DROP,
                RewriteVerb::Reword => text::ROOT_REWORD,
            },
            Self::OffCurrentBranch => text::OFF_BRANCH,
            Self::SeveralInScope => text::SEVERAL,
            Self::OutsideScope => text::EXTERNAL,
            Self::Protected => reason::PROTECTED,
        }
    }
}

pub fn blocked(commit: &Commit, facts: &CommitFacts<'_>) -> Option<Block> {
    if let Some(owner) = facts.owner {
        return Some(Block::Owned(owner));
    }
    if commit.parents.len() > 1 {
        return Some(Block::MergeCommit);
    }
    if commit.parents.is_empty() {
        return Some(Block::RootCommit);
    }
    if !facts.on_current_branch {
        return Some(Block::OffCurrentBranch);
    }
    match facts.scope {
        Scope::Several => return Some(Block::SeveralInScope),
        Scope::External => return Some(Block::OutsideScope),
        Scope::Own => {}
    }
    if facts.protected_branch {
        return Some(Block::Protected);
    }
    None
}
