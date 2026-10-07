//! Operation (CONTEXT.md "Operation"): the unit the **Shell** dispatches.
//!
//! One typed value carrying what to run, its display label, its invalidation
//! [`Affected`] scope, whether it changes a root's worktree set, and — for the
//! operations that have one — its identity. Settlement matches on that identity
//! ([`OpKind`]) rather than reverse-engineering it from the label prose, so a
//! reworded message cannot change what the app does and a new operation that
//! forgot its settlement arm is a compile error.

use std::path::PathBuf;

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{
    Change, CleanTreeMethod, CommitId, MergeOpts, RebaseOpts, RebasePlanEntry, Root, RootId,
    UpdateMethod, VcsSettings,
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
    /// [`Operation::DropCommit`] — one commit removed from the current branch's
    /// history, its descendants replayed.
    DropCommit,
    /// [`Operation::RewordCommit`] — one commit's message rewritten.
    RewordCommit,
    /// [`Operation::Push`] — a push dispatched from the Push dialog. Its own
    /// identity so settlement can classify a rejection (ticket 13) instead of
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

/// How a rejected push recovers before its re-push (ticket 13).
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
/// verbatim (ticket 13), and the no-dialog push verb builds the same request
/// without ever opening the dialog (ticket 14).
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
    Unshelve {
        root: RootId,
        name: String,
        remove: bool,
    },
    /// `git worktree add` — mutates the root's linked-worktree set.
    WorktreeAdd {
        root: RootId,
        path: PathBuf,
        branch: String,
    },
    /// `git worktree remove` — mutates the root's linked-worktree set.
    WorktreeRemove { root: RootId, path: PathBuf },
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
    /// [`Operation::Custom`] closure because the rejection classification
    /// (ticket 13) settles on the operation's identity, and because a retry
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
    /// an [`OpKind::Push`]. Flag state rides on the request (ticket 14's
    /// dialog-free verb needs the same defaults as the dialog's).
    pub fn push(request: PushRequest) -> Self {
        Self::Push {
            request,
            recover: None,
        }
    }

    /// The same push preceded by a recovery (ticket 13): the update or fetch
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
            | Self::DropCommit { root, .. }
            | Self::RewordCommit { root, .. }
            // The in-flight resolution invalidates exactly the root it announced:
            // the item is per root, so the refresh has to be per root too, or one
            // root's abort would redraw every root's caches while leaving the
            // announced marker's own sibling stale.
            | Self::Resolve { root, .. } => Affected::Root(root.clone()),
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
            Self::DropCommit { .. } => OpKind::DropCommit,
            Self::RewordCommit { .. } => OpKind::RewordCommit,
            // The recovery plan's presence is the phase: settlement matches
            // on this identity, not on the label.
            Self::Push {
                recover: Some(_), ..
            } => OpKind::PushRecovery,
            Self::Push { .. } => OpKind::Push,
            Self::Resolve { .. } => OpKind::Resolve,
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
            Self::Unshelve { root, name, remove } => turbogit_services::shelve_stash::unshelve(
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
            Self::WorktreeAdd { root, path, branch } => {
                vcs.worktree_add(root.as_path(), &path, &branch, true)
            }
            Self::WorktreeRemove { root, path } => {
                vcs.worktree_remove(root.as_path(), &path, false)
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
                // Recovery first (ticket 13): the update or fetch runs before
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
                                vcs.fetch(path, None)?;
                                vcs.pull(path, method == UpdateMethod::Rebase)?;
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
                    LifecycleVerb::Abort => vcs.abort(path, owner.git_subcommand()),
                    LifecycleVerb::Continue => vcs.continue_op(path, owner.git_subcommand()),
                    // No `owner` read: the verb table only hands `Skip` out with a
                    // rebase, and re-deriving the state here from `owner` would let
                    // a stale announcement run `git merge --skip`, which git has no
                    // such flag for.
                    LifecycleVerb::Skip => vcs.rebase_skip(path),
                }
            }
            Self::Custom { work, .. } => work(vcs),
        }
    }
}
