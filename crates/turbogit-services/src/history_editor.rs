//! Interactive-rebase plan builder / editor for F5 / I-series history editing.
//!
//! Operates purely on the [`model::RebasePlanEntry`] rows surfaced by the UI:
//! building a plan from `base..HEAD`, mutating actions, reordering, and
//! dispatching the final plan to [`GitExecutor::rebase_interactive`].

#![allow(dead_code)]

use std::path::Path;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::GitExecutor;

/// Build an interactive-rebase plan for `base..HEAD`.
///
/// `vcs.log` returns commits newest-first, so the result is REVERSED so the
/// plan is oldest-first: the first entry is the oldest commit, closest to
/// `base`. Every entry defaults to [`RebaseAction::Pick`].
pub fn build_plan(
    vcs: &dyn GitExecutor,
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
pub fn execute(vcs: &dyn GitExecutor, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
    vcs.rebase_interactive(root, plan)
}

/// The safety-net ref written before the first replayed commit (issue 30,
/// screen 17's RECOVERY panel): points at the pre-rebase HEAD, so an abort
/// ([`abort_to_backup`]) can always restore the rewritten branch.
pub const BACKUP_REF: &str = "refs/turbogit/preflight-backup";

/// Execute a rebase plan with the backup-ref safety net (issue 30): the
/// guarded dispatch (protected branches refuse before anything runs), then
/// [`BACKUP_REF`] is written at the current HEAD, then the plan replays.
pub fn execute_with_backup(
    vcs: &dyn GitExecutor,
    root: &Path,
    plan: &[RebasePlanEntry],
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    if let Some(current) = vcs.current_branch(root)? {
        crate::integrate_service::ensure_rebase_allowed(settings, &current)?;
    } else if !branch.is_empty() {
        crate::integrate_service::ensure_rebase_allowed(settings, branch)?;
    }
    let head = vcs.run_raw(root, &["rev-parse".to_string(), "HEAD".to_string()])?;
    vcs.run_raw(
        root,
        &[
            "update-ref".to_string(),
            BACKUP_REF.to_string(),
            head.trim().to_string(),
        ],
    )?;
    vcs.rebase_interactive(root, plan)
}

/// Abort the running rebase and restore the pre-rebase state from
/// [`BACKUP_REF`] (screen 17's RECOVERY action): an in-progress rebase is
/// aborted first, then the branch resets hard to the backup ref, and the
/// spent backup ref is deleted. Errors when no backup ref exists — the net
/// is only present while a [`execute_with_backup`] replay is the last
/// rewrite.
pub fn abort_to_backup(vcs: &dyn GitExecutor, root: &Path) -> TgResult<()> {
    if crate::integrate_service::in_progress(root) {
        vcs.abort(root, "rebase")?;
    }
    vcs.run_raw(
        root,
        &[
            "reset".to_string(),
            "--hard".to_string(),
            BACKUP_REF.to_string(),
        ],
    )?;
    vcs.run_raw(
        root,
        &[
            "update-ref".to_string(),
            "-d".to_string(),
            BACKUP_REF.to_string(),
        ],
    )?;
    Ok(())
}

/// Render a plan as the raw REBASE-TODO text (screen 17's editable todo):
/// one `<verb> <sha> <subject>` line per entry, git's own todo format so the
/// buffer can be round-tripped back through [`parse_todo`].
pub fn render_todo(plan: &[RebasePlanEntry]) -> String {
    let mut text = String::new();
    for e in plan {
        let verb = match e.action {
            RebaseAction::Pick => "pick",
            RebaseAction::Reword => "reword",
            RebaseAction::Edit => "edit",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        };
        text.push_str(&format!("{} {} {}\n", verb, e.commit, e.subject));
    }
    text
}

/// Parse an edited REBASE-TODO back into a plan (screen 17's Log tab):
/// `<verb> <sha> <subject>` per line; comment (`#`) and blank lines are
/// skipped. An unknown verb is an error — a typo must not silently rewrite
/// history.
pub fn parse_todo(text: &str) -> TgResult<Vec<RebasePlanEntry>> {
    let mut plan = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((verb, rest)) = line.split_once(' ') else {
            return Err(TgError::Other(format!(
                "malformed rebase-todo line: {line}"
            )));
        };
        let action = match verb {
            "pick" | "p" => RebaseAction::Pick,
            "reword" | "r" => RebaseAction::Reword,
            "edit" | "e" => RebaseAction::Edit,
            "squash" | "s" => RebaseAction::Squash,
            "fixup" | "f" => RebaseAction::Fixup,
            "drop" | "d" => RebaseAction::Drop,
            other => {
                return Err(TgError::Other(format!("unknown rebase-todo verb: {other}")));
            }
        };
        let Some((commit, subject)) = rest.split_once(' ') else {
            return Err(TgError::Other(format!(
                "malformed rebase-todo line: {line}"
            )));
        };
        plan.push(RebasePlanEntry {
            action,
            commit: commit.to_string(),
            subject: subject.to_string(),
        });
    }
    Ok(plan)
}

/// One fold group of the result preview (screen 17): the squash/fixup
/// entries that collapse into their nearest kept ancestor commit. Plain
/// data; the UI renders "X + Y folded into Z" from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldSummary {
    /// The entries folded away, in plan order.
    pub folded: Vec<String>,
    /// The surviving commit they fold into.
    pub into: String,
}

/// What running a plan will produce, computed before anything runs
/// (screen 17's RESULT PREVIEW): the surviving commit sequence, the
/// before → after counts, the fold groups, and the dropped count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanPreview {
    /// Entries in the plan (the "N" in "N → M COMMITS").
    pub before: usize,
    /// Commits the replay will create (the "M").
    pub after: usize,
    /// The post-plan commit sequence, in replay order.
    pub kept: Vec<RebasePlanEntry>,
    /// Consecutive squash/fixup runs grouped by their fold target.
    pub folds: Vec<FoldSummary>,
    /// Entries removed by the Drop action.
    pub dropped: usize,
}

/// Compute the post-plan commit sequence for the RESULT PREVIEW (screen
/// 17). Squash/fixup entries collapse into the nearest preceding surviving
/// commit (a run of them becomes one [`FoldSummary`]); Drop entries are
/// counted and removed; pick/reword/edit entries survive verbatim. A
/// squash/fixup with no preceding survivor has nothing to fold into and
/// survives as-is, matching git's own behaviour of operating on the commit
/// above.
pub fn plan_preview(plan: &[RebasePlanEntry]) -> PlanPreview {
    let mut kept: Vec<RebasePlanEntry> = Vec::new();
    let mut folds: Vec<FoldSummary> = Vec::new();
    let mut dropped = 0usize;
    for e in plan {
        match e.action {
            RebaseAction::Pick | RebaseAction::Reword | RebaseAction::Edit => {
                kept.push(e.clone());
            }
            RebaseAction::Squash | RebaseAction::Fixup => {
                match kept.last().map(|k| k.commit.clone()) {
                    // A run of consecutive folds shares one summary; an
                    // intervening pick starts a new one.
                    Some(into) => match folds.last_mut() {
                        Some(f) if f.into == into => f.folded.push(e.commit.clone()),
                        _ => folds.push(FoldSummary {
                            folded: vec![e.commit.clone()],
                            into,
                        }),
                    },
                    // Nothing above to fold into — the entry survives as-is.
                    None => kept.push(e.clone()),
                }
            }
            RebaseAction::Drop => dropped += 1,
        }
    }
    PlanPreview {
        before: plan.len(),
        after: kept.len(),
        kept,
        folds,
        dropped,
    }
}

/// A pre-flight warning the editor's CAUTIONS rail renders (screen 17),
/// computed from the plan and the live repository before anything runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebaseCaution {
    /// Files touched by more than one planned commit — replaying them in
    /// sequence may conflict.
    ConflictRisk { files: usize },
    /// The planned commits carry more than one author identity.
    MixedIdentities { authors: usize },
}

/// Compute the CAUTIONS (screen 17) for a plan over `root`: the conflict
/// likelihood (files touched by several planned commits, via
/// [`GitExecutor::commit_files`]) and mixed committer identities (distinct
/// author names, via the log). Only computable cautions are returned — a
/// clean single-author plan yields an empty list.
pub fn cautions(
    vcs: &dyn GitExecutor,
    root: &Path,
    plan: &[RebasePlanEntry],
) -> Vec<RebaseCaution> {
    let mut cautions = Vec::new();

    // Conflict likelihood: a file touched by more than one planned commit.
    let mut per_file: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in plan {
        if e.action == RebaseAction::Drop {
            continue;
        }
        if let Ok(files) = vcs.commit_files(root, &e.commit) {
            for f in files {
                *per_file.entry(f.path.display().to_string()).or_default() += 1;
            }
        }
    }
    let risky = per_file.values().filter(|n| **n > 1).count();
    if risky > 0 {
        cautions.push(RebaseCaution::ConflictRisk { files: risky });
    }

    // Mixed identities: distinct author names across the planned commits.
    let shas: std::collections::HashSet<&str> = plan.iter().map(|e| e.commit.as_str()).collect();
    let mut authors: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(commits) = vcs.log(root, &LogOpts::default()) {
        for c in commits {
            if shas.contains(c.id.as_str()) {
                authors.insert(c.author.name);
            }
        }
    }
    if authors.len() > 1 {
        cautions.push(RebaseCaution::MixedIdentities {
            authors: authors.len(),
        });
    }

    cautions
}

/// The selected commit's first parent — the default rebase base when the
/// editor opens (screen 17's "Interactive rebase onto …"). `None` for an
/// unknown commit or a rootless commit.
pub fn base_of(vcs: &dyn GitExecutor, root: &Path, commit: &str) -> Option<String> {
    let commits = vcs.log(root, &LogOpts::default()).ok()?;
    commits
        .iter()
        .find(|c| c.id.as_str() == commit)?
        .parents
        .first()
        .map(|p| p.to_string())
}

/// The footer's time estimate (screen 17): roughly three seconds per
/// actually replayed commit — dropped entries never run.
pub fn estimate(plan: &[RebasePlanEntry]) -> String {
    let replays = plan
        .iter()
        .filter(|e| e.action != RebaseAction::Drop)
        .count();
    format!("~{}s estimated", 3 * replays)
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
