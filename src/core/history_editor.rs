//! Interactive-rebase plan builder / editor for F5 / I-series history editing.
//!
//! Operates purely on the [`model::RebasePlanEntry`] rows surfaced by the UI:
//! building a plan from `base..HEAD`, mutating actions, reordering, and
//! dispatching the final plan to [`VcsManager::rebase_interactive`].

#![allow(dead_code)]

use crate::core::vcs_manager::VcsManager;
use crate::error::TgResult;
use crate::model::*;
use std::path::Path;

/// Build an interactive-rebase plan for `base..HEAD`.
///
/// `vcs.log` returns commits newest-first, so the result is REVERSED so the
/// plan is oldest-first: the first entry is the oldest commit, closest to
/// `base`. Every entry defaults to [`RebaseAction::Pick`].
pub fn build_plan(
    vcs: &VcsManager,
    root: &Path,
    base: &str,
) -> TgResult<Vec<RebasePlanEntry>> {
    let commits = vcs.log(
        root,
        &LogOpts {
            branch: Some(format!("{}..HEAD", base)),
            ..Default::default()
        },
    )?;

    let mut plan: Vec<RebasePlanEntry> = commits
        .into_iter()
        .map(|c| RebasePlanEntry {
            action: RebaseAction::Pick,
            commit: c.id,
            subject: c.message.lines().next().unwrap_or("").to_string(),
        })
        .collect();

    plan.reverse();
    Ok(plan)
}

/// Set the [`RebaseAction`] of the plan entry at `index`, if in range.
pub fn set_action(plan: &mut [RebasePlanEntry], index: usize, action: RebaseAction) {
    if let Some(entry) = plan.get_mut(index) {
        entry.action = action;
    }
}

/// Move the entry at `from` to position `to`.
///
/// `to` is clamped into the valid insert range so an out-of-bounds target
/// simply moves the entry to the end (or as close as the plan allows).
pub fn reorder(plan: &mut Vec<RebasePlanEntry>, from: usize, to: usize) {
    if from >= plan.len() {
        return;
    }
    let to = to.min(plan.len().saturating_sub(1));
    if from == to {
        return;
    }
    let entry = plan.remove(from);
    plan.insert(to, entry);
}

/// Execute a rebase plan.
pub fn execute(vcs: &VcsManager, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
    vcs.rebase_interactive(root, plan)
}

/// Whether `root`'s current branch may be edited.
///
/// Returns `true` when there is no current branch (detached HEAD) or the
/// current branch is not listed in `protected`. A `None` current branch is
/// always editable.
pub fn can_edit(root: &Root, protected: &[String]) -> bool {
    match &root.current_branch {
        None => true,
        Some(branch) => !protected.contains(branch),
    }
}
