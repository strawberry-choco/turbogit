use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

pub fn add(vcs: &dyn GitExecutor, root: &Path, name: &str, url: &str) -> TgResult<()> {
    vcs.add_remote(root, name, url)
}

pub fn set_url(
    vcs: &dyn GitExecutor,
    root: &Path,
    name: &str,
    fetch_url: Option<&str>,
    push_url: Option<&str>,
) -> TgResult<()> {
    vcs.set_remote_url(root, name, fetch_url, push_url)
}

pub fn rename(vcs: &dyn GitExecutor, root: &Path, old: &str, new: &str) -> TgResult<()> {
    vcs.rename_remote(root, old, new)
}

pub fn remove(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<()> {
    vcs.remove_remote(root, name)
}

pub fn set_upstream(
    vcs: &dyn GitExecutor,
    root: &Path,
    branch: &str,
    upstream: &str,
) -> TgResult<()> {
    vcs.set_branch_upstream(root, branch, upstream)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteChange {
    Add {
        name: String,
        url: String,
    },
    /// Update one of a remote's URLs (`None` = leave that side untouched).
    SetUrl {
        name: String,
        fetch_url: Option<String>,
        push_url: Option<String>,
    },
}

/// Apply `change` across exactly `selected` roots, returning one
/// `(RootId, result)` per selected root in the caller's order and continuing
/// after individual failures (the issue 33 per-repo outcomes). Each
/// successful root's `remotes` is refreshed in the manager so the UI list
/// reflects the change. Roots not registered in the manager are skipped.
pub fn apply_to_selection(
    vcs: &dyn GitExecutor,
    mgr: &mut MultiRootManager,
    selected: &[RootId],
    change: &RemoteChange,
) -> Vec<(RootId, TgResult<()>)> {
    let paths: Vec<(RootId, PathBuf)> = selected
        .iter()
        .filter_map(|rid| mgr.by_id(rid).map(|root| (rid.clone(), root.path.clone())))
        .collect();
    let outcomes = apply_to_paths(vcs, &paths, change);
    for ((rid, path), (_, result)) in paths.iter().zip(&outcomes) {
        if result.is_ok()
            && let Ok(remotes) = vcs.remotes(path)
            && let Some(root) = mgr.roots.iter_mut().find(|r| r.id == *rid)
        {
            root.remotes = remotes;
        }
    }
    outcomes
}

/// Apply `change` to exactly the given root paths, returning one
/// `(RootId, result)` per entry in the caller's order and continuing after
/// individual failures. Never touches the manager, the worker-side seam the
/// UI dispatches on (the app refreshes the manager from the completion
/// event). [`apply_to_selection`] resolves the paths and adds the refresh.
pub fn apply_to_paths(
    vcs: &dyn GitExecutor,
    roots: &[(RootId, PathBuf)],
    change: &RemoteChange,
) -> Vec<(RootId, TgResult<()>)> {
    roots
        .iter()
        .map(|(rid, path)| {
            let result = match change {
                RemoteChange::Add { name, url } => vcs.add_remote(path, name, url),
                RemoteChange::SetUrl {
                    name,
                    fetch_url,
                    push_url,
                } => vcs.set_remote_url(path, name, fetch_url.as_deref(), push_url.as_deref()),
            };
            (rid.clone(), result)
        })
        .collect()
}
