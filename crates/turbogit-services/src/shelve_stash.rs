//! Git stash operations and the IDE "shelf" (patch store).
//!
//! The stash functions are thin pass-throughs to [`GitExecutor`]. The shelf is a
//! pure-Rust IDE patch store: named buckets of affected file paths persisted by
//! the caller as RON under `.turbogit/shelf.ron`.

use std::fs;
use std::path::{Path, PathBuf};
use turbogit_domain::error::TgResult;
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

// ---------- Stash pass-throughs ----------

/// Create a stash entry for `root`.
pub fn stash(vcs: &dyn GitExecutor, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
    vcs.stash_push(root, message, keep_index)
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

/// Serialize `shelves` to `shelf.ron` under `.turbogit/`.
pub fn save_shelves(project_dir: &Path, shelves: &[Shelf]) -> TgResult<()> {
    fs::create_dir_all(project_dir.join(".turbogit"))?;
    let text = ron::ser::to_string_pretty(shelves, ron::ser::PrettyConfig::new())
        .map_err(|e| turbogit_domain::error::TgError::Serde(e.to_string()))?;
    fs::write(shelf_path(project_dir), text)?;
    Ok(())
}
