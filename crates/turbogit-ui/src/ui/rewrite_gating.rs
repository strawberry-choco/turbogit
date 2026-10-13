//! Rewrite gating: the ONE reason a history verb states, read off the state of
//! the repository the verb would run in.
//!
//! The reference does not gate Drop / Reword on a boolean ("is a rewrite
//! allowed?") but on **which operation owns the repository**, and its refusal
//! vocabulary is closed rather than open-ended: four operation states (a
//! cherry-pick, a rebase, a merge and a revert in flight), one structural reason
//! about the commit itself, and two about the repositories in scope. This module
//! is that enumeration, spelled once, and `blocked` returns at most one of them
//! — which is the property, not a detail. A menu that listed every applicable
//! reason would read as noise, and a menu that picked by `if`-chain at the call
//! site would let two surfaces pick differently.
//!
//! **One reason per item, ordered by what the developer can act on next.** The
//! order in [`blocked`] compiles either way, so it is stated here: the
//! repository's owner first (nothing else about the commit matters while another
//! operation holds it, and finishing that operation is the only way forward),
//! then the commit's own shape (merge, root — facts of the object, never stale),
//! then where the commit sits (off the current branch: the plan is built to this
//! branch's tip, so the commit has no row in it), then the workspace's posture
//! (the scope pair below), and last the branch policy. The last of those is the
//! one the reference does not have at all: TurboGit refuses a protected branch
//! UP FRONT rather than discovering it at push time, and that guard stays here
//! ahead of nothing and behind everything, because a protected branch is a policy
//! the developer may have chosen deliberately and it is the least actionable of
//! the seven.
//!
//! Everything is computed from plain facts handed in by the caller
//! ([`crate::ui::commit_menu::CommitFacts`]) and the commit itself, so no gate
//! here can contradict the commit it is gating, and none costs a git call.
//!
//! Wording is TurboGit's: the copy rule adopts the reference's *semantics* — the
//! closed set, the per-root discrimination, the single reason — and none of its
//! strings.

use std::collections::HashSet;

use turbogit_app::integrate_service::OwnedOperation;
use turbogit_domain::model::{Commit, RootId};

use crate::ui::commit_menu::{CommitFacts, reason};

/// The two verbs this gate bounds. Both route through the same plan-building
/// path, which is why they are bounded identically and differ only in the two
/// sentences that name what the verb does to the commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriteVerb {
    Drop,
    Reword,
}

/// Which repositories the workspace has in scope, as seen from the commit a
/// single-repository verb would act on.
///
/// This is the multi-root half of the vocabulary, and the reason TurboGit can
/// state it at all is that its log is a union listing across roots while a
/// rewrite plan is built in exactly one repository. The reference's two
/// cross-repository reasons are about a selection of commits spread over
/// repositories; TurboGit's menu acts on one commit, so the same discrimination
/// lands on the checked-repository set of the workspace tree — the app's
/// `UiState::repo_selection`, the scope every fleet
/// verb already reads, and the one the status bar's own `N repos in scope`
/// counter reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// This commit's repository is the one in scope — either nothing is checked
    /// (the workspace reads as one repository, its own, which is what the status
    /// bar says then) or exactly this root is.
    Own,
    /// Two or more repositories are checked, so a verb that runs in one of them
    /// is being aimed at a scope wider than itself.
    Several,
    /// Repositories are checked, and this commit's is not among them.
    External,
}

/// The scope `root` is in for a verb that acts in one repository.
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

/// One blocked reason. An exhaustive enum rather than an `Option<String>`: a
/// reason the vocabulary does not name cannot be constructed, and adding an
/// eighth has to come through this list and the order in [`blocked`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    /// Another operation holds the repository, and the refusal names it: a
    /// cherry-pick, a rebase, a merge and a revert need different fixes, so
    /// "busy" is not an answer.
    Owned(OwnedOperation),
    /// The commit has more than one parent, and nothing routes a merge through
    /// the targeted-plan path.
    MergeCommit,
    /// The commit has no parent, so there is no `HEAD~1` for a plan to be built
    /// over.
    RootCommit,
    /// The commit is not in the current branch's history, so it has no row in
    /// the plan the verb would build.
    OffCurrentBranch,
    /// Several repositories are in scope for a verb that runs in one.
    SeveralInScope,
    /// The commit's repository is outside the ones in scope.
    OutsideScope,
    /// The branch the rewrite would move is protected.
    Protected,
}

mod text {
    /// The four operation states share their sentence and differ only by the
    /// owner's one-word name, which is why they are four consts rather than a
    /// formatted string: the status bar paints a `&'static str` it never
    /// reallocates, and `OwnedOperation::label`'s lowercase noun is exactly the
    /// hole in this shape.
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
    /// The sentence this reason states, for this verb.
    ///
    /// Only the two commit-shape reasons vary by verb; the rest say the same
    /// thing whatever the developer meant to do, because they are about the
    /// repository, the scope or the branch, not about the edit.
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
            // The menu's own sentence, shared with Revert commit: two verbs that
            // refuse for the same policy state it in the same words.
            Self::Protected => reason::PROTECTED,
        }
    }
}

/// Why this rewrite verb is blocked here, if it is blocked at all.
///
/// `None` means the item is live — and enablement is the whole affordance: no
/// confirmation is added by this gate, and the preflight a live verb opens is
/// the shared RewritePreflight dialog.
///
/// The order of the five arms is load-bearing and compiles either way; see the
/// module doc. `scope` is matched rather than tested as a boolean so the two
/// cross-repository reasons stay mutually exclusive by construction.
///
/// The verb is not an input: which reason binds does not depend on what the
/// developer meant to do, only on what is true of the repository, the commit and
/// the scope. The verb enters at [`Block::stated`], where the sentence is
/// written.
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
