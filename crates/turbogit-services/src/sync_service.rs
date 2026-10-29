use std::collections::HashSet;
use std::path::Path;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SubsetPushState {
    None,
    #[default]
    Full,
    /// The selection is a proper suffix of the outgoing list (newest
    /// commits); `oldest` carries the SHA to use as the refspec base.
    Suffix {
        oldest: String,
    },
    OlderUnchecked,
}
fn matches_pattern(pat: &str, branch: &str) -> bool {
    if pat == "*" {
        true
    } else if let Some(prefix) = pat.strip_suffix('*') {
        branch.starts_with(prefix)
    } else if let Some(suffix) = pat.strip_prefix('*') {
        branch.ends_with(suffix)
    } else {
        pat == branch
    }
}

pub fn is_protected(settings: &VcsSettings, branch: &str) -> bool {
    settings
        .protected_branch_patterns
        .iter()
        .any(|pat| matches_pattern(pat, branch))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PushScope {
    ThisRepo,
    Selection,
    Subtree,
    #[default]
    All,
}

/// Resolve `scope` to the roots a batch push covers, in registration order.
/// `project_dir` bounds [`PushScope::Subtree`]; `selected` bounds
/// [`PushScope::ThisRepo`]; `selection` bounds [`PushScope::Selection`].
pub fn roots_in_scope<'a>(
    scope: PushScope,
    roots: &'a [Root],
    selection: &HashSet<RootId>,
    selected: Option<&RootId>,
    project_dir: &Path,
) -> Vec<&'a Root> {
    roots
        .iter()
        .filter(|root| match scope {
            PushScope::ThisRepo => selected == Some(&root.id),
            PushScope::Selection => selection.contains(&root.id),
            PushScope::Subtree => root.path.starts_with(project_dir),
            PushScope::All => true,
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedRoot {
    pub id: RootId,
    pub name: String,
    pub branch: String,
    pub patterns: Vec<String>,
}

/// Roots whose current branch is protected per `settings.protected_branch_patterns`
/// (see [`is_protected`]). A root with no current branch is never protected.
pub fn protected_roots(settings: &VcsSettings, roots: &[Root]) -> Vec<ProtectedRoot> {
    roots
        .iter()
        .filter_map(|root| {
            let branch = root.current_branch.as_ref()?;
            let patterns: Vec<String> = settings
                .protected_branch_patterns
                .iter()
                .filter(|pat| matches_pattern(pat, branch))
                .cloned()
                .collect();
            if patterns.is_empty() {
                None
            } else {
                Some(ProtectedRoot {
                    id: root.id.clone(),
                    name: root
                        .path
                        .display_name_lossy_or(&root.path.display().to_string()),
                    branch: branch.clone(),
                    patterns,
                })
            }
        })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DryRunSummary {
    pub remotes: usize,
    pub refs: usize,
    pub rejected: usize,
}

fn count_ref_updates(report: &str) -> usize {
    report.lines().filter(|l| l.contains(" -> ")).count()
}

pub fn summarize_dry_runs(results: &[(String, Result<String, String>)]) -> DryRunSummary {
    let mut remotes: HashSet<&str> = HashSet::new();
    let mut refs = 0;
    let mut rejected = 0;
    for (remote, report) in results {
        remotes.insert(remote.as_str());
        match report {
            Ok(text) => refs += count_ref_updates(text),
            Err(_) => rejected += 1,
        }
    }
    DryRunSummary {
        remotes: remotes.len(),
        refs,
        rejected,
    }
}

pub fn push_remote(root: &Root) -> String {
    tracked_upstream(root)
        .map(|u| u.remote.clone())
        .or_else(|| root.remotes.first().map(|r| r.name.clone()))
        .unwrap_or_else(|| "origin".to_string())
}

pub fn tracked_upstream(root: &Root) -> Option<&Upstream> {
    root.branches
        .iter()
        .find(|b| Some(&b.name) == root.current_branch.as_ref())?
        .tracking
        .as_ref()
}

pub fn push_dry_run_roots(
    vcs: &dyn GitExecutor,
    roots: &[&Root],
    force: bool,
    selected_oldest: Option<&str>,
) -> Vec<(String, TgResult<String>)> {
    roots
        .iter()
        .map(|root| {
            let result = match &root.current_branch {
                Some(branch) => {
                    let _ = selected_oldest;
                    vcs.push_dry_run(&root.path, &push_remote(root), branch, force)
                }
                None => Ok(String::new()),
            };
            (push_remote(root), result)
        })
        .collect()
}

pub fn subset_push_state(outgoing: &[String], selected: &[String]) -> SubsetPushState {
    if outgoing.is_empty() || selected.is_empty() {
        return SubsetPushState::None;
    }
    if selected.len() == outgoing.len() {
        return SubsetPushState::Full;
    }
    let Some(oldest_idx) = outgoing.iter().position(|c| selected.contains(c)) else {
        return SubsetPushState::None;
    };
    let Some(oldest_sha) = outgoing.get(oldest_idx).cloned() else {
        return SubsetPushState::None;
    };
    for c in &outgoing[..oldest_idx] {
        if !selected.contains(c) {
            return SubsetPushState::OlderUnchecked;
        }
    }
    SubsetPushState::Suffix { oldest: oldest_sha }
}

/// Which phase a push-path operation was in when it failed. The
/// push itself, or the recovery work (update / fetch) a rejection affordance
/// ran before its re-push. The phase decides whether an unmarked failure is
/// read as a rejection or as the update's own error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushPhase {
    Push,
    Recovery,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    Rejected { remote: String, branch: String },
    RejectedByRemote { remote: String, branch: String },
    LeaseRejected { remote: String, branch: String },
    UpdateConflicted,
    UpdateUnresolved,
    UpdateErrored { detail: String },
}

/// Fresh-conflict markers in git's update output: the strings it prints when
/// an update STOPS on a conflict it just produced. A refusal over conflicts
/// that were already in the worktree prints none of these, which is the
/// distinction between `UpdateConflicted` and `UpdateUnresolved`.
const CONFLICT_STOP_MARKERS: [&str; 3] = ["CONFLICT", "Automatic merge failed", "could not apply"];

/// git's marker for the remote refusing the update itself (hooks, perms).
const REMOTE_REJECTED_MARKER: &str = "[remote rejected]";

/// git's marker for a rejected ref update. Both an ordinary non-fast-forward
/// and a stale lease print it, so it is the LAST rejection marker checked.
const REJECTED_MARKER: &str = "[rejected]";

/// Classify a failed push-path operation into one of the six outcome classes.
/// `err` is the error the executor surfaced; its text carries
/// the verbatim git stderr on `TgError::Cli`. `conflicts_present` says
/// whether the affected root's freshly refreshed status still lists
/// conflicted paths, the worktree, not the stderr, is the reliable signal
/// that conflicts exist (the same rule the merge/rebase settlement uses).
///
/// Order matters: the conflict rules win over the rejection markers because a
/// recovery that stopped on conflicts never reached its re-push, so a
/// rejection marker in the text belongs to the update, not to a push.
/// A recovery-phase failure that matches no marker at all is the update's own
/// error; a push-phase failure that matches no marker is still a push that
/// did not succeed, so it lands in the generic `Rejected` class and the
/// verbatim detail stays in the toast and the activity feed.
pub fn classify_push_failure(
    err: &TgError,
    conflicts_present: bool,
    phase: PushPhase,
    remote: &str,
    branch: &str,
) -> PushOutcome {
    let text = err.to_string();
    if conflicts_present {
        if CONFLICT_STOP_MARKERS.iter().any(|m| text.contains(m)) {
            return PushOutcome::UpdateConflicted;
        }
        return PushOutcome::UpdateUnresolved;
    }
    if text.contains("(stale info)") {
        return PushOutcome::LeaseRejected {
            remote: remote.to_string(),
            branch: branch.to_string(),
        };
    }
    if text.contains(REMOTE_REJECTED_MARKER) {
        return PushOutcome::RejectedByRemote {
            remote: remote.to_string(),
            branch: branch.to_string(),
        };
    }
    if text.contains(REJECTED_MARKER) || phase == PushPhase::Push {
        return PushOutcome::Rejected {
            remote: remote.to_string(),
            branch: branch.to_string(),
        };
    }
    let detail = match err {
        TgError::Cli { stderr, .. } => stderr.trim().to_string(),
        other => other.to_string(),
    };
    PushOutcome::UpdateErrored { detail }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushTargetState {
    EmptyRepository,
    New,
    NewAndUpstream,
    SetUpstream,
    Upstream,
}

impl PushTargetState {
    pub fn label(self) -> &'static str {
        match self {
            Self::EmptyRepository => "Empty repository",
            Self::New => "New",
            Self::NewAndUpstream => "New & Upstream",
            Self::SetUpstream => "Set upstream",
            Self::Upstream => "Upstream",
        }
    }
}

pub fn repository_is_empty(root: &Root) -> bool {
    root.branches.iter().all(|b| b.kind != BranchKind::Local)
}

/// Name the target state of a push of `branch` to `remote` from `root`, with
/// `set_upstream` as the panel's checkbox. `remote` is the panel's own field,
/// not [`push_remote`]'s resolution: the state has to describe the ref the
/// push would actually write, and an edited field changes that.
///
/// `None` is "there is nothing to name yet": no branch typed, or no remote to
/// target. Both are the panel's own problem, the remote-less case paints the
/// inline `Define remote` link in place of the remote name, and an empty branch
/// field has no target to name, so this stays a total function over the states
/// rather than inventing a sixth one for "not applicable".
pub fn push_target_state(
    root: &Root,
    remote: &str,
    branch: &str,
    set_upstream: bool,
) -> Option<PushTargetState> {
    if repository_is_empty(root) {
        return Some(PushTargetState::EmptyRepository);
    }
    if root.remotes.is_empty() || branch.trim().is_empty() || remote.trim().is_empty() {
        return None;
    }
    let expected = if remote == "." {
        branch.to_string()
    } else {
        format!("{remote}/{branch}")
    };
    let on_remote = root
        .branches
        .iter()
        .any(|b| b.kind == BranchKind::Remote && b.name == expected)
        || root.branches.iter().any(|b| {
            b.kind == BranchKind::Local
                && b.name == branch
                && b.tracking.as_ref().is_some_and(|t| t.remote == remote)
        });
    Some(match (on_remote, set_upstream) {
        (false, false) => PushTargetState::New,
        (false, true) => PushTargetState::NewAndUpstream,
        (true, true) => PushTargetState::SetUpstream,
        (true, false) => PushTargetState::Upstream,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectPushTarget {
    pub root: RootId,
    pub remote: String,
    pub branch: String,
    pub set_upstream: bool,
}

/// The target of the no-dialog push, or `None` when the push MUST go through
/// the dialog.
///
/// Four gates, each one a case where a single click cannot say what it will do:
/// more than one root (which roots?, that is the dialog's scope control), a
/// subset of roots checked in the tree (the dialog names the scope in its
/// button), a protected branch (the dialog's force-push block and remediation
/// banner live there), and no remote at all (nothing to push to; the dialog is
/// where `Define remote` appears). A fast path offered outside its gates is
/// worse than no fast path.
pub fn direct_push_target(
    roots: &[Root],
    selected: Option<&RootId>,
    selection: &HashSet<RootId>,
    settings: &VcsSettings,
) -> Option<DirectPushTarget> {
    if roots.len() != 1 {
        return None;
    }
    let root = roots.first()?;
    if selected != Some(&root.id) {
        return None;
    }
    if !selection.is_empty() && (selection.len() != roots.len() || !selection.contains(&root.id)) {
        return None;
    }
    if root.remotes.is_empty() || repository_is_empty(root) {
        return None;
    }
    let branch = root.current_branch.as_ref()?;
    if is_protected(settings, branch) {
        return None;
    }
    Some(DirectPushTarget {
        root: root.id.clone(),
        remote: push_remote(root),
        branch: branch.clone(),
        // git's own `push.default = simple` refuses a first push of an
        // untracked branch, so the fast path records the upstream exactly when
        // there is nothing to record it against.
        set_upstream: tracked_upstream(root).is_none(),
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateChoice {
    #[default]
    AffectedOnly,
    AllRepositories,
}

impl UpdateChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::AffectedOnly => "Update affected repositories only",
            Self::AllRepositories => "Update all repositories",
        }
    }
}

/// Fetch from the given remote (or all remotes when `remote` is `None`).
pub fn fetch(vcs: &dyn GitExecutor, root: &Path, remote: Option<&str>) -> TgResult<()> {
    vcs.fetch(root, remote)
}
pub fn pull(vcs: &dyn GitExecutor, root: &Path, rebase: bool) -> TgResult<()> {
    vcs.pull(root, rebase)
}

/// Pull the way a developer asking to pull is answered, the guarded form of
/// [`pull`], paying the two dues a rebasing update owes and a merging one does
/// not: a protected branch refuses before anything runs, and a rebase records
/// the tip it is about to rewrite.
///
/// `branch` is the branch the update lands on, when the caller already knows it
/// (a `Checkout and pull` names it from the row it clicked). The repository's own
/// HEAD wins when it can be read, because it is the fact rather than the
/// caller's belief about it, and a caller that checked a branch out before
/// calling here has HEAD as exactly that branch. With neither, a detached HEAD,
/// there is no branch to guard, and so no rewrite to allow either.
///
/// **The order is load-bearing and compiles either way.**
/// [`crate::integrate_service::ensure_rebase_allowed`] must refuse BEFORE
/// [`GitExecutor::save_rewrite_backup`]: one backup reference exists per
/// repository, it is the Undo verb's only target, and a refusal that had already
/// written it would overwrite the record of an earlier rewrite that succeeded.
/// And the backup must be written BEFORE the pull, or the rebase rewrites history
/// with nothing to undo. That is why a rebasing pull cannot be inlined as a bare
/// `vcs.pull(root, rebase)` at a call site, which is precisely what the shell's
/// pull verbs used to do.
pub fn pull_guarded(
    vcs: &dyn GitExecutor,
    root: &Path,
    method: UpdateMethod,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    let rebase = method == UpdateMethod::Rebase;
    if !rebase {
        return pull(vcs, root, false);
    }
    let target = vcs
        .current_branch(root)?
        .unwrap_or_else(|| branch.to_string());
    if target.is_empty() {
        return Err(TgError::Other(
            "detached HEAD — a rebasing update has no branch to rewrite".to_string(),
        ));
    }
    crate::integrate_service::ensure_rebase_allowed(settings, &target)?;
    vcs.save_rewrite_backup(root)?;
    pull(vcs, root, true)
}

pub fn update_for_recovery(
    vcs: &dyn GitExecutor,
    root: &Path,
    method: UpdateMethod,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    if method == UpdateMethod::Rebase {
        crate::integrate_service::ensure_rebase_allowed(settings, branch)?;
    }
    fetch(vcs, root, None)?;
    pull_guarded(vcs, root, method, settings, branch)
}
#[allow(clippy::too_many_arguments)]
pub fn push(
    vcs: &dyn GitExecutor,
    root: &Path,
    remote: &str,
    branch: &str,
    force: bool,
    tags: bool,
    no_verify: bool,
    set_upstream: bool,
    selected_oldest: Option<&str>,
    settings: &VcsSettings,
) -> TgResult<()> {
    if force && is_protected(settings, branch) {
        return Err(TgError::Other(format!(
            "Refusing force-push to protected branch '{}'",
            branch
        )));
    }
    vcs.push(
        root,
        remote,
        branch,
        force,
        tags,
        no_verify,
        set_upstream,
        selected_oldest,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn push_all_forced(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
    force: bool,
    tags: bool,
    no_verify: bool,
    set_upstream: bool,
    selected_oldest: Option<&str>,
) -> Vec<(RootId, TgResult<()>)> {
    let refs: Vec<&Root> = mgr.roots.iter().collect();
    push_roots(
        vcs,
        &refs,
        settings,
        force,
        tags,
        no_verify,
        set_upstream,
        selected_oldest,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn push_roots(
    vcs: &dyn GitExecutor,
    roots: &[&Root],
    settings: &VcsSettings,
    force: bool,
    tags: bool,
    no_verify: bool,
    set_upstream: bool,
    selected_oldest: Option<&str>,
) -> Vec<(RootId, TgResult<()>)> {
    roots
        .iter()
        .map(|root| {
            let result = match &root.current_branch {
                Some(branch) => push(
                    vcs,
                    &root.path,
                    &push_remote(root),
                    branch,
                    force,
                    tags,
                    no_verify,
                    set_upstream,
                    selected_oldest,
                    settings,
                ),
                None => Ok(()),
            };
            (root.id.clone(), result)
        })
        .collect()
}
pub fn push_all(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
) -> Vec<(RootId, TgResult<()>)> {
    push_all_forced(vcs, mgr, settings, false, false, false, false, None)
}
pub fn outgoing_per_root(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
) -> Vec<(RootId, TgResult<Vec<CommitId>>)> {
    mgr.roots
        .iter()
        .map(|root| {
            let result = match &root.current_branch {
                Some(branch) => match tracked_upstream(root) {
                    Some(upstream) => vcs.outgoing_commits(&root.path, branch, &upstream.git_ref()),
                    None => Ok(Vec::new()),
                },
                None => Ok(Vec::new()),
            };
            (root.id.clone(), result)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use turbogit_engine::fake::{Call, FakeExecutor};

    fn settings_with(patterns: &[&str]) -> VcsSettings {
        VcsSettings {
            protected_branch_patterns: patterns.iter().map(|s| s.to_string()).collect(),
            ..VcsSettings::default()
        }
    }

    #[test]
    fn is_protected_matches_exact_prefix_suffix_and_wildcard() {
        let s = settings_with(&["main", "release/*", "*/hotfix"]);
        assert!(is_protected(&s, "main"));
        assert!(is_protected(&s, "release/1.0"));
        assert!(is_protected(&s, "eu/hotfix"));
        assert!(!is_protected(&s, "hotfix"), "suffix must start at */");
        let wild = settings_with(&["*"]);
        assert!(is_protected(&wild, "anything/at/all"));
    }

    fn rejected_stderr() -> String {
        "To /repo/remote.git\n ! [rejected]        main -> main (non-fast-forward)\n".into()
    }

    #[test]
    fn ordinary_rejection_classifies_as_rejected() {
        let err = TgError::Cli {
            code: 1,
            stderr: rejected_stderr(),
        };
        let out = classify_push_failure(&err, false, PushPhase::Push, "origin", "main");
        assert_eq!(
            out,
            PushOutcome::Rejected {
                remote: "origin".into(),
                branch: "main".into(),
            }
        );
    }

    #[test]
    fn remote_rejected_marker_classifies_as_rejected_by_remote() {
        let err = TgError::Cli {
            code: 1,
            stderr: " ! [remote rejected]  main -> main (pre-receive hook declined)\n".into(),
        };
        let out = classify_push_failure(&err, false, PushPhase::Push, "origin", "main");
        assert_eq!(
            out,
            PushOutcome::RejectedByRemote {
                remote: "origin".into(),
                branch: "main".into(),
            }
        );
    }

    #[test]
    fn stale_lease_marker_classifies_as_lease_rejected() {
        let err = TgError::Cli {
            code: 1,
            stderr: " ! [rejected]        main -> main (stale info)\n".into(),
        };
        let out = classify_push_failure(&err, false, PushPhase::Push, "origin", "main");
        assert_eq!(
            out,
            PushOutcome::LeaseRejected {
                remote: "origin".into(),
                branch: "main".into(),
            }
        );
    }

    #[test]
    fn conflicted_recovery_update_classifies_as_update_conflicted() {
        let err = TgError::Cli {
            code: 1,
            stderr: "Auto-merging base.txt\nCONFLICT (content): Merge conflict in base.txt\n\
                   Automatic merge failed; fix conflicts and then commit the result.\n"
                .into(),
        };
        let out = classify_push_failure(&err, true, PushPhase::Recovery, "origin", "main");
        assert_eq!(out, PushOutcome::UpdateConflicted);
    }

    #[test]
    fn refusal_with_conflicts_still_in_the_tree_classifies_as_update_unresolved() {
        let err = TgError::Cli {
            code: 128,
            stderr: "error: You have not concluded your merge (MERGE_HEAD exists).\n".into(),
        };
        let out = classify_push_failure(&err, true, PushPhase::Recovery, "origin", "main");
        assert_eq!(out, PushOutcome::UpdateUnresolved);
    }

    #[test]
    fn recovery_failure_without_conflicts_classifies_as_update_errored() {
        let err = TgError::Cli {
            code: 128,
            stderr: "fatal: '/repo/remote.git' does not appear to be a git repository\n".into(),
        };
        let out = classify_push_failure(&err, false, PushPhase::Recovery, "origin", "main");
        assert_eq!(
            out,
            PushOutcome::UpdateErrored {
                detail: "fatal: '/repo/remote.git' does not appear to be a git repository".into(),
            }
        );
    }

    #[test]
    fn a_rejected_repush_after_a_clean_update_is_a_rejection_not_an_update_failure() {
        let err = TgError::Cli {
            code: 1,
            stderr: rejected_stderr(),
        };
        let out = classify_push_failure(&err, false, PushPhase::Recovery, "origin", "main");
        assert!(matches!(out, PushOutcome::Rejected { .. }));
    }

    #[test]
    fn a_push_phase_failure_never_lands_in_an_update_class() {
        let err = TgError::Cli {
            code: 128,
            stderr: "fatal: Could not read from remote repository\n".into(),
        };
        let out = classify_push_failure(&err, false, PushPhase::Push, "origin", "main");
        assert!(matches!(out, PushOutcome::Rejected { .. }));
    }

    #[test]
    fn update_classes_win_over_rejection_markers_when_conflicts_stay() {
        let err = TgError::Cli {
            code: 1,
            stderr: "CONFLICT (content): Merge conflict in base.txt\n\
                   Automatic merge failed; fix conflicts and then commit the result.\n"
                .into(),
        };
        let out = classify_push_failure(&err, true, PushPhase::Recovery, "origin", "main");
        assert_eq!(out, PushOutcome::UpdateConflicted);
    }

    fn target_root(remotes: &[&str], on_remote: &[&str], has_commits: bool, tracked: bool) -> Root {
        let path = PathBuf::from("/repo");
        let mut branches: Vec<Branch> = if has_commits {
            vec![Branch {
                name: "feat".into(),
                kind: BranchKind::Local,
                tracking: tracked.then(|| Upstream {
                    remote: "origin".into(),
                    branch: "feat".into(),
                }),
                favorite: false,
                protected: false,
                exists: true,
                ahead: 0,
                behind: 0,
                gone: false,
                last_touched: None,
                tip: None,
                remote: None,
            }]
        } else {
            Vec::new()
        };
        branches.extend(on_remote.iter().map(|name| Branch {
            name: (*name).to_string(),
            kind: BranchKind::Remote,
            tracking: None,
            favorite: false,
            protected: false,
            exists: true,
            ahead: 0,
            behind: 0,
            gone: false,
            last_touched: None,
            tip: None,
            remote: None,
        }));
        Root {
            id: RootId(path.clone().into()),
            path,
            remotes: remotes
                .iter()
                .map(|name| Remote {
                    name: (*name).to_string(),
                    fetch_url: Some(String::new()),
                    push_url: None,
                })
                .collect(),
            branches,
            current_branch: has_commits.then(|| "feat".to_string()),
            head: Some("abc".to_string()),
            status: RootStatus::default(),
        }
    }

    #[test]
    fn a_branch_absent_on_the_remote_is_named_new_and_upstream_alongside_it() {
        let root = target_root(&["origin"], &[], true, false);
        assert_eq!(
            push_target_state(&root, "origin", "feat", false),
            Some(PushTargetState::New)
        );
        assert_eq!(
            push_target_state(&root, "origin", "feat", true),
            Some(PushTargetState::NewAndUpstream)
        );
        assert_eq!(PushTargetState::NewAndUpstream.label(), "New & Upstream");
    }

    #[test]
    fn a_branch_already_on_the_remote_is_upstream_or_set_upstream() {
        let root = target_root(&["origin"], &["origin/feat"], true, true);
        assert_eq!(
            push_target_state(&root, "origin", "feat", false),
            Some(PushTargetState::Upstream)
        );
        assert_eq!(
            push_target_state(&root, "origin", "feat", true),
            Some(PushTargetState::SetUpstream)
        );
    }

    #[test]
    fn an_empty_repository_names_itself_and_nothing_is_named_without_a_target() {
        assert_eq!(
            push_target_state(
                &target_root(&["origin"], &[], false, false),
                "origin",
                "feat",
                false
            ),
            Some(PushTargetState::EmptyRepository)
        );
        assert_eq!(
            push_target_state(&target_root(&[], &[], true, false), "origin", "feat", false),
            None
        );
        assert_eq!(
            push_target_state(
                &target_root(&["origin"], &[], true, false),
                "origin",
                "  ",
                false
            ),
            None
        );
    }

    fn selected_id(root: &Root) -> RootId {
        root.id.clone()
    }

    #[test]
    fn a_single_unprotected_root_with_a_remote_takes_the_fast_path() {
        let root = target_root(&["origin"], &["origin/main"], true, true);
        let roots = [root.clone()];
        let none_selected: HashSet<RootId> = HashSet::new();
        let got = direct_push_target(
            &roots,
            Some(&selected_id(&root)),
            &none_selected,
            &settings_with(&["main", "master"]),
        );
        assert_eq!(
            got,
            Some(DirectPushTarget {
                root: root.id.clone(),
                remote: "origin".into(),
                branch: "feat".into(),
                set_upstream: false,
            })
        );
    }

    #[test]
    fn a_first_push_of_an_untracked_branch_records_the_upstream() {
        let root = target_root(&["origin"], &[], true, false);
        let got = direct_push_target(
            std::slice::from_ref(&root),
            Some(&selected_id(&root)),
            &HashSet::new(),
            &settings_with(&["main"]),
        )
        .expect("single untracked root takes the fast path");
        assert!(got.set_upstream, "git's own default refuses the push");
    }

    #[test]
    fn the_fast_path_is_withheld_on_every_case_the_dialog_decides() {
        let settings = settings_with(&["main"]);
        let ok = target_root(&["origin"], &[], true, true);
        let mut protected = ok.clone();
        protected.current_branch = Some("main".into());
        protected.branches[0].name = "main".into();
        let no_remote = target_root(&[], &[], true, true);
        let unborn = target_root(&["origin"], &[], false, true);
        let detached = Root {
            current_branch: None,
            ..ok.clone()
        };
        let other = RootId(PathBuf::from("/other").into());
        let two_roots = vec![
            ok.clone(),
            Root {
                id: other.clone(),
                ..ok.clone()
            },
        ];
        let selected = ok.id.clone();
        let none_selected: HashSet<RootId> = HashSet::new();

        for (case, roots, chosen) in [
            ("protected", vec![protected], Some(selected.clone())),
            ("no remote", vec![no_remote], Some(selected.clone())),
            ("no commits", vec![unborn], Some(selected.clone())),
            ("detached", vec![detached], Some(selected.clone())),
            ("multi-root", two_roots.clone(), Some(selected.clone())),
        ] {
            assert_eq!(
                direct_push_target(&roots, chosen.as_ref(), &none_selected, &settings),
                None,
                "{case} must not get a one-click push"
            );
        }
        let subset: HashSet<RootId> = [other.clone()].into_iter().collect();
        assert_eq!(
            direct_push_target(
                std::slice::from_ref(&ok),
                Some(&selected),
                &subset,
                &settings
            ),
            None,
            "a checked subset must not get a one-click push"
        );
        let whole: HashSet<RootId> = [selected.clone()].into_iter().collect();
        assert!(
            direct_push_target(
                std::slice::from_ref(&ok),
                Some(&selected),
                &whole,
                &settings
            )
            .is_some(),
            "checking the only root selects nothing away from it"
        );
    }

    #[test]
    fn push_refuses_force_to_protected_but_delegates_otherwise() {
        let engine = FakeExecutor::new();
        let s = settings_with(&["main"]);
        let root = PathBuf::from("/repo");

        let blocked = push(
            &engine, &root, "origin", "main", true, false, false, false, None, &s,
        );
        assert!(blocked.is_err(), "force-push to protected branch must fail");
        assert!(
            engine.calls.lock().unwrap().is_empty(),
            "blocked push must not reach the engine"
        );

        let normal = push(
            &engine, &root, "origin", "feature", false, false, false, false, None, &s,
        );
        assert!(normal.is_ok());
        let calls = engine.calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            [Call::Push {
                root: root.clone(),
                remote: "origin".into(),
                branch: "feature".into(),
                force: false,
                tags: false,
                no_verify: false,
                set_upstream: false,
                selected_oldest: None,
            }]
        );
    }
}
