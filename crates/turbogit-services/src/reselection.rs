//! Where the selection goes after a rewrite moves the commit it was on.
//!
//! A history rewrite re-creates every commit above the acted-on one, so the id
//! the details pane was showing stops naming anything. The rule is one function
//! because the two rewrites that need it are the same problem: take the first
//! surviving plan row at or after the acted-on row, and find that row in the
//! refreshed log by the subject it will carry.
//!
//! Pure: plans and logs in, an id out. No git, no state, no egui — which is
//! what lets it be a table.

use turbogit_domain::model::{Commit, CommitId, RebaseAction, RebasePlanEntry};

use crate::history_editor::plan_preview;

/// Which rewrite the rule is looking at — the one fact the two cases disagree
/// on, and the reason this is a parameter rather than two functions.
///
/// A drop removes the acted-on row, so the row that took its place is the next
/// survivor and carries the subject it always had. A reword KEEPS the acted-on
/// row under a new hash, and that row's subject is the new one: matching the old
/// subject would find some unrelated commit whenever the history happens to
/// repeat it, so the caller states the subject the row will carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewrittenAnchor {
    /// The anchor is gone; the commit that took its place is the next surviving
    /// row, found by the subject that row already had.
    Dropped,
    /// The anchor survives under a new hash, carrying this subject.
    Reworded { subject: String },
}

/// The commit that should be selected after `plan` ran, given the refreshed
/// `after` log. `None` means the rule declines, and the caller clears the
/// selection — a guess here would be worse than an empty pane.
///
/// `plan` is the plan as [`crate::history_editor::build_plan`] leaves it,
/// oldest-first. `after` is a log listing in the order the app reads logs,
/// newest-first, and it may be one page of a long history rather than all of it —
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

    // The row this rule is about: the first survivor at or after the acted-on
    // row — which IS the acted-on row when a reword kept it — and the last
    // survivor before it when nothing was built on top, because dropping the tip
    // leaves the anchor's own parent as the commit now standing there.
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

    // By subject first: the only identifier that survives both the re-created
    // hashes and a paged window.
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
    // deterministic answer — never a coin flip. `min_by_key` keeps the first of
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

/// Where `wanted` lands in a newest-first listing, from its place in the
/// surviving plan sequence (which is oldest-first). `None` when the refreshed log
/// is not long enough to hold the position at all.
fn expected_index(kept: &[RebasePlanEntry], wanted: &RebasePlanEntry) -> Option<usize> {
    let ordinal = kept.iter().position(|k| k.commit == wanted.commit)?;
    kept.len().checked_sub(1)?.checked_sub(ordinal)
}

/// A commit's subject, read the way every surface reads it: the first line of
/// the message.
fn subject_of(commit: &Commit) -> &str {
    subject_of_message(&commit.message)
}

/// The subject a MESSAGE will read as — the first line, which is git's own
/// definition of a commit's subject and this rule's.
///
/// Public because a caller building [`RewrittenAnchor::Reworded`] has to extract
/// the subject exactly the way the rule does. If the two ever disagreed the
/// comparison below would silently find nothing and the pane would clear, so the
/// definition is exported rather than re-implemented at the call site.
pub fn subject_of_message(message: &str) -> &str {
    message.lines().next().unwrap_or_default()
}
