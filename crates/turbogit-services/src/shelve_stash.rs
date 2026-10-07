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

/// Build a new [`Shelf`] capturing `changes` under `name`, owned by `root`.
pub fn make_shelf(name: &str, changes: &[Change], root: RootId) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: changes.to_vec(),
        created_at: chrono::Utc::now(),
        root,
    }
}

/// The shelf name a stash entry's message carries, or `None` when the entry was
/// not written by a shelve.
///
/// **The shelf's content IS a git stash entry**: [`crate::shelve_stash::stash`]
/// parks the work with `git stash push -m <shelf name>`, and git stores a named
/// stash as `On <branch>: <name>` (an unnamed one reads `WIP on <branch>:
/// <sha> <subject>`). Unshelving therefore has to find the entry again by name,
/// and the two adapters hand back the message at different depths — the CLI
/// strips `stash@{n}: ` when it parses the listing, libgit2 never had it — so
/// this recognises both the prefixed and the bare spelling rather than betting
/// on one. A name that contains `": "` survives the split because only the
/// leading `On <branch>` prefix is removed.
pub fn shelf_name_of(message: &str) -> Option<&str> {
    let rest = message
        .strip_prefix("WIP on ")
        .or_else(|| message.strip_prefix("On "))
        .unwrap_or(message);
    match rest.split_once(": ") {
        // `On <branch>` with no message after it: not a shelve's entry.
        Some((_, name)) if name.is_empty() => None,
        Some((_, name)) => Some(name),
        None => (!rest.is_empty()).then_some(rest),
    }
}

/// The stash entry holding `name`'s shelved content — the newest match, since
/// `git stash list` is newest-first and two shelves may share a name.
pub fn find_shelf_stash<'a>(entries: &'a [Stash], name: &str) -> Option<&'a Stash> {
    entries
        .iter()
        .find(|s| shelf_name_of(&s.message) == Some(name))
}

/// Put a shelf's content back in the working tree.
///
/// `remove` is the difference between the reference's two verbs with **opposite
/// keep/remove defaults**: `false` is `Unshelve` (the shelved copy stays where
/// it is), `true` is `Unshelve and Remove` (the git copy goes with it, which is
/// what makes the shelf row's own removal safe). Returns the stash entry it
/// applied, so the caller can name it in what it reports.
pub fn unshelve(
    vcs: &dyn GitExecutor,
    root: &Path,
    name: &str,
    remove: bool,
) -> TgResult<Stash> {
    let entries = list(vcs, root)?;
    let found = find_shelf_stash(&entries, name).cloned().ok_or_else(|| {
        turbogit_domain::error::TgError::Other(format!(
            "Nothing shelved as '{name}' is left in this repository's stash list"
        ))
    })?;
    if remove {
        vcs.stash_pop(root, found.index)?;
    } else {
        vcs.stash_apply(root, found.index)?;
    }
    Ok(found)
}

/// Throw away the stash entry a shelf's content was parked in, when it is still
/// there. A shelf whose entry is already gone has nothing left on the git side,
/// which is a normal answer rather than a failure: the verb is deleting the
/// record, and this is the cleanup that follows it.
pub fn discard_shelf_content(vcs: &dyn GitExecutor, root: &Path, name: &str) -> TgResult<()> {
    if let Some(found) = find_shelf_stash(&list(vcs, root)?, name) {
        let index = found.index;
        vcs.stash_drop(root, index)?;
    }
    Ok(())
}

/// How one stash entry is named to a person: git's own ref spelling.
pub fn stash_ref(index: usize) -> String {
    format!("stash@{{{index}}}")
}

/// Path to the persisted shelf store for `project_dir`.
pub fn shelf_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".turbogit").join("shelf.ron")
}

/// Read the shelf store back. A file that is not there is an empty shelf — the
/// store is created by the first shelve, not at launch.
///
/// A file this build cannot parse is an **error, not an empty list**: those rows
/// are the user's shelved work, and a pane that answered "nothing is shelved"
/// over a store it failed to read would be reporting an absence it has not
/// established.
pub fn load_shelves(project_dir: &Path) -> TgResult<Vec<Shelf>> {
    let path = shelf_path(project_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)?;
    ron::from_str(&text).map_err(|e| turbogit_domain::error::TgError::Serde(e.to_string()))
}

/// Serialize `shelves` to `shelf.ron` under `.turbogit/`.
pub fn save_shelves(project_dir: &Path, shelves: &[Shelf]) -> TgResult<()> {
    fs::create_dir_all(project_dir.join(".turbogit"))?;
    let text = ron::ser::to_string_pretty(shelves, ron::ser::PrettyConfig::new())
        .map_err(|e| turbogit_domain::error::TgError::Serde(e.to_string()))?;
    fs::write(shelf_path(project_dir), text)?;
    Ok(())
}
