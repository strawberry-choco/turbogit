//! Tag services (issue 31, screen 16): the live tag-name validation behind
//! the dialog's "✓ valid" / reason line, tag creation through the executor
//! port, and the push-immediately outcome reporting.

use std::path::Path;

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::TagSpec;
use turbogit_engine_api::GitExecutor;

/// Validate a tag name against git's ref-name rules and the repository's
/// existing tags (the dialog's live "✓ valid" / reason line). `Err` carries
/// a user-facing reason naming the violated rule.
pub fn validate_name(name: &str, existing: &[String]) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A tag name is required".to_string());
    }
    if name.starts_with('.') {
        return Err("cannot start with '.'".to_string());
    }
    if name.ends_with('/') {
        return Err("cannot end with '/'".to_string());
    }
    if let Some(c) = name
        .chars()
        .find(|c| matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        let shown = if c == ' ' {
            "a space".to_string()
        } else {
            format!("'{c}'")
        };
        return Err(format!("cannot contain {shown}"));
    }
    if name.contains("..") {
        return Err("cannot contain '..'".to_string());
    }
    if name.contains("@{") {
        return Err("cannot contain '@{{'".to_string());
    }
    if name.ends_with('.') {
        return Err("cannot end with '.'".to_string());
    }
    if name.ends_with(".lock") {
        return Err("cannot end with '.lock'".to_string());
    }
    if existing.iter().any(|e| e == name) {
        return Err(format!("A tag named '{name}' already exists"));
    }
    Ok(())
}

/// Create the tag described by `spec` (`git tag [-a -m] [-s] <name>
/// [<commit>]`).
pub fn create(vcs: &dyn GitExecutor, root: &Path, spec: &TagSpec) -> TgResult<()> {
    vcs.tag_create(root, spec)
}

/// The repository's tags — the duplicate side of [`validate_name`]'s live
/// check.
pub fn list(vcs: &dyn GitExecutor, root: &Path) -> TgResult<Vec<String>> {
    vcs.tag_list(root)
}

/// The configured GPG signing key (`git config user.signingkey`) — the
/// fingerprint chip shown next to the dialog's "Sign with GPG key" option.
/// `None` when no signing key is configured.
pub fn signing_key(vcs: &dyn GitExecutor, root: &Path) -> Option<String> {
    vcs.run_raw(
        root,
        &[
            "config".to_string(),
            "--get".to_string(),
            "user.signingkey".to_string(),
        ],
    )
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

/// Push a freshly created tag to the repository's first remote ("Push tag to
/// origin immediately"). Returns the remote it was pushed to; a repository
/// with no remote is a clear error, and a push failure after the tag exists
/// keeps the "tag was created" context in the message.
pub fn push_new(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<String> {
    let remotes = vcs.remotes(root)?;
    let remote = match remotes.first() {
        Some(r) => r.name.clone(),
        None => {
            return Err(TgError::Other(format!(
                "Tag {name} was created, but this repository has no remote to push it to"
            )));
        }
    };
    vcs.tag_push(root, &remote, Some(name), false)
        .map_err(|e| {
            TgError::Other(format!(
                "Tag {name} was created, but pushing to {remote} failed: {e}"
            ))
        })?;
    Ok(remote)
}
