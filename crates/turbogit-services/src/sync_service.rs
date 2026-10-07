//! Remote synchronization service: fetch / pull / push for single roots and
//! aggregated across all roots in a [`MultiRootManager`].
//!
//! Provides protected-branch gating for force-pushes and batch operations that
//! iterate every registered root, recording one result per root.

use std::collections::HashSet;
use std::path::Path;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// Issue #24: classification of the Push dialog's per-commit selection,
/// used to decide whether the dialog can push a true subset or must
/// fall back to the full outgoing list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SubsetPushState {
    /// Nothing selected: the dialog disables Push or runs no-ops.
    None,
    /// Every outgoing commit is selected: the dialog pushes the full list.
    #[default]
    Full,
    /// The selection is a proper suffix of the outgoing list (newest
    /// commits); `oldest` carries the SHA to use as the refspec base.
    Suffix { oldest: String },
    /// The selection has an older unselected ancestor while a newer
    /// commit is still checked. Pushing a true subset is impossible
    /// the dialog falls back to a full push and explains why.
    OlderUnchecked,
}
///
/// Pattern semantics:
/// - `*` matches every branch.
/// - a pattern ending with `*` matches by prefix (e.g. `release/*`).
/// - a pattern starting with `*` matches by suffix (e.g. `*/main`).
/// - otherwise the pattern must match the branch name exactly.
///
/// Whether one pattern matches `branch` (see [`is_protected`] for semantics).
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

/// Issue #25: which roots the Push dialog's PUSH SCOPE control covers
/// (screen 10). Resolved against registered root snapshots by
/// [`roots_in_scope`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PushScope {
    /// Only the currently selected root, via the explicit Remote/Branch
    /// fields (successor to the old "Push current branch only" checkbox).
    ThisRepo,
    /// The workspace tree's checked repos (issue #08 multi-repo selection).
    Selection,
    /// Every root under the project directory.
    Subtree,
    /// Every registered root, wherever it was registered from.
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

/// Issue #25: one root in the push scope whose current branch matches a
/// protected pattern — the data the remediation banner renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedRoot {
    pub id: RootId,
    /// The root's display name (its directory name).
    pub name: String,
    /// The protected branch the root currently sits on.
    pub branch: String,
    /// Every protected pattern the branch matches (usually one).
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
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| root.path.display().to_string()),
                    branch: branch.clone(),
                    patterns,
                })
            }
        })
        .collect()
}

/// Issue #25: aggregate over a scope's per-root dry-run reports — the
/// numbers behind the dialog's "N commits → R remotes · X refs · Y rejected"
/// summary line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DryRunSummary {
    /// Distinct remotes the dry-runs targeted.
    pub remotes: usize,
    /// Ref-update lines parsed from accepted verbatim reports.
    pub refs: usize,
    /// Roots whose dry-run was rejected.
    pub rejected: usize,
}

/// Count a git dry-run report's ref-update lines: lines containing ` -> `
/// (e.g. ` * [new branch] main -> main`).
fn count_ref_updates(report: &str) -> usize {
    report.lines().filter(|l| l.contains(" -> ")).count()
}

/// Aggregate already-captured per-root dry-run results (remote, report).
/// Expected-value literals live in the tests; this is the counting rule.
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

/// Issue #25: run one `git push --dry-run` per root in scope with the SAME
/// per-root remote resolution as [`push_roots`] — the preview must reflect
/// what the real push would target. Root dry-runs are independent: one
/// rejection never hides the others.
/// The remote a push from `root` goes to — the one rule, stated once.
///
/// The tracked upstream's remote; failing that the root's first configured
/// remote; failing that `origin`. This is what the push, the dry-run preview and
/// the bulk fleet all consult, so the "does the dry-run path share the real
/// push's rule" comment is now true by construction rather than by promise.
pub fn push_remote(root: &Root) -> String {
    tracked_upstream(root)
        .map(|u| u.remote.clone())
        .or_else(|| root.remotes.first().map(|r| r.name.clone()))
        .unwrap_or_else(|| "origin".to_string())
}

/// The current branch's upstream, as the **Git engine**'s listing answered it.
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
    // Find the oldest selected commit's index in the outgoing list.
    let Some(oldest_idx) = outgoing.iter().position(|c| selected.contains(c)) else {
        return SubsetPushState::None;
    };
    let Some(oldest_sha) = outgoing.get(oldest_idx).cloned() else {
        return SubsetPushState::None;
    };
    // Suffix rule: every outgoing commit NEWER than the oldest selected
    // (smaller index in newest-first order) must also be selected.
    for c in &outgoing[..oldest_idx] {
        if !selected.contains(c) {
            return SubsetPushState::OlderUnchecked;
        }
    }
    SubsetPushState::Suffix { oldest: oldest_sha }
}

/// Ticket 13: which phase a push-path operation was in when it failed. The
/// push itself, or the recovery work (update / fetch) a rejection affordance
/// ran before its re-push. The phase decides whether an unmarked failure is
/// read as a rejection or as the update's own error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushPhase {
    /// The push command failed.
    Push,
    /// The recovery's update or fetch failed (or its re-push did).
    Recovery,
}

/// Ticket 13: the classified push outcome — six classes, four of which carry
/// a next step (update-and-retry, fetch-and-retry, resolve) and two of which
/// carry none. The classification is read from the git stderr the port
/// already surfaces on `TgError::Cli`; the wording the UI paints per class is
/// TurboGit's own, only the taxonomy is adopted from the parity evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    /// The remote branch has work this push did not account for. Next step:
    /// update, then push again.
    Rejected { remote: String, branch: String },
    /// The remote itself refused the update (hook, permissions, …). Next
    /// step: fetch, then push again.
    RejectedByRemote { remote: String, branch: String },
    /// A `--force-with-lease` push failed on its lease: the recorded
    /// expectation of the remote tip is stale, not the push. Next step:
    /// fetch (refresh the lease), then push again.
    LeaseRejected { remote: String, branch: String },
    /// The recovery update stopped on conflicts. Next step: resolve them,
    /// then push again.
    UpdateConflicted,
    /// The update could not run because conflicts from an earlier recovery
    /// attempt are still unresolved. No next step this dialog can honour
    /// beyond the message: resolve first.
    UpdateUnresolved,
    /// The recovery's update or fetch errored (or was cancelled). No next
    /// step: the failure is git's own to describe.
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

/// Classify a failed push-path operation into one of the six outcome classes
/// (ticket 13). `err` is the error the executor surfaced; its text carries
/// the verbatim git stderr on `TgError::Cli`. `conflicts_present` says
/// whether the affected root's freshly refreshed status still lists
/// conflicted paths — the worktree, not the stderr, is the reliable signal
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
    // The `git exited with code N:` prefix is the error wrapper's, not git's
    // own detail; strip it so the class carries what git actually said.
    let detail = match err {
        TgError::Cli { stderr, .. } => stderr.trim().to_string(),
        other => other.to_string(),
    };
    PushOutcome::UpdateErrored { detail }
}

/// How wide a rejected push's recovery update reaches (ticket 13).
///
/// The reference documents the EFFECT of the choice — "if this option is
/// cleared, only the affected repositories will be updated" — and never its
/// default, so the conservative reading is the one shipped: a push rejection in
/// one repository must not integrate other repositories' branches behind the
/// user.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpdateChoice {
    /// Only the repositories the rejected push acted on.
    #[default]
    AffectedOnly,
    /// Every registered repository.
    AllRepositories,
}

impl UpdateChoice {
    /// The segmented control's label for the choice.
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
/// Pull into `root`, merging or rebasing per `rebase`.
pub fn pull(vcs: &dyn GitExecutor, root: &Path, rebase: bool) -> TgResult<()> {
    vcs.pull(root, rebase)
}
/// Fetch everything, then pull using the configured update method.
pub fn update_project(vcs: &dyn GitExecutor, root: &Path, method: UpdateMethod) -> TgResult<()> {
    fetch(vcs, root, None)?;
    pull(vcs, root, method == UpdateMethod::Rebase)
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

/// Like [`push_all`], but honors `force` (`--force-with-lease`) for every
/// root; protected branches are still refused per root by [`push`]. Issue
/// #24 adds the other flags (`tags`, `no_verify`, `set_upstream`) plus the
/// optional `selected_oldest` (used only by the "Push current branch only"
/// path; the batch push never narrows per root).
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

/// Issue #25: push an EXPLICIT root subset (the dialog's resolved PUSH SCOPE)
/// with the same per-root remote resolution and protected-branch gating as
/// [`push_all_forced`]. One `(RootId, result)` per given root, in input order.
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
/// root. A root with no current branch records `Ok(())` (nothing to push).
pub fn push_all(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
) -> Vec<(RootId, TgResult<()>)> {
    push_all_forced(vcs, mgr, settings, false, false, false, false, None)
}
/// Returns one `(RootId, result)` per root. A root with no current branch, or
/// whose branch has no tracking upstream, records `Ok(vec![])`.
pub fn outgoing_per_root(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
) -> Vec<(RootId, TgResult<Vec<CommitId>>)> {
    mgr.roots
        .iter()
        .map(|root| {
            let result = match &root.current_branch {
                Some(branch) => {
                    match root
                        .branches
                        .iter()
                        .find(|b| &b.name == branch)
                        .and_then(|b| b.tracking.as_ref())
                    {
                        Some(upstream) => {
                            vcs.outgoing_commits(&root.path, branch, &upstream.git_ref())
                        }
                        // No tracking ref = nothing known to be ahead.
                        None => Ok(Vec::new()),
                    }
                }
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
        // Raw-prefix semantics (documented): "release/*" matches by prefix, so it also covers "releaseX/…".
        assert!(is_protected(&s, "eu/hotfix"));
        assert!(!is_protected(&s, "hotfix"), "suffix must start at */");
        let wild = settings_with(&["*"]);
        assert!(is_protected(&wild, "anything/at/all"));
    }

    // --- ticket 13: the six push-rejection outcome classes -----------------

    /// git's own rejection line for an ordinary (local) refusal.
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
        // git refuses to update over an unfinished merge — conflicts are in
        // the worktree but the failure text carries no fresh-conflict marker.
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
        // A recovery that stopped on conflicts AND left them in the tree must
        // not be mistaken for the re-push's own rejection.
        let err = TgError::Cli {
            code: 1,
            stderr: "CONFLICT (content): Merge conflict in base.txt\n\
                   Automatic merge failed; fix conflicts and then commit the result.\n"
                .into(),
        };
        let out = classify_push_failure(&err, true, PushPhase::Recovery, "origin", "main");
        assert_eq!(out, PushOutcome::UpdateConflicted);
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
