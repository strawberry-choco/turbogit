//! The sidebar's fleet pull.
//!
//! One operation over every selected root, sitting beside
//! [`AppState::dispatch_pull`](crate::state::AppState::dispatch_pull) so that
//! "is this pull guarded?" is answered by reading this file and that one rather
//! than by consulting an allow-list.

use std::path::PathBuf;

use crate::operation::Operation;
use crate::root_caches::Affected;

/// Pull every root in `paths`, one operation.
///
/// **Merge-only by construction**, which is what the `false` is: this verb is
/// not handed the configured update method, so it cannot be moved onto a
/// rebasing update without an edit to this line, and a rebasing update is a
/// rewrite that owes the protected-branch refusal and the rewrite backup. It
/// therefore owes neither — a fleet verb that rewrote `main` in three
/// repositories at once would be the defect the guarded seam exists to prevent.
///
/// `Affected::All` because the scope is the caller's choice: the sidebar is
/// selecting across the whole workspace, and narrowing it to a root the entry
/// cannot name would silently skip the rest.
pub fn selection(paths: Vec<PathBuf>) -> Operation {
    let n = paths.len();
    Operation::custom(format!("Pull · {n} repos"), Affected::All, move |v| {
        for p in &paths {
            v.pull(p, false)?;
        }
        Ok(())
    })
}
