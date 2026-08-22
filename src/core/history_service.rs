//! History service: log graph, file/selection/dir history, blame, compare, and
//! "open on web". Thin orchestration over [`GitExecutor`]; no git plumbing here.

#![allow(dead_code)]

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::path::Path;

/// Commit log for a root (log graph backing store).
pub fn log(vcs: &dyn GitExecutor, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
    vcs.log(root, opts)
}

/// History of a single file (follow renames handled by the engine).
pub fn file_history(vcs: &dyn GitExecutor, root: &Path, path: &Path) -> TgResult<Vec<Commit>> {
    vcs.log(
        root,
        &LogOpts {
            path: Some(path.to_path_buf()),
            ..Default::default()
        },
    )
}

/// Per-line blame for a file at an optional revision.
pub fn blame(
    vcs: &dyn GitExecutor,
    root: &Path,
    path: &Path,
    rev: Option<&str>,
) -> TgResult<Vec<BlameLine>> {
    vcs.blame(root, path, rev)
}

/// Unified diff between two commits (compare view).
pub fn compare_commits(
    vcs: &dyn GitExecutor,
    root: &Path,
    left: &str,
    right: &str,
) -> TgResult<String> {
    vcs.diff(
        root,
        &DiffOpts {
            left: Some(left.to_string()),
            right: Some(right.to_string()),
            ..Default::default()
        },
    )
}

/// Contents of a file at a given revision (selection/dir history viewer).
pub fn show_at(vcs: &dyn GitExecutor, root: &Path, rev: &str, path: &Path) -> TgResult<String> {
    vcs.show_file(root, rev, path)
}

/// Build a web URL for a commit on the hosting provider, if a known remote exists.
pub fn open_on_web(root: &Root, rev: &str) -> Option<String> {
    for remote in &root.remotes {
        let url = remote.url.trim();
        let (host, owner_repo) = if let Some(rest) = url.strip_prefix("git@") {
            // git@host:owner/repo.git
            let (host, repo) = rest.split_once(':')?;
            (host, repo)
        } else if let Some(rest) = url.strip_prefix("https://") {
            // https://host/owner/repo.git
            let without_host = rest.split_once('/').map(|(_, r)| r).unwrap_or(rest);
            (rest.split('/').next().unwrap_or(rest), without_host)
        } else {
            continue;
        };

        let owner_repo = owner_repo.strip_suffix(".git").unwrap_or(owner_repo);
        let (owner, repo) = owner_repo.split_once('/')?;
        if owner.is_empty() || repo.is_empty() {
            continue;
        }

        if host.contains("github.com") {
            return Some(format!(
                "https://github.com/{owner}/{repo}/commit/{rev}"
            ));
        } else if host.contains("gitlab") {
            return Some(format!(
                "https://gitlab.com/{owner}/{repo}/-/commit/{rev}"
            ));
        }
    }
    None
}

/// Search-Everywhere history: full log filtered by id/message substring.
pub fn log_index(vcs: &dyn GitExecutor, root: &Path, query: &str) -> TgResult<Vec<Commit>> {
    let all = vcs.log(root, &LogOpts::default())?;
    let q = query.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|c| {
            c.id.to_lowercase().contains(&q) || c.message.to_lowercase().contains(&q)
        })
        .collect())
}
