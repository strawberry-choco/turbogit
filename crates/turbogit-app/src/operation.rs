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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Fetch,
    Merge,
    Rebase,
    DeleteBranch,
    Shelve,
    Unshelve,
    WorktreeAdd,
    WorktreeRemove,
    Commit,
    DropCommit,
    RewordCommit,
    Push,
    PushRecovery,
    Resolve,
    GetFileFromBranch,
    ApplyPatch,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleVerb {
    Abort,
    Continue,
    Skip,
}

impl LifecycleVerb {
    pub fn label(self) -> &'static str {
        match self {
            Self::Abort => "Abort",
            Self::Continue => "Continue",
            Self::Skip => "Skip",
        }
    }

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

    #[test]
    fn the_lifecycle_verbs_address_git_by_the_owner_s_own_subcommand() {
        assert_eq!(OwnedOperation::Rebase.label(), "rebase");
        assert_eq!(OwnedOperation::CherryPick.label(), "cherry-pick");
        assert_eq!(OwnedOperation::Merge.label(), "merge");
    }

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

#[derive(Clone, Debug)]
pub enum RebaseKind {
    Standard {
        onto: String,
    },
    Autosquash {
        onto: String,
    },
    /// `git rebase --onto <new_base> <old_base>`, the dialog's `--onto` mode:
    /// move only the sequence ABOVE `old_base` onto `new_base`. Two fields, not
    /// one, because the cutoff and the destination are different arguments and
    /// swapping them rebases the wrong commits.
    Onto {
        new_base: String,
        old_base: String,
    },
    Interactive {
        plan: Vec<RebasePlanEntry>,
        backup: bool,
    },
    Branch {
        branch: String,
        onto: String,
    },
}

#[derive(Clone, Debug)]
pub enum PushTargets {
    Single {
        root: RootId,
        remote: String,
        branch: String,
    },
    Scope(Vec<Root>),
}

#[derive(Clone, Debug)]
pub enum RecoveryMode {
    Update { method: UpdateMethod },
    Fetch,
}

#[derive(Clone, Debug)]
pub struct RecoveryPlan {
    pub mode: RecoveryMode,
    pub roots: Vec<RootId>,
}

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

    pub fn roots(&self) -> Vec<RootId> {
        match &self.targets {
            PushTargets::Single { root, .. } => vec![root.clone()],
            PushTargets::Scope(roots) => roots.iter().map(|r| r.id.clone()).collect(),
        }
    }
}

type Work = Box<dyn FnOnce(&dyn GitExecutor) -> TgResult<()> + Send>;

#[derive(Clone, Default)]
pub struct ApplyReport(Arc<Mutex<Option<turbogit_services::patch_apply::ApplyOutcome>>>);

impl ApplyReport {
    pub fn armed() -> Self {
        Self::default()
    }

    pub(crate) fn record(&self, outcome: turbogit_services::patch_apply::ApplyOutcome) {
        *self.lock() = Some(outcome);
    }

    pub fn take(&self) -> Option<turbogit_services::patch_apply::ApplyOutcome> {
        self.lock().clone()
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<turbogit_services::patch_apply::ApplyOutcome>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub enum Operation {
    Fetch {
        roots: Vec<RootId>,
    },
    Merge {
        root: RootId,
        target: String,
        opts: MergeOpts,
        clean: CleanTreeMethod,
        commits: usize,
    },
    Rebase {
        root: RootId,
        branch: String,
        settings: VcsSettings,
        opts: RebaseOpts,
        kind: RebaseKind,
    },
    DeleteBranch {
        root: RootId,
        name: String,
    },
    Shelve {
        root: RootId,
        changes: Vec<Change>,
        message: String,
    },
    Unshelve {
        root: RootId,
        name: String,
        remove: bool,
        patch: Option<turbogit_domain::model::Patch>,
    },
    WorktreeAdd {
        root: RootId,
        path: PathBuf,
        branch: String,
    },
    WorktreeRemove {
        root: RootId,
        path: PathBuf,
    },
    Commit {
        root: RootId,
        message: String,
        amend: bool,
        no_verify: bool,
    },
    DropCommit {
        root: RootId,
        branch: String,
        commit: CommitId,
        settings: VcsSettings,
    },
    RewordCommit {
        root: RootId,
        branch: String,
        commit: CommitId,
        message: String,
        settings: VcsSettings,
    },
    Push {
        request: PushRequest,
        recover: Option<RecoveryPlan>,
    },
    Resolve {
        root: RootId,
        owner: OwnedOperation,
        verb: LifecycleVerb,
    },
    GetFileFromBranch {
        root: RootId,
        source: String,
        path: PathBuf,
    },
    ApplyPatch {
        root: RootId,
        patch: turbogit_domain::model::Patch,
        report: ApplyReport,
    },
    Custom {
        label: String,
        affected: Affected,
        work: Work,
    },
}

impl Operation {
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

    pub fn push(request: PushRequest) -> Self {
        Self::Push {
            request,
            recover: None,
        }
    }

    pub fn push_with_recovery(request: PushRequest, plan: RecoveryPlan) -> Self {
        Self::Push {
            request,
            recover: Some(plan),
        }
    }

    pub fn shelve(root: RootId, changes: Vec<Change>, message: impl Into<String>) -> Self {
        Self::Shelve {
            root,
            changes,
            message: message.into(),
        }
    }

    pub fn unshelve(root: RootId, name: impl Into<String>, remove: bool) -> Self {
        Self::Unshelve {
            root,
            name: name.into(),
            remove,
            patch: None,
        }
    }

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

    pub fn drop_commit(root: &RootId, branch: &str, commit: &str, settings: &VcsSettings) -> Self {
        Self::DropCommit {
            root: root.clone(),
            branch: branch.to_string(),
            commit: commit.to_string(),
            settings: settings.clone(),
        }
    }

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
                RebaseKind::Onto { new_base, old_base } => {
                    format!("Rebase {old_base}.. onto {new_base}")
                }
                RebaseKind::Interactive { .. } => "Interactive rebase".to_string(),
                RebaseKind::Branch { branch, onto } => format!("Rebase {branch} onto {onto}"),
            },
            Self::DeleteBranch { name, .. } => format!("Delete branch {name}"),
            Self::Shelve { .. } => "Shelve".to_string(),
            Self::Unshelve {
                name, remove: true, ..
            } => {
                format!("Unshelve and Remove '{name}'")
            }
            Self::Unshelve { name, .. } => format!("Unshelve '{name}'"),
            Self::WorktreeAdd { branch, .. } => format!("Add worktree {branch}"),
            Self::WorktreeRemove { .. } => "Remove worktree".to_string(),
            Self::Commit { .. } => "Commit".to_string(),
            Self::DropCommit { commit, .. } => {
                format!("Drop commit {}", crate::state::short_sha(commit))
            }
            Self::RewordCommit { commit, .. } => {
                format!("Reword commit {}", crate::state::short_sha(commit))
            }
            Self::Push { .. } => "Push".to_string(),
            Self::Resolve { owner, verb, .. } => verb.label_for(*owner),
            Self::GetFileFromBranch { source, path, .. } => {
                let name = path.display_name_lossy_or(&path.to_string_lossy());
                format!("Get {name} from {source}")
            }
            Self::ApplyPatch { patch, .. } => {
                format!("Apply patch · {} file(s)", patch.files.len())
            }
            Self::Custom { label, .. } => label.clone(),
        }
    }

    pub fn affected(&self) -> Affected {
        match self {
            Self::Fetch { roots } => match roots.as_slice() {
                [one] => Affected::Root(one.clone()),
                _ => Affected::All,
            },
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
            | Self::Resolve { root, .. }
            | Self::GetFileFromBranch { root, .. }
            | Self::ApplyPatch { root, .. } => Affected::Root(root.clone()),
        }
    }

    /// Whether this operation changes a root's linked-worktree set, which is
    /// what makes [`AppState::dispatch`](crate::state::AppState::dispatch) post
    /// the extra invalidation *after* the work lands (ADR-0019's ordering).
    pub fn mutates_worktrees(&self) -> bool {
        matches!(self, Self::WorktreeAdd { .. } | Self::WorktreeRemove { .. })
    }

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
                let _ = turbogit_services::changes::discard_changes(vcs, path, &changes);
                Ok(())
            }
            Self::Unshelve {
                root,
                name,
                remove,
                patch,
            } => match patch {
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
                None => {
                    turbogit_services::shelve_stash::unshelve(vcs, root.as_path(), &name, remove)
                        .map(|_| ())
                }
            },
            Self::WorktreeAdd { root, path, branch } => {
                vcs.worktree_add(root.as_path(), &path, &branch, true)
            }
            Self::WorktreeRemove { root, path } => {
                vcs.worktree_remove(root.as_path(), &path, false)
            }
            Self::Commit {
                root,
                message,
                amend,
                no_verify,
            } => {
                let _ = turbogit_services::changes::commit_index(
                    vcs,
                    root.as_path(),
                    &message,
                    amend,
                    no_verify,
                )?;
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
