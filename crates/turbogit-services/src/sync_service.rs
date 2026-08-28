//! Remote synchronization service: fetch / pull / push for single roots and
//! aggregated across all roots in a [`MultiRootManager`].
//!
//! Provides protected-branch gating for force-pushes and batch operations that
//! iterate every registered root, recording one result per root.

#![allow(dead_code)]

use std::path::Path;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// Returns `true` if `branch` matches any protected-branch pattern.
///
/// Pattern semantics:
/// - `*` matches every branch.
/// - a pattern ending with `*` matches by prefix (e.g. `release/*`).
/// - a pattern starting with `*` matches by suffix (e.g. `*/main`).
/// - otherwise the pattern must match the branch name exactly.
pub fn is_protected(settings: &VcsSettings, branch: &str) -> bool {
    settings.protected_branch_patterns.iter().any(|pat| {
        if pat == "*" {
            true
        } else if let Some(prefix) = pat.strip_suffix('*') {
            branch.starts_with(prefix)
        } else if let Some(suffix) = pat.strip_prefix('*') {
            branch.ends_with(suffix)
        } else {
            pat == branch
        }
    })
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

/// Push `branch` to `remote`, refusing a force-push to a protected branch.
pub fn push(
    vcs: &dyn GitExecutor,
    root: &Path,
    remote: &str,
    branch: &str,
    force: bool,
    settings: &VcsSettings,
) -> TgResult<()> {
    if force && is_protected(settings, branch) {
        return Err(TgError::Other(format!(
            "Refusing force-push to protected branch '{}'",
            branch
        )));
    }
    vcs.push(root, remote, branch, force)
}

/// Push all tags to `remote` (or a single named tag when `name` is `Some`).
pub fn push_tags(vcs: &dyn GitExecutor, root: &Path, remote: &str, all: bool) -> TgResult<()> {
    vcs.tag_push(root, remote, None, all)
}

/// Push the current branch of every root. Returns one `(RootId, result)` per
/// root. A root with no current branch records `Ok(())` (nothing to push).
pub fn push_all(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
) -> Vec<(RootId, TgResult<()>)> {
    push_all_forced(vcs, mgr, settings, false)
}

/// Like [`push_all`], but honors `force` (`--force-with-lease`) for every
/// root; protected branches are still refused per root by [`push`].
pub fn push_all_forced(
    vcs: &dyn GitExecutor,
    mgr: &MultiRootManager,
    settings: &VcsSettings,
    force: bool,
) -> Vec<(RootId, TgResult<()>)> {
    mgr.roots
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
                    push(vcs, &root.path, remote, branch, force, settings)
                }
                None => Ok(()),
            };
            (root.id.clone(), result)
        })
        .collect()
}

/// List each root's outgoing commits (local-ahead SHAs, newest-first).
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
    use turbogit_engine_api::fake::{Call, FakeExecutor};

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

        let blocked = push(&engine, &root, "origin", "main", true, &s);
        assert!(blocked.is_err(), "force-push to protected branch must fail");
        assert!(
            engine.calls.lock().unwrap().is_empty(),
            "blocked push must not reach the engine"
        );

        let normal = push(&engine, &root, "origin", "feature", false, &s);
        assert!(normal.is_ok());
        let calls = engine.calls.lock().unwrap();
        assert_eq!(
            calls.as_slice(),
            [Call::Push {
                root: root.clone(),
                remote: "origin".into(),
                branch: "feature".into(),
                force: false,
            }]
        );
    }
}
