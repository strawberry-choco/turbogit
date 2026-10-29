use std::collections::HashSet;
use std::path::PathBuf;

use turbogit_domain::model::RootId;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PinnedView {
    pub name: String,
    pub repos: Vec<PathBuf>,
}

pub fn next_name(existing: &[PinnedView]) -> String {
    let max = existing
        .iter()
        .filter_map(|v| v.name.strip_prefix("View ")?.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("View {}", max + 1)
}

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
