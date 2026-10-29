use turbogit_domain::model::{self, Commit, CommitId, RebaseAction, RebasePlanEntry};

use crate::history_editor::plan_preview;

/// Which rewrite the rule is looking at, the one fact the two cases disagree
/// on, and the reason this is a parameter rather than two functions.
///
/// A drop removes the acted-on row, so the row that took its place is the next
/// survivor and carries the subject it always had. A reword KEEPS the acted-on
/// row under a new hash, and that row's subject is the new one: matching the old
/// subject would find some unrelated commit whenever the history happens to
/// repeat it, so the caller states the subject the row will carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewrittenAnchor {
    Dropped,
    Reworded { subject: String },
}

/// The commit that should be selected after `plan` ran, given the refreshed
/// `after` log. `None` means the rule declines, and the caller clears the
/// selection, a guess here would be worse than an empty pane.
///
/// `plan` is the plan as [`crate::history_editor::build_plan`] leaves it,
/// oldest-first. `after` is a log listing in the order the app reads logs,
/// newest-first, and it may be one page of a long history rather than all of it,
/// which is why the wanted row is found by SUBJECT, with the position only as
/// the tie-break between repeated subjects and as the last resort.
pub fn reselect_after_rewrite(
    plan: &[RebasePlanEntry],
    after: &[Commit],
    anchor: &str,
    fate: &RewrittenAnchor,
) -> Option<CommitId> {
    let kept = plan_preview(plan).kept;
    let at = plan.iter().position(|e| e.commit == anchor)?;

    let survivor_from = |start: usize, step: isize| -> Option<&RebasePlanEntry> {
        let mut i = start as isize;
        while i >= 0 && (i as usize) < plan.len() {
            let row = &plan[i as usize];
            if row.action != RebaseAction::Drop && kept.iter().any(|k| k.commit == row.commit) {
                return Some(row);
            }
            i += step;
        }
        None
    };
    let wanted = survivor_from(at, 1).or_else(|| survivor_from(at.checked_sub(1)?, -1))?;
    let subject = match fate {
        RewrittenAnchor::Dropped => wanted.subject.as_str(),
        RewrittenAnchor::Reworded { subject } => subject.as_str(),
    };

    let candidates: Vec<(usize, &Commit)> = after
        .iter()
        .enumerate()
        .filter(|(_, c)| subject_of(c) == subject)
        .collect();
    let first = candidates.first()?.1;
    if candidates.len() == 1 {
        return Some(first.id.clone());
    }
    // A repeated subject: the candidate nearest the place the plan put the row
    // wins. With no position to judge by, the earlier row in the log is the
    // deterministic answer, never a coin flip. `min_by_key` keeps the first of
    // equal distances, so a tie resolves the same way every run.
    let winner = match expected_index(&kept, wanted) {
        Some(expected) => candidates
            .iter()
            .min_by_key(|(i, _)| i.abs_diff(expected))
            .expect("candidates is not empty"),
        None => candidates.first().expect("candidates is not empty"),
    };
    Some(winner.1.id.clone())
}

fn expected_index(kept: &[RebasePlanEntry], wanted: &RebasePlanEntry) -> Option<usize> {
    let ordinal = kept.iter().position(|k| k.commit == wanted.commit)?;
    kept.len().checked_sub(1)?.checked_sub(ordinal)
}

fn subject_of(commit: &Commit) -> &str {
    commit.subject()
}

pub fn subject_of_message(message: &str) -> &str {
    model::subject_of_message(message)
}
