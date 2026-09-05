//! Remote synchronization service: fetch / pull / push for single roots and
//! aggregated across all roots in a [`MultiRootManager`].
//!
//! Provides protected-branch gating for force-pushes and batch operations that
//! iterate every registered root, recording one result per root.

#![allow(dead_code)]

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
                    let remote = match root
                        .branches
                        .iter()
                        .find(|b| &b.name == branch)
                        .and_then(|b| b.tracking.as_deref())
                    {
                        Some(t) if t.contains('/') => t.split('/').next().unwrap_or("origin"),
                        _ => root
                            .remotes
                            .first()
                            .map(|r| r.name.as_str())
                            .unwrap_or("origin"),
                    };
                    let _ = selected_oldest;
                    vcs.push_dry_run(&root.path, remote, branch, force)
                }
                None => Ok(String::new()),
            };
            let remote = root
                .branches
                .iter()
                .find(|b| Some(&b.name) == root.current_branch.as_ref())
                .and_then(|b| b.tracking.as_deref())
                .and_then(|t| t.split('/').next())
                .map(|s| s.to_string())
                .or_else(|| root.remotes.first().map(|r| r.name.clone()))
                .unwrap_or_else(|| "origin".into());
            (remote, result)
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
                Some(branch) => {
                    // Resolve the upstream remote from the branch's tracking ref
                    // when present; otherwise fall back to the first remote, or
                    // "origin" if the root has no remotes configured.
                    let remote = match root
                        .branches
                        .iter()
                        .find(|b| &b.name == branch)
                        .and_then(|b| b.tracking.as_deref())
                    {
                        Some(t) if t.contains('/') => t.split('/').next().unwrap_or("origin"),
                        _ => root
                            .remotes
                            .first()
                            .map(|r| r.name.as_str())
                            .unwrap_or("origin"),
                    };
                    push(
                        vcs,
                        &root.path,
                        remote,
                        branch,
                        force,
                        tags,
                        no_verify,
                        set_upstream,
                        selected_oldest,
                        settings,
                    )
                }
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
                        .and_then(|b| b.tracking.as_deref())
                    {
                        Some(upstream) => vcs.outgoing_commits(&root.path, branch, upstream),
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

/// Update (fetch + pull) every root using each root's configured update method.
pub fn update_all(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
) -> Vec<(RootId, TgResult<()>)> {
    let method = settings.update_method;
    mgr.roots
        .iter()
        .map(|root| {
            let result = update_project(vcs, &root.path, method);
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
