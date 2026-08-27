#![allow(dead_code)]

//! Git stash operations and the IDE "shelf" (patch store).
//!
//! The stash functions are thin pass-throughs to [`GitExecutor`]. The shelf is a
//! pure-Rust IDE patch store: named buckets of affected file paths persisted by
//! the caller as RON under `.turbogit/shelf.ron`.

use crate::engine::GitExecutor;
use crate::error::TgResult;
use crate::model::*;
use std::fs;
use std::path::{Path, PathBuf};

// ---------- Stash pass-throughs ----------

/// Create a stash entry for `root`.
pub fn stash(vcs: &dyn GitExecutor, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
    vcs.stash_push(root, message, keep_index)
}

/// Apply the stash at `index` without dropping it.
pub fn stash_apply_index(vcs: &dyn GitExecutor, root: &Path, index: usize) -> TgResult<()> {
    vcs.stash_apply(root, index)
}

/// Apply the stash at `index` and drop it.
pub fn stash_pop_index(vcs: &dyn GitExecutor, root: &Path, index: usize) -> TgResult<()> {
    vcs.stash_pop(root, index)
}

/// Drop the stash at `index`.
pub fn stash_drop_index(vcs: &dyn GitExecutor, root: &Path, index: usize) -> TgResult<()> {
    vcs.stash_drop(root, index)
}

/// Create a branch from a stash entry (`git stash branch`).
pub fn stash_branch(
    vcs: &dyn GitExecutor,
    root: &Path,
    index: usize,
    branch: &str,
) -> TgResult<()> {
    vcs.branch_create(root, branch, true, Some(&format!("stash@{{{}}}", index)))
}

/// List all stash entries for `root`.
pub fn list(vcs: &dyn GitExecutor, root: &Path) -> TgResult<Vec<Stash>> {
    vcs.stash_list(root)
}

// ---------- IDE shelf (patch store) ----------

/// Build a new [`Shelf`] capturing `changes` under `name`.
pub fn make_shelf(name: &str, changes: &[Change]) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: changes.to_vec(),
        created_at: chrono::Utc::now(),
    }
}

/// Path to the persisted shelf store for `project_dir`.
pub fn shelf_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".turbogit").join("shelf.ron")
}

/// Load all shelves; returns an empty `Vec` if missing or on any parse error
/// (never panics).
pub fn load_shelves(project_dir: &Path) -> Vec<Shelf> {
    let path = shelf_path(project_dir);
    match fs::read_to_string(&path) {
        Ok(txt) => ron::de::from_str::<Vec<Shelf>>(&txt).unwrap_or_default(),
        Err(_) => vec![],
    }
}

/// Serialize `shelves` to `shelf.ron` under `.turbogit/`.
pub fn save_shelves(project_dir: &Path, shelves: &[Shelf]) -> TgResult<()> {
    fs::create_dir_all(project_dir.join(".turbogit"))?;
    let text = ron::ser::to_string_pretty(shelves, ron::ser::PrettyConfig::new())
        .map_err(|e| crate::error::TgError::Serde(e.to_string()))?;
    fs::write(shelf_path(project_dir), text)?;
    Ok(())
}

/// Re-stage a shelf's affected paths.
///
/// Shelve stores the *list of affected paths* and re-stages them on unshelve;
/// full patch-content storage is a known limitation of this model.
pub fn unshelve(vcs: &dyn GitExecutor, root: &Path, shelf: &Shelf) -> TgResult<()> {
    let paths: Vec<PathBuf> = shelf.changes.iter().map(|c| c.path.clone()).collect();
    vcs.add(root, &paths)
}
