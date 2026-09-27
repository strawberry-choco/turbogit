//! Operation (CONTEXT.md "Operation"): the unit the **Shell** dispatches.
//!
//! One typed value carrying what to run, its display label, its invalidation
//! [`Affected`] scope, whether it changes a root's worktree set, and — for the
//! operations that have one — its identity. Settlement matches on that identity
//! ([`OpKind`]) rather than reverse-engineering it from the label prose, so a
//! reworded message cannot change what the app does and a new operation that
//! forgot its settlement arm is a compile error.

use std::path::PathBuf;

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{
    Change, CleanTreeMethod, CommitId, MergeOpts, RebaseOpts, RebasePlanEntry, RootId, VcsSettings,
};
use turbogit_engine_api::GitExecutor;

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
    WorktreeAdd,
    WorktreeRemove,
    /// [`Operation::DropCommit`] — one commit removed from the current branch's
    /// history, its descendants replayed.
    DropCommit,
    /// [`Operation::RewordCommit`] — one commit's message rewritten.
    RewordCommit,
    /// The [`Operation::Custom`] tail: dispatched, but nothing settles on it.
    Other,
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

    /// Shelve the working changes under `message`, discarding `changes` after
    /// the stash has them.
    pub fn shelve(root: RootId, changes: Vec<Change>, message: impl Into<String>) -> Self {
        Self::Shelve {
            root,
            changes,
            message: message.into(),
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
                RebaseKind::Interactive { .. } => "Interactive rebase".to_string(),
                RebaseKind::Branch { branch, onto } => format!("Rebase {branch} onto {onto}"),
            },
            Self::DeleteBranch { name, .. } => format!("Delete branch {name}"),
            Self::Shelve { .. } => "Shelve".to_string(),
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
            Self::Custom { affected, .. } => affected.clone(),
            Self::Merge { root, .. }
            | Self::Rebase { root, .. }
            | Self::DeleteBranch { root, .. }
            | Self::Shelve { root, .. }
            | Self::WorktreeAdd { root, .. }
            | Self::WorktreeRemove { root, .. }
            | Self::DropCommit { root, .. }
            | Self::RewordCommit { root, .. } => Affected::Root(root.clone()),
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
            Self::WorktreeAdd { .. } => OpKind::WorktreeAdd,
            Self::WorktreeRemove { .. } => OpKind::WorktreeRemove,
            Self::DropCommit { .. } => OpKind::DropCommit,
            Self::RewordCommit { .. } => OpKind::RewordCommit,
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
            Self::Custom { work, .. } => work(vcs),
        }
    }
}
