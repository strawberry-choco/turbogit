//! Pinned views (issue #08, screen 04's "Pin as view"): a saved multi-repo
//! selection recalled by name. Plain serde data persisted with the
//! workspace's `ui.ron` (the smart-group-rules precedent); recall filters
//! the saved paths against the roots still registered, so a view written
//! before a repo vanished restores to the survivors.

use std::collections::HashSet;
use std::path::PathBuf;

use turbogit_domain::model::RootId;

/// One saved selection: an auto-generated name ("View 1", …) and the
/// pinned repository root paths.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PinnedView {
    pub name: String,
    pub repos: Vec<PathBuf>,
}

/// The name for the next pinned view: one past the highest existing
/// "View N" label; free-form names don't shift the counter.
pub fn next_name(existing: &[PinnedView]) -> String {
    let max = existing
        .iter()
        .filter_map(|v| v.name.strip_prefix("View ")?.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("View {}", max + 1)
}

/// Resolve a view back into a live selection: only the saved paths that
/// are still registered roots return.
pub fn selection_from_view(view: &PinnedView, registered: &[RootId]) -> HashSet<RootId> {
    view.repos
        .iter()
        .filter_map(|p| {
            registered
                .iter()
                .find(|r| r.0.as_ref() == p.as_path())
                .cloned()
        })
        .collect()
}
