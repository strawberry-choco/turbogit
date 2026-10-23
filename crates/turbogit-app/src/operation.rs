//! Operation (CONTEXT.md "Operation"): the unit the **Shell** dispatches.
//!
//! One typed value carrying what to run, its display label, its invalidation
//! [`Affected`] scope, whether it changes a root's worktree set, and — for the
//! operations that have one — its identity. Settlement matches on that identity
//! ([`OpKind`]) rather than reverse-engineering it from the label prose, so a
//! reworded message cannot change what the app does and a new operation that
//! forgot its settlement arm is a compile error.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{
    Change, CleanTreeMethod, CommitId, DisplayName, MergeOpts, RebaseOpts, RebasePlanEntry, Root,
    RootId, UpdateMethod, VcsSettings,
};
use turbogit_engine_api::GitExecutor;
use turbogit_services::integrate_service::OwnedOperation;

use crate::root_caches::Affected;

/// The operation's identity, which is what rides back in the completion event.
///
/// [`AppState::dispatch`](crate::state::AppState::dispatch) consumes the
/// [`Operation`] to run it, so the operation itself cannot travel back to the
/// render thread; this tag is the copyable remainder of it. Settlement is one
/// exhaustive match over these cases and never inspects label text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Fetch,
    Merge,
    Rebase,
    DeleteBranch,
    Shelve,
    /// [`Operation::Unshelve`] — one shelf's content put back in the working
    /// tree. Its own identity because the shelf's IDE record is app state, not
    /// git state, and only a named kind lets settlement drop that record once
    /// the git side has actually landed (never on a failure).
    Unshelve,
    WorktreeAdd,
    WorktreeRemove,
    /// [`Operation::Commit`] — the index written as a commit on one root's HEAD.
    /// Its own identity because the Push dialog's open must be gated on *which*
    /// operation landed, and the [`Operation::Custom`] tail cannot answer that:
    /// commit shared it with staging (`shell_writes::commit::set_staged`), so
    /// the two are one identity there, and settlement may not read label prose
    /// to tell them apart (ADR-0020).
    Commit,
    /// [`Operation::DropCommit`] — one commit removed from the current branch's
    /// history, its descendants replayed.
    DropCommit,
    /// [`Operation::RewordCommit`] — one commit's message rewritten.
    RewordCommit,
    /// [`Operation::Push`] — a push dispatched from the Push dialog. Its own
    /// identity so settlement can classify a rejection instead of
    /// reading label prose.
    Push,
    /// [`Operation::Push`] carrying a [`RecoveryPlan`] — the rejected-push
    /// recovery (update or fetch, then re-push). A separate identity from
    /// [`OpKind::Push`] because the same error text classifies differently in
    /// the two phases, and because only a push-phase rejection may trigger
    /// the silent auto-update (a failed recovery must not re-trigger itself).
    PushRecovery,
    /// [`Operation::Resolve`] — one of the three in-flight resolutions (Abort,
    /// Continue, Skip) run from the status bar or the palette. Its own identity
    /// because settlement treats a continuation that stops on the NEXT conflict
    /// as the same handoff a first-pass merge or rebase gets, and because the
    /// durable record has to name the verb rather than the operation it
    /// resolved.
    Resolve,
    /// [`Operation::GetFileFromBranch`] — the reference's fifth integration verb,
    /// `Get from Branch`. Its own identity because what it settles is a worktree
    /// file rather than a history: the record has to name the path and the branch
    /// it came from, and the changes window that offered the verb has to re-read
    /// the rung list the moment it lands (a file whose contents now match is no
    /// longer a row, and a window that still lists it would offer to take it
    /// again).
    GetFileFromBranch,
    /// [`Operation::ApplyPatch`] — write a patch into one root's working tree.
    /// Its own identity because its outcome is three-way and `TgResult<()>` is
    /// two-way: an apply that landed, an apply that failed and was rolled back,
    /// and an apply that failed and could NOT be rolled back are three different
    /// things to tell the developer, and collapsing the third into the second is
    /// the exact promise this surface must not make.
    ApplyPatch,
    /// The [`Operation::Custom`] tail: dispatched, but nothing settles on it.
    Other,
}

/// How an operation a repository is holding gets resolved — the three verbs the
/// status bar announces.
///
/// The table lives here, beside the dispatch unit, rather than at either
/// presentation site: the bar and the palette must offer the same verb set for
/// the same state, and a second copy of that pair would be free to disagree.
/// [`LifecycleVerb::offered_for`] is exhaustive over [`OwnedOperation`], so a
/// fifth held state in git makes the verb question a compile error instead of a
/// silent two-button announcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleVerb {
    /// `git <op> --abort` — give the whole operation up.
    Abort,
    /// `git <op> --continue` — retry the commit that stopped it, now resolved.
    Continue,
    /// `git rebase --skip` — abandon the commit that stopped it and carry on.
    /// The third resolution a paused rebase has, and the one you want when the
    /// NEXT commit is the one that conflicts; Abort would throw away the whole
    /// rebase and Continue would retry the same failing commit forever.
    Skip,
}

impl LifecycleVerb {
    /// The verb as one word: the status bar's control label, where the
    /// announcement beside it has already named the operation.
    pub fn label(self) -> &'static str {
        match self {
            Self::Abort => "Abort",
            Self::Continue => "Continue",
            Self::Skip => "Skip",
        }
    }

    /// The sentence the toast and the activity feed leave behind. `Skip` names
    /// the object rather than the operation because that is what the reference's
    /// own bundle label says (`Skip Commit`), and a rebase is the only state
    /// [`Self::offered_for`] hands it out in.
    pub fn label_for(self, owner: OwnedOperation) -> String {
        match self {
            Self::Abort => format!("Abort {}", owner.label()),
            Self::Continue => format!("Continue {}", owner.label()),
            Self::Skip => "Skip commit".to_owned(),
        }
    }

    /// The resolutions one held state actually has, in the order the bar paints
    /// them: the destructive verb first and apart, then the two that carry on.
    pub fn offered_for(owner: OwnedOperation) -> &'static [LifecycleVerb] {
        match owner {
            OwnedOperation::Rebase => &[Self::Abort, Self::Continue, Self::Skip],
            OwnedOperation::Merge | OwnedOperation::CherryPick | OwnedOperation::Revert => {
                &[Self::Abort, Self::Continue]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The port's lifecycle verbs address git by the owner's own subcommand
    /// rather than by a caller-spelled string, and the value the three current
    /// call sites hand the engine is the one `OwnedOperation` owns. This is the
    /// assertion the narrowed parameter is load-bearing: a change that made
    /// any of these differ would send a spell out to the engine that git then
    /// refuses.
    #[test]
    fn the_lifecycle_verbs_address_git_by_the_owner_s_own_subcommand() {
        assert_eq!(OwnedOperation::Rebase.git_subcommand(), "rebase");
        assert_eq!(OwnedOperation::CherryPick.git_subcommand(), "cherry-pick");
        assert_eq!(OwnedOperation::Merge.git_subcommand(), "merge");
    }

    /// Every owner the verb table hands Abort / Continue out to is an owner the
    /// narrowed parameter can name: the four `OwnedOperation` variants are the
    /// whole state space the two verbs accept.
    #[test]
    fn every_owner_offering_abort_or_continue_is_nameable_by_the_value_the_port_carries() {
        for owner in [
            OwnedOperation::Rebase,
            OwnedOperation::CherryPick,
            OwnedOperation::Merge,
            OwnedOperation::Revert,
        ] {
            let offered = LifecycleVerb::offered_for(owner);
            assert!(
                offered.contains(&LifecycleVerb::Abort),
                "{owner:?} must still offer Abort",
            );
            assert!(
                offered.contains(&LifecycleVerb::Continue),
                "{owner:?} must still offer Continue",
            );
        }
        assert!(
            !LifecycleVerb::offered_for(OwnedOperation::Merge).contains(&LifecycleVerb::Skip),
            "merge has no --skip, and the table must not offer one",
        );
    }
}

/// How a [`Operation::Rebase`] is being run.
///
/// The kind exists only to produce the label wording and pick the plan;
/// settlement matches [`OpKind::Rebase`], not a kind. That is what makes the
/// conflict-hand-off miss unrepresentable: a fourth rebase flavour cannot skip
/// an arm it was never named in.
#[derive(Clone, Debug)]
pub enum RebaseKind {
    /// `git rebase <onto>` — the rebase dialog's Standard mode.
    Standard { onto: String },
    /// `git rebase --autosquash <onto>` — the dialog's Autosquash mode.
    Autosquash { onto: String },
    /// `git rebase --onto <new_base> <old_base>` — the dialog's `--onto` mode:
    /// move only the sequence ABOVE `old_base` onto `new_base`. Two fields, not
    /// one, because the cutoff and the destination are different arguments and
    /// swapping them rebases the wrong commits.
    Onto { new_base: String, old_base: String },
    /// `git rebase -i <plan>` — the dialog's Interactive mode. `backup` writes
    /// the pre-rebase safety ref first, which is what the plan editor does and
    /// the dialog's own replay does not.
    Interactive {
        plan: Vec<RebasePlanEntry>,
        backup: bool,
    },
    /// Check out `branch`, then replay it onto `onto` — the branch tree's
    /// "rebase this onto the current branch", whose label names both sides.
    Branch { branch: String, onto: String },
}

/// What a [`Operation::Push`] targets — the dialog's two execution paths,
/// carried by the operation so settlement's classification and the worker's
/// run share one shape.
#[derive(Clone, Debug)]
pub enum PushTargets {
    /// The `ThisRepo` scope: one root with the dialog's explicit Remote/Branch
    /// fields.
    Single {
        root: RootId,
        remote: String,
        branch: String,
    },
    /// Every other scope: the resolved roots, each with its per-call remote
    /// resolution inside `push_roots`.
    Scope(Vec<Root>),
}

/// How a rejected push recovers before its re-push.
#[derive(Clone, Debug)]
pub enum RecoveryMode {
    /// Fetch, then integrate the upstream with the chosen update method —
    /// the "Update and retry" affordance. `UpdateMethod::Rebase` maps onto a
    /// rebasing pull, `Merge` onto a merging pull.
    Update { method: UpdateMethod },
    /// Fetch only — the "Fetch and retry" affordance. A rejected-by-remote or
    /// stale-lease push needs the refreshed remote state, not an integration.
    Fetch,
}

/// The roots a recovery updates before the re-push. Affected-only by default
/// (the conservative documented fallback); "Update all repositories" widens
/// the list to every registered root.
#[derive(Clone, Debug)]
pub struct RecoveryPlan {
    pub mode: RecoveryMode,
    pub roots: Vec<RootId>,
}

/// Everything one push needs to run, as one value.
///
/// The dialog's inputs travel with the operation instead of being captured
/// into a closure, because two consumers replay them: a rejected push re-pushes
/// verbatim, and the no-dialog push verb builds the same request without ever
/// opening the dialog.
#[derive(Clone, Debug)]
pub struct PushRequest {
    pub targets: PushTargets,
    pub force: bool,
    pub tags: bool,
    pub no_verify: bool,
    pub set_upstream: bool,
    pub selected_oldest: Option<String>,
    pub settings: VcsSettings,
}

impl PushRequest {
    /// The `ThisRepo` scope: one root, pushed to the dialog's explicit
    /// Remote/Branch fields.
    pub fn single(root: RootId, remote: String, branch: String, settings: &VcsSettings) -> Self {
        Self::for_targets(
            PushTargets::Single {
                root,
                remote,
                branch,
            },
            settings,
        )
    }

    /// Every scope but `ThisRepo`: the resolved roots, each pushed through
    /// `push_roots`' per-root remote resolution.
    pub fn scope(roots: Vec<Root>, settings: &VcsSettings) -> Self {
        Self::for_targets(PushTargets::Scope(roots), settings)
    }

    fn for_targets(targets: PushTargets, settings: &VcsSettings) -> Self {
        Self {
            targets,
            force: false,
            tags: false,
            no_verify: false,
            set_upstream: false,
            selected_oldest: None,
            settings: settings.clone(),
        }
    }

    /// The roots this push acts on — what a recovery may update before the
    /// re-push, and what the rejection notification names.
    pub fn roots(&self) -> Vec<RootId> {
        match &self.targets {
            PushTargets::Single { root, .. } => vec![root.clone()],
            PushTargets::Scope(roots) => roots.iter().map(|r| r.id.clone()).collect(),
        }
    }
}

/// One-shot work against the Git engine.
type Work = Box<dyn FnOnce(&dyn GitExecutor) -> TgResult<()> + Send>;

/// The slot an [`Operation::ApplyPatch`] writes its outcome into.
///
/// Exists because the apply's report is three-way and the completion event it
/// travels on is two-way. Cloning the handle shares the one record, so the
/// worker writes it and settlement reads it without either owning the other;
/// a poisoned lock still hands back the outcome it was given, because a panic in
/// the middle of a rollback is exactly the case where reading what it recorded
/// matters.
#[derive(Clone, Default)]
pub struct ApplyReport(Arc<Mutex<Option<turbogit_services::patch_apply::ApplyOutcome>>>);

impl ApplyReport {
    /// An armed, empty slot: created before the dispatch, so settlement always
    /// finds a handle to read even under an inline pump.
    pub fn armed() -> Self {
        Self::default()
    }

    /// The worker's write.
    pub(crate) fn record(&self, outcome: turbogit_services::patch_apply::ApplyOutcome) {
        *self.lock() = Some(outcome);
    }

    /// Settlement's read: the outcome, leaving it in place so a second look at
    /// the same completion can still see what happened.
    pub fn take(&self) -> Option<turbogit_services::patch_apply::ApplyOutcome> {
        self.lock().clone()
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<turbogit_services::patch_apply::ApplyOutcome>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A unit of git work and everything the shell needs to know about it.
///
/// Deliberately not `Clone`: [`Operation::Custom`] holds a consumed [`FnOnce`],
/// which is the one variant with nothing to rebuild from. Retrying is therefore
/// a property of the variant set, not of a method — a caller that wants a
/// second attempt constructs the operation again, the way the Shelve
/// confirmation does.
pub enum Operation {
    /// `git fetch` on one root, or on several at once. There is no
    /// single-vs-many flag: the scope the fetch covers *is* the difference, and
    /// the report covers every root in it either way.
    Fetch { roots: Vec<RootId> },
    /// `git merge <target>` from the merge dialog.
    Merge {
        root: RootId,
        target: String,
        opts: MergeOpts,
        /// The dialog's clean-tree method, which `smart_merge` needs to get out
        /// of the way of a dirty worktree.
        clean: CleanTreeMethod,
        /// How many commits the preview said come in, when it knew. `0` paints
        /// no count.
        commits: usize,
    },
    /// A rebase of the current branch, in one of its flavours.
    Rebase {
        root: RootId,
        /// The branch being rewritten; the protected-branch guard reads it
        /// before git is touched.
        branch: String,
        settings: VcsSettings,
        /// Mode-mapped executor flags. The interactive kinds ignore it.
        opts: RebaseOpts,
        kind: RebaseKind,
    },
    /// `git branch -D <name>` — force, because the confirmation already
    /// disclosed what an unmerged delete loses.
    DeleteBranch { root: RootId, name: String },
    /// `git stash push`, then a clean discard of the changes the user chose.
    /// An empty `changes` set is the stash-only shelf.
    Shelve {
        root: RootId,
        changes: Vec<Change>,
        message: String,
    },
    /// Put one shelf's content back: `git stash apply` of the entry the shelve
    /// wrote, or `git stash pop` of it when `remove`.
    ///
    /// The two verbs the reference gives this pair have **opposite keep/remove
    /// defaults** and the names carry the consequence — `Unshelve` keeps,
    /// `Unshelve and Remove` destroys the shelved copy. `remove` is that
    /// difference, and it is also what the settlement reads to decide whether
    /// the shelf's own record goes with the git copy: only a landed
    /// `Unshelve and Remove` may delete the row.
    ///
    /// `patch` is the shelf's other mechanism, chosen by the record rather than by a
    /// new verb: `None` is a row whose content is git's stash entry, `Some` is a row
    /// imported from a **patch file**, whose bytes were never in any working tree and so
    /// were never stashed. Putting those back is an apply, and it runs under this kind —
    /// an unshelve is the act, whichever store held the work. That reuse is what makes
    /// the atomic guarantee carry over: a failed import-unshelve rolls every file it
    /// touched back through the same engine as an apply, and lands on the same failure
    /// arm that keeps the row painted.
    Unshelve {
        root: RootId,
        name: String,
        remove: bool,
        patch: Option<turbogit_domain::model::Patch>,
    },
    /// `git worktree add` — mutates the root's linked-worktree set.
    WorktreeAdd {
        root: RootId,
        path: PathBuf,
        branch: String,
    },
    /// `git worktree remove` — mutates the root's linked-worktree set.
    WorktreeRemove { root: RootId, path: PathBuf },
    /// The index of `root`, written as a commit under `message`; `amend`
    /// rewrites HEAD instead of adding to it.
    ///
    /// It carries no path list on purpose. The window computes the message and
    /// the amend flag, and the granular selection is already in the index, so a
    /// commit that was told which files would be a second, contradicting answer
    /// to what the developer staged.
    ///
    /// A named variant rather than a [`Operation::Custom`] closure because the
    /// `Commit and Push…` chain opens a surface on this operation's
    /// *completion*, and settlement can only gate that on an identity — not on
    /// the word "Commit", which staging's closures do not share and which
    /// ADR-0020 forbids reading as a discriminator.
    ///
    /// It is not a worktree mutation: a commit moves HEAD and the ref log, and
    /// leaves every linked worktree's checked-out path exactly where it was.
    Commit {
        root: RootId,
        message: String,
        amend: bool,
    },
    /// Remove ONE commit from `branch`'s history and replay everything built on
    /// top of it. A rewrite, so it carries the settings the protected-branch
    /// guard reads and the branch that guard names when HEAD is detached.
    DropCommit {
        root: RootId,
        branch: String,
        commit: CommitId,
        settings: VcsSettings,
    },
    /// Rewrite ONE commit's message, leaving its content alone. The same guarded
    /// rewrite as [`Operation::DropCommit`], one plan row apart (ADR-0025).
    RewordCommit {
        root: RootId,
        branch: String,
        commit: CommitId,
        message: String,
        settings: VcsSettings,
    },
    /// `git push` from the Push dialog, from the no-dialog push verb, or from
    /// a rejected push's retry. This is the dispatch unit rather than a
    /// [`Operation::Custom`] closure because rejection classification settles on
    /// the operation's identity, and because a retry
    /// must replay the original request verbatim.
    Push {
        request: PushRequest,
        /// Present exactly when this is a recovery: the update/fetch to run
        /// before the re-push. Its presence is what makes the operation a
        /// [`OpKind::PushRecovery`] rather than a [`OpKind::Push`].
        recover: Option<RecoveryPlan>,
    },
    /// Resolve an operation a root is HOLDING: the status bar's per-root
    /// in-flight item, or the palette's menu route, runs one of the three
    /// [`LifecycleVerb`]s against the [`OwnedOperation`] it announced.
    ///
    /// A named variant rather than a [`Operation::Custom`] closure because the
    /// verb has to settle: a Continue that stops on the next conflict is the same
    /// handoff the original merge or rebase got, and a resolution that fell into
    /// `Other` would report a git failure for what is really a paused operation.
    /// `owner` rides along so the work and the label are computed from the state
    /// the announcement showed, never from a re-read that could have moved.
    Resolve {
        root: RootId,
        owner: OwnedOperation,
        verb: LifecycleVerb,
    },
    /// `Get from Branch` — replace one working-tree file with **the entire
    /// contents it has on `source`**. The reference's fifth integration verb,
    /// reached from the file-level comparison `Show Diff with Working Tree` opens
    /// and confirmed before it runs, because the file it overwrites is the
    /// developer's own unsaved work in the working tree.
    ///
    /// A named variant rather than a [`Operation::Custom`] closure for the same
    /// reason as [`Operation::Resolve`]: the verb has to settle, and settlement
    /// here has two jobs a `Custom` op cannot be trusted with — the durable record
    /// names the path it took and the branch it came from, and the changes window
    /// that offered the verb re-reads its rung list once the take lands. Per root
    /// by construction: one `root`, one `path`, never a fleet.
    GetFileFromBranch {
        root: RootId,
        source: String,
        path: PathBuf,
    },
    /// `Apply patch`: write `patch` into one root's working tree, with the
    /// total-rollback discipline [`turbogit_services::patch_apply`] holds.
    ///
    /// Per root by construction — one `root`, one working tree, one record — the
    /// same shape `GetFileFromBranch` takes, and for the same reason: an apply
    /// that silently widened to every registered root would write the patch N
    /// times and roll back whichever tree failed.
    ///
    /// A named variant rather than a [`Operation::Custom`] closure because the
    /// verb settles, and settles three ways. `report` is why the outcome rides a
    /// slot instead of the `TgResult<()>` a completion event carries: the
    /// [`ApplyOutcome`] names which paths could not be restored and still holds
    /// their recorded bytes, which is the content a `Rollback failed` recovery
    /// has to have in hand to offer its retry at all. The slot is armed before
    /// the dispatch and read by settlement after the completion — the same
    /// "recorded before it runs" discipline as a push's replay request.
    ApplyPatch {
        root: RootId,
        patch: turbogit_domain::model::Patch,
        report: ApplyReport,
    },
    /// Work whose label nothing inspects. Greppable, so the long tail stays
    /// visible instead of silently growing into free-text dispatch again.
    Custom {
        label: String,
        affected: Affected,
        work: Work,
    },
}

impl Operation {
    /// Wrap a one-shot closure as an operation with no identity.
    pub fn custom(
        label: impl Into<String>,
        affected: Affected,
        work: impl FnOnce(&dyn GitExecutor) -> TgResult<()> + Send + 'static,
    ) -> Self {
        Self::Custom {
            label: label.into(),
            affected,
            work: Box::new(work),
        }
    }

    /// The push `request` runs as dispatched — no [`RecoveryPlan`], so it is
    /// an [`OpKind::Push`]. Flag state rides on the request because the
    /// dialog-free verb needs the same defaults as the dialog's.
    pub fn push(request: PushRequest) -> Self {
        Self::Push {
            request,
            recover: None,
        }
    }

    /// The same push preceded by a recovery: the update or fetch
    /// runs first, then the push replays verbatim. The recovery's presence is
    /// what makes this an [`OpKind::PushRecovery`].
    pub fn push_with_recovery(request: PushRequest, plan: RecoveryPlan) -> Self {
        Self::Push {
            request,
            recover: Some(plan),
        }
    }

    /// Shelve the working changes under `message`, discarding `changes` after
    /// the stash has them.
    pub fn shelve(root: RootId, changes: Vec<Change>, message: impl Into<String>) -> Self {
        Self::Shelve {
            root,
            changes,
            message: message.into(),
        }
    }

    /// Put the shelf named `name` back in `root`'s working tree, keeping its
    /// shelved copy unless `remove` — the `Unshelve` / `Unshelve and Remove`
    /// pair in one dispatch unit.
    pub fn unshelve(root: RootId, name: impl Into<String>, remove: bool) -> Self {
        Self::Unshelve {
            root,
            name: name.into(),
            remove,
            patch: None,
        }
    }

    /// The same verb for a shelf row imported from a **patch file**: put the parked
    /// bytes back by applying them, keeping or removing the row as the pair says.
    ///
    /// Not a new verb and deliberately not a new `OpKind` — the act is an unshelve, so
    /// it settles on the arm that already decides whether the shelf's record and its
    /// payload go with it.
    pub fn unshelve_patch(
        root: RootId,
        name: impl Into<String>,
        remove: bool,
        patch: turbogit_domain::model::Patch,
    ) -> Self {
        Self::Unshelve {
            root,
            name: name.into(),
            remove,
            patch: Some(patch),
        }
    }

    /// Drop one commit out of `branch`, replaying its descendants.
    pub fn drop_commit(root: &RootId, branch: &str, commit: &str, settings: &VcsSettings) -> Self {
        Self::DropCommit {
            root: root.clone(),
            branch: branch.to_string(),
            commit: commit.to_string(),
            settings: settings.clone(),
        }
    }

    /// Reword one commit in `branch`, its content untouched.
    pub fn reword_commit(
        root: &RootId,
        branch: &str,
        commit: &str,
        message: &str,
        settings: &VcsSettings,
    ) -> Self {
        Self::RewordCommit {
            root: root.clone(),
            branch: branch.to_string(),
            commit: commit.to_string(),
            message: message.to_string(),
            settings: settings.clone(),
        }
    }

    /// Rebase `branch` onto `onto` with the dialog's mode-mapped flags.
    fn rebase_with(
        root: &RootId,
        branch: &str,
        settings: &VcsSettings,
        opts: &RebaseOpts,
        kind: RebaseKind,
    ) -> Self {
        Self::Rebase {
            root: root.clone(),
            branch: branch.to_string(),
            settings: settings.clone(),
            opts: opts.clone(),
            kind,
        }
    }

    /// `git rebase <onto>` — the rebase dialog's Standard mode.
    pub fn rebase_onto(
        root: &RootId,
        branch: &str,
        onto: &str,
        opts: &RebaseOpts,
        settings: &VcsSettings,
    ) -> Self {
        Self::rebase_with(
            root,
            branch,
            settings,
            opts,
            RebaseKind::Standard {
                onto: onto.to_string(),
            },
        )
    }

    /// `git rebase --autosquash <onto>` — the dialog's Autosquash mode.
    pub fn rebase_autosquash(
        root: &RootId,
        branch: &str,
        onto: &str,
        opts: &RebaseOpts,
        settings: &VcsSettings,
    ) -> Self {
        Self::rebase_with(
            root,
            branch,
            settings,
            opts,
            RebaseKind::Autosquash {
                onto: onto.to_string(),
            },
        )
    }

    /// `git rebase --onto <new base> <old base>` — the dialog's `--onto` mode.
    /// The argument order here is the developer's order (destination first,
    /// cutoff second); the service is what turns them into git's two argv
    /// slots.
    pub fn rebase_with_onto(
        root: &RootId,
        branch: &str,
        new_base: &str,
        old_base: &str,
        opts: &RebaseOpts,
        settings: &VcsSettings,
    ) -> Self {
        Self::rebase_with(
            root,
            branch,
            settings,
            opts,
            RebaseKind::Onto {
                new_base: new_base.to_string(),
                old_base: old_base.to_string(),
            },
        )
    }

    /// `git rebase -i <plan>`. `backup` writes the pre-rebase safety ref first,
    /// which is what the plan editor does and the dialog's replay does not.
    pub fn rebase_interactive(
        root: &RootId,
        branch: &str,
        plan: Vec<RebasePlanEntry>,
        settings: &VcsSettings,
        backup: bool,
    ) -> Self {
        Self::rebase_with(
            root,
            branch,
            settings,
            &RebaseOpts::default(),
            RebaseKind::Interactive { plan, backup },
        )
    }

    /// Check out `branch`, then replay it onto `onto`. The branch tree's
    /// "rebase this onto the current branch"; it carries no flags because the
    /// rewrite is git's plain default.
    pub fn rebase_branch_onto(root: &RootId, branch: &str, onto: &str) -> Self {
        Self::Rebase {
            root: root.clone(),
            branch: branch.to_string(),
            settings: VcsSettings::default(),
            opts: RebaseOpts::default(),
            kind: RebaseKind::Branch {
                branch: branch.to_string(),
                onto: onto.to_string(),
            },
        }
    }

    /// The sentence the toast, the activity feed and the log show. Display
    /// only: nothing selects behaviour on it.
    pub fn label(&self) -> String {
        match self {
            Self::Fetch { roots } => match roots.as_slice() {
                [_] => "Fetch".to_string(),
                many => format!("Fetch · {} repos", many.len()),
            },
            Self::Merge {
                target, commits: n, ..
            } => {
                if *n > 0 {
                    format!("Merge {target} ({n} commits)")
                } else {
                    format!("Merge {target}")
                }
            }
            Self::Rebase { kind, .. } => match kind {
                RebaseKind::Standard { onto } => format!("Rebase onto {onto}"),
                RebaseKind::Autosquash { onto } => format!("Autosquash rebase onto {onto}"),
                // Both bases named: the label is what the toast and the activity
                // feed leave behind, and a move that names only its destination
                // cannot be told apart from the plain rebase that has one.
                RebaseKind::Onto { new_base, old_base } => {
                    format!("Rebase {old_base}.. onto {new_base}")
                }
                RebaseKind::Interactive { .. } => "Interactive rebase".to_string(),
                RebaseKind::Branch { branch, onto } => format!("Rebase {branch} onto {onto}"),
            },
            Self::DeleteBranch { name, .. } => format!("Delete branch {name}"),
            Self::Shelve { .. } => "Shelve".to_string(),
            // The two verbs' names carry their own consequence, so the toast and
            // the activity feed say which of the pair ran and which copy is left.
            Self::Unshelve {
                name, remove: true, ..
            } => {
                format!("Unshelve and Remove '{name}'")
            }
            Self::Unshelve { name, .. } => format!("Unshelve '{name}'"),
            Self::WorktreeAdd { branch, .. } => format!("Add worktree {branch}"),
            Self::WorktreeRemove { .. } => "Remove worktree".to_string(),
            // One word for both `amend` arms: the activity feed has always said
            // "Commit" for an amend, and the message stays out of it exactly as
            // a reword's does.
            Self::Commit { .. } => "Commit".to_string(),
            // The short reference, because that is what the developer read in
            // the menu item they clicked. The reword's new message is content
            // and stays out of the feed.
            Self::DropCommit { commit, .. } => {
                format!("Drop commit {}", crate::state::short_sha(commit))
            }
            Self::RewordCommit { commit, .. } => {
                format!("Reword commit {}", crate::state::short_sha(commit))
            }
            Self::Push { .. } => "Push".to_string(),
            Self::Resolve { owner, verb, .. } => verb.label_for(*owner),
            // The file the developer read on the row, named with the branch it
            // comes from: the verb's whole subject is one path across two sides,
            // so a record that said only "Get from Branch" would not say which.
            Self::GetFileFromBranch { source, path, .. } => {
                let name = path.display_name_lossy_or(&path.to_string_lossy());
                format!("Get {name} from {source}")
            }
            // The count is the fact the record needs: an apply over three files
            // that failed on the second is told apart from a one-file refusal by
            // nothing else in this line, and the durable feed outlives the dialog.
            Self::ApplyPatch { patch, .. } => {
                format!("Apply patch · {} file(s)", patch.files.len())
            }
            Self::Custom { label, .. } => label.clone(),
        }
    }

    /// The roots whose caches and status this operation invalidates on
    /// completion. Owned here so no caller can dispatch work on one root and
    /// refresh another.
    pub fn affected(&self) -> Affected {
        match self {
            Self::Fetch { roots } => match roots.as_slice() {
                [one] => Affected::Root(one.clone()),
                _ => Affected::All,
            },
            // A recovery may update roots beyond the push's own targets
            // ("Update all repositories"), so its invalidation is always the
            // widest scope it could have touched.
            Self::Push {
                request, recover, ..
            } => match (&request.targets, recover) {
                (PushTargets::Single { root, .. }, None) => Affected::Root(root.clone()),
                _ => Affected::All,
            },
            Self::Custom { affected, .. } => affected.clone(),
            Self::Merge { root, .. }
            | Self::Rebase { root, .. }
            | Self::DeleteBranch { root, .. }
            | Self::Shelve { root, .. }
            | Self::Unshelve { root, .. }
            | Self::WorktreeAdd { root, .. }
            | Self::WorktreeRemove { root, .. }
            | Self::Commit { root, .. }
            | Self::DropCommit { root, .. }
            | Self::RewordCommit { root, .. }
            // The in-flight resolution invalidates exactly the root it announced:
            // the item is per root, so the refresh has to be per root too, or one
            // root's abort would redraw every root's caches while leaving the
            // announced marker's own sibling stale.
            | Self::Resolve { root, .. }
            // `Get from Branch` writes one file in one root. The scope is the
            // reference's per-root shape, not a fleet: the verb is reached from a
            // row of one window opened against one branch of one repository, and
            // widening it here would be the multi-root collapse the programme
            // forbids wearing an invalidation rule.
            | Self::GetFileFromBranch { root, .. }
            // An apply writes one root's working tree, so it invalidates exactly
            // that root: the record it writes is the tree's own contents, and a
            // fleet-wide refresh would be an apply that reached past the tree the
            // dialog named.
            | Self::ApplyPatch { root, .. } => Affected::Root(root.clone()),
        }
    }

    /// Whether this operation changes a root's linked-worktree set, which is
    /// what makes [`AppState::dispatch`](crate::state::AppState::dispatch) post
    /// the extra invalidation *after* the work lands (ADR-0019's ordering).
    pub fn mutates_worktrees(&self) -> bool {
        matches!(self, Self::WorktreeAdd { .. } | Self::WorktreeRemove { .. })
    }

    /// The identity settlement matches on.
    pub fn kind(&self) -> OpKind {
        match self {
            Self::Fetch { .. } => OpKind::Fetch,
            Self::Merge { .. } => OpKind::Merge,
            Self::Rebase { .. } => OpKind::Rebase,
            Self::DeleteBranch { .. } => OpKind::DeleteBranch,
            Self::Shelve { .. } => OpKind::Shelve,
            Self::Unshelve { .. } => OpKind::Unshelve,
            Self::WorktreeAdd { .. } => OpKind::WorktreeAdd,
            Self::WorktreeRemove { .. } => OpKind::WorktreeRemove,
            Self::Commit { .. } => OpKind::Commit,
            Self::DropCommit { .. } => OpKind::DropCommit,
            Self::RewordCommit { .. } => OpKind::RewordCommit,
            // The recovery plan's presence is the phase: settlement matches
            // on this identity, not on the label.
            Self::Push {
                recover: Some(_), ..
            } => OpKind::PushRecovery,
            Self::Push { .. } => OpKind::Push,
            Self::Resolve { .. } => OpKind::Resolve,
            Self::GetFileFromBranch { .. } => OpKind::GetFileFromBranch,
            Self::ApplyPatch { .. } => OpKind::ApplyPatch,
            Self::Custom { .. } => OpKind::Other,
        }
    }

    /// Run the work. Consumes the operation, which is why the completion event
    /// carries an [`OpKind`] rather than the operation itself.
    pub fn run(self, vcs: &dyn GitExecutor) -> TgResult<()> {
        match self {
            Self::Fetch { roots } => {
                for root in roots {
                    vcs.fetch(root.as_path(), None)?;
                }
                Ok(())
            }
            Self::Merge {
                root,
                target,
                opts,
                clean,
                ..
            } => turbogit_services::integrate_service::smart_merge(
                vcs,
                root.as_path(),
                &target,
                &opts,
                clean,
            ),
            Self::Rebase {
                root,
                branch,
                settings,
                opts,
                kind,
            } => {
                let path = root.as_path();
                match kind {
                    RebaseKind::Standard { onto } | RebaseKind::Autosquash { onto } => {
                        turbogit_services::integrate_service::rebase_current(
                            vcs, path, &onto, &opts, &settings, &branch,
                        )
                    }
                    RebaseKind::Onto { new_base, old_base } => {
                        turbogit_services::integrate_service::rebase_with_onto(
                            vcs, path, &new_base, &old_base, &opts, &settings, &branch,
                        )
                    }
                    RebaseKind::Interactive {
                        plan,
                        backup: false,
                    } => turbogit_services::integrate_service::rebase_plan(
                        vcs, path, &plan, &settings, &branch,
                    ),
                    RebaseKind::Interactive { plan, backup: true } => {
                        turbogit_services::history_editor::execute_with_backup(
                            vcs, path, &plan, &settings, &branch,
                        )
                    }
                    RebaseKind::Branch { branch: b, onto } => {
                        vcs.branch_checkout(path, &b)?;
                        vcs.rebase(path, &onto, &RebaseOpts::default())
                    }
                }
            }
            Self::DeleteBranch { root, name } => vcs.branch_delete(root.as_path(), &name, true),
            Self::Shelve {
                root,
                changes,
                message,
            } => {
                let path = root.as_path();
                turbogit_services::shelve_stash::stash(vcs, path, &message, false)?;
                // The stash only captures worktree changes; the clean discard
                // follows for the paths the user explicitly chose. Best-effort:
                // a stash failure already errored above.
                let _ = turbogit_services::changes::discard_changes(vcs, path, &changes);
                Ok(())
            }
            Self::Unshelve {
                root,
                name,
                remove,
                patch,
            } => match patch {
                // An imported row: the parked bytes go into THEIR root through the apply
                // engine, and an outcome that is not `Applied` is an error — the state
                // that error names is the engine's titled one, because the guarantee it
                // carries (every file the apply touched is back where it was) is what
                // makes keeping the row the safe answer.
                Some(patch) => {
                    let outcome =
                        turbogit_services::patch_apply::apply_patch(vcs, root.as_path(), &patch);
                    match outcome {
                        turbogit_services::patch_apply::ApplyOutcome::Applied { paths } => {
                            let _ = paths;
                            Ok(())
                        }
                        other => Err(turbogit_domain::error::TgError::Other(format!(
                            "{} — {}",
                            other.title(),
                            other.message()
                        ))),
                    }
                }
                None => turbogit_services::shelve_stash::unshelve(
                    vcs,
                    root.as_path(),
                    &name,
                    // A shelf's own record goes with the git copy only for the
                    // destructive sibling of the pair; the shelf list is dropped on
                    // the success side of settlement, which cannot run for an entry
                    // that never existed.
                    remove,
                )
                .map(|_| ()),
            },
            Self::WorktreeAdd { root, path, branch } => {
                vcs.worktree_add(root.as_path(), &path, &branch, true)
            }
            Self::WorktreeRemove { root, path } => {
                vcs.worktree_remove(root.as_path(), &path, false)
            }
            // The service returns the new commit's id and it is **discarded**:
            // what the completion reports is the operation's own outcome, and
            // substituting the id would change what the toast says.
            Self::Commit {
                root,
                message,
                amend,
            } => {
                let _ =
                    turbogit_services::changes::commit_index(vcs, root.as_path(), &message, amend)?;
                Ok(())
            }
            Self::DropCommit {
                root,
                branch,
                commit,
                settings,
            } => turbogit_services::history_editor::drop_commit(
                vcs,
                root.as_path(),
                &commit,
                &settings,
                &branch,
            ),
            Self::RewordCommit {
                root,
                branch,
                commit,
                message,
                settings,
            } => turbogit_services::history_editor::reword_commit(
                vcs,
                root.as_path(),
                &commit,
                &message,
                &settings,
                &branch,
            ),
            Self::Push { request, recover } => {
                let PushRequest {
                    targets,
                    force,
                    tags,
                    no_verify,
                    set_upstream,
                    selected_oldest,
                    settings,
                } = request;
                // Recovery first: the update or fetch runs before
                // the re-push, so its failure classifies as an update outcome
                // and the push is never attempted over a half-updated tree.
                // A root with no current branch or no upstream has nothing to
                // update — skipping it is not failing it.
                if let Some(plan) = recover {
                    match plan.mode {
                        RecoveryMode::Update { method } => {
                            for id in &plan.roots {
                                let path = id.as_path();
                                let Some(branch) = vcs.current_branch(path)? else {
                                    continue;
                                };
                                if vcs.branch_upstream(path, &branch)?.is_none() {
                                    continue;
                                }
                                turbogit_services::sync_service::update_for_recovery(
                                    vcs, path, method, &settings, &branch,
                                )?;
                            }
                        }
                        RecoveryMode::Fetch => {
                            for id in &plan.roots {
                                vcs.fetch(id.as_path(), None)?;
                            }
                        }
                    }
                }
                match targets {
                    PushTargets::Single {
                        root,
                        remote,
                        branch,
                    } => turbogit_services::sync_service::push(
                        vcs,
                        root.as_path(),
                        &remote,
                        &branch,
                        force,
                        tags,
                        no_verify,
                        set_upstream,
                        selected_oldest.as_deref(),
                        &settings,
                    ),
                    PushTargets::Scope(roots) => {
                        let refs: Vec<&Root> = roots.iter().collect();
                        let results = turbogit_services::sync_service::push_roots(
                            vcs,
                            &refs,
                            &settings,
                            force,
                            tags,
                            no_verify,
                            set_upstream,
                            selected_oldest.as_deref(),
                        );
                        // One error per failing root, root-named, so the
                        // rejection classifier can read git's markers out of
                        // the aggregated text and the toast can name the root.
                        let failures: Vec<String> = results
                            .into_iter()
                            .filter_map(|(id, r)| r.err().map(|e| format!("{}: {e}", id.name())))
                            .collect();
                        if failures.is_empty() {
                            Ok(())
                        } else {
                            Err(TgError::Other(format!(
                                "push failed — {}",
                                failures.join("; ")
                            )))
                        }
                    }
                }
            }
            Self::Resolve { root, owner, verb } => {
                let path = root.as_path();
                match verb {
                    LifecycleVerb::Abort => vcs.abort(path, owner),
                    LifecycleVerb::Continue => vcs.continue_op(path, owner),
                    // No `owner` read: the verb table only hands `Skip` out with a
                    // rebase, and re-deriving the state here from `owner` would let
                    // a stale announcement run `git merge --skip`, which git has no
                    // such flag for.
                    LifecycleVerb::Skip => vcs.rebase_skip(path),
                }
            }
            Self::GetFileFromBranch { root, source, path } => {
                turbogit_services::integrate_service::get_file_from_branch(
                    vcs,
                    root.as_path(),
                    &source,
                    &path,
                )
            }
            // The apply records its own three-way outcome before it answers, and
            // the `TgResult` it returns is only the two-way summary the generic
            // completion path reads: a patch that landed is `Ok`, one that did not
            // — rolled back or not — is `Err` carrying git's reason. Which of the
            // two failures it was is the slot's business, and settlement reads it
            // there rather than out of prose.
            Self::ApplyPatch {
                root,
                patch,
                report,
            } => {
                let outcome =
                    turbogit_services::patch_apply::apply_patch(vcs, root.as_path(), &patch);
                let failure = turbogit_services::patch_apply::error_of(&outcome);
                report.record(outcome);
                match failure {
                    None => Ok(()),
                    Some(e) => Err(e),
                }
            }
            Self::Custom { work, .. } => work(vcs),
        }
    }
}
