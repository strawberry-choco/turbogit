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

/// Which phase a push-path operation was in when it failed. The
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

/// The classified push outcome — six classes, four of which carry
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

/// Classify a failed push-path operation into one of the six outcome classes.
/// `err` is the error the executor surfaced; its text carries
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

/// The push panel's **named target state**: what the panel says about the ref
/// the push is about to write, so "this creates a branch" and "this moves a
/// branch that exists" are never the same word.
///
/// Five states, one name each. The question this answers is "where does this push
/// go": a Remote/Branch field pair plus a `--set-upstream` checkbox carries the
/// capability without the disclosure, which is why the state is named here rather
/// than left for the user to infer — creating a branch and overwriting one must
/// never share a word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushTargetState {
    /// Nothing on this branch yet — the repository has no commits.
    EmptyRepository,
    /// The remote branch does not exist: the push creates it.
    New,
    /// The remote branch does not exist and the push also records the upstream.
    NewAndUpstream,
    /// The remote branch exists; the push records the upstream for it.
    SetUpstream,
    /// The remote branch exists and is already the branch this push moves.
    Upstream,
}

impl PushTargetState {
    /// The word the panel paints. `New & Upstream` is its OWN string, never
    /// `New` with a suffix appended: a panel that grows one label out of another
    /// cannot be read as two distinct states.
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

/// Whether the repository holds no commit on any local branch — the panel's
/// `Empty repository` case, and a push git would refuse.
///
/// NOT `Root::head`: the registration snapshot leaves `head` `None` for every
/// repository, so a check on it reports every repo as empty and passes for the
/// wrong reason.
pub fn repository_is_empty(root: &Root) -> bool {
    root.branches.iter().all(|b| b.kind != BranchKind::Local)
}

/// Name the target state of a push of `branch` to `remote` from `root`, with
/// `set_upstream` as the panel's checkbox. `remote` is the panel's own field,
/// not [`push_remote`]'s resolution: the state has to describe the ref the
/// push would actually write, and an edited field changes that.
///
/// `None` is "there is nothing to name yet": no branch typed, or no remote to
/// target. Both are the panel's own problem — the remote-less case paints the
/// inline `Define remote` link in place of the remote name, and an empty branch
/// field has no target to name — so this stays a total function over the states
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
    // A remote branch row carries git's `<remote>/<branch>` spelling, which is
    // the same assembly `Upstream::git_ref` uses for the argv positions.
    let expected = if remote == "." {
        branch.to_string()
    } else {
        format!("{remote}/{branch}")
    };
    // Two sources say the ref is out there, and the root snapshot carries only
    // the second one until a remote listing is fetched: a listed remote row, or
    // the branch's own tracking ref.
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

/// A push that needs no dialog: the one root, its own remote and branch, with
/// the upstream recorded when the branch has none.
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
/// more than one root (which roots? — that is the dialog's scope control), a
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
    // A checked set is either the whole workspace — nothing subset about it —
    // or a selection this one-click verb cannot honour.
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

/// How wide a rejected push's recovery update reaches.
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

/// Pull the way a developer asking to pull is answered — the guarded form of
/// [`pull`], paying the two dues a rebasing update owes and a merging one does
/// not: a protected branch refuses before anything runs, and a rebase records
/// the tip it is about to rewrite.
///
/// `branch` is the branch the update lands on, when the caller already knows it
/// (a `Checkout and pull` names it from the row it clicked). The repository's own
/// HEAD wins when it can be read, because it is the fact rather than the
/// caller's belief about it, and a caller that checked a branch out before
/// calling here has HEAD as exactly that branch. With neither — a detached HEAD —
/// there is no branch to guard, and so no rewrite to allow either.
///
/// **The order is load-bearing and compiles either way.**
/// [`crate::integrate_service::ensure_rebase_allowed`] must refuse BEFORE
/// [`GitExecutor::save_rewrite_backup`]: one backup reference exists per
/// repository, it is the Undo verb's only target, and a refusal that had already
/// written it would overwrite the record of an earlier rewrite that succeeded.
/// And the backup must be written BEFORE the pull, or the rebase rewrites history
/// with nothing to undo. That is why a rebasing pull cannot be inlined as a bare
/// `vcs.pull(root, rebase)` at a call site — which is precisely what the shell's
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
        // A merging update rewrites nothing, so it owes neither the refusal nor
        // the backup — and must not consume the repository's one backup slot.
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

/// Fetch, then integrate the upstream into `root` as a rejected push's
/// recovery. The two guards are [`pull_guarded`]'s; what is specific to a
/// recovery is the fetch it must do first — the push was rejected because a
/// remote moved, so the update works from a freshly fetched set of remotes —
/// and the fact that the caller has just read `HEAD` to decide whether there is
/// an upstream to update at all, so it hands that branch down rather than
/// asking again.
pub fn update_for_recovery(
    vcs: &dyn GitExecutor,
    root: &Path,
    method: UpdateMethod,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    if method == UpdateMethod::Rebase {
        // The refusal [`pull_guarded`] also makes, made one step earlier and
        // deliberately twice: a recovery's fetch is a git command too, and a
        // protected branch owes the recovery nothing at all. Asking after the
        // fetch would be the ordinary `Pull` verb's behavior, not this one's.
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
                Some(branch) => match tracked_upstream(root) {
                    Some(upstream) => vcs.outgoing_commits(&root.path, branch, &upstream.git_ref()),
                    // No tracking ref = nothing known to be ahead.
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
        // Raw-prefix semantics (documented): "release/*" matches by prefix, so it also covers "releaseX/…".
        assert!(is_protected(&s, "eu/hotfix"));
        assert!(!is_protected(&s, "hotfix"), "suffix must start at */");
        let wild = settings_with(&["*"]);
        assert!(is_protected(&wild, "anything/at/all"));
    }

    // --- the six push-rejection outcome classes ----------------------------

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

    // --- the named push target states --------------------------------------

    /// A root on `main`… rather, on `feat`: `has_commits` decides whether the
    /// local branch row exists at all, which is the panel's `Empty repository`
    /// signal (`head` is `None` for every registered root and says nothing).
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
        // The ticked box is a DIFFERENT name, not `New` plus a suffix.
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
        // No remote: the panel shows the `Define remote` link instead of a name.
        assert_eq!(
            push_target_state(&target_root(&[], &[], true, false), "origin", "feat", false),
            None
        );
        // No branch typed: there is no ref to name.
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

    // --- the no-dialog push target ------------------------------------------

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
        // `feat` is not protected here, and it tracks, so no upstream flag.
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
        // A checked set that names a DIFFERENT root is a subset the one-click
        // verb cannot honour; a checked set that names the only root is the
        // whole workspace, and nothing is subset away from it.
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
