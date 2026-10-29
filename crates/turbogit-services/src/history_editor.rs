use std::path::Path;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::{GitExecutor, OwnedOperation};

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
            subject: subject_of_message(&c.message).to_string(),
            message: None,
        })
        .collect();

    plan.reverse();
    Ok(plan)
}

/// Set the [`RebaseAction`] of the plan entry at `index`, if in range.
///
/// Setting [`RebaseAction::Pick`] RESETS the row ("this commit is applied as
/// is", the reference's per-row way back): the replacement message goes with
/// it, so a reworded row clicked back to pick replays the commit untouched.
/// Every other verb is a retarget, not a reset, and keeps the message.
pub fn set_action(plan: &mut [RebasePlanEntry], index: usize, action: RebaseAction) {
    if let Some(entry) = plan.get_mut(index) {
        if action == RebaseAction::Pick {
            entry.message = None;
        }
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

pub fn execute(vcs: &dyn GitExecutor, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
    vcs.rebase_interactive(root, plan)
}

/// Execute a rebase plan with the backup-ref safety net (issue 30): the
/// guarded dispatch (protected branches refuse before anything runs), then the
/// engine's rewrite backup is written at the current HEAD, then the plan
/// replays. Which reference that is, the engine decides.
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
    vcs.save_rewrite_backup(root)?;
    vcs.rebase_interactive(root, plan)
}

/// The plan one targeted history verb runs: from the target commit's first
/// parent through the current tip, with exactly one row set to `action`.
///
/// Built over the FIRST PARENT rather than the target so the target itself is
/// in the replayed set, which is what lets one row carry the verb while every
/// descendant is replayed on top of it unchanged. A commit that is not an
/// ancestor of the tip has no row here, so it gets an error rather than a todo
/// git rejects; the menu states that bound on the item before it is reached.
pub fn targeted_plan(
    vcs: &dyn GitExecutor,
    root: &Path,
    commit: &str,
    action: RebaseAction,
    message: Option<String>,
) -> TgResult<Vec<RebasePlanEntry>> {
    let base = base_of(vcs, root, commit).ok_or_else(|| {
        TgError::Other(format!(
            "{commit} has no first parent to rewrite from — it is a root commit or is unknown"
        ))
    })?;
    let mut plan = build_plan(vcs, root, &base)?;
    let target = plan
        .iter_mut()
        .find(|e| e.commit == commit)
        .ok_or_else(|| {
            TgError::Other(format!(
                "{commit} is not on the current branch, so it cannot be edited"
            ))
        })?;
    target.action = action;
    target.message = message;
    Ok(plan)
}

/// Drop ONE named commit: rewrite the current branch as if it had never been
/// made, replaying everything built on top of it.
///
/// Goes through [`execute_with_backup`], so a protected branch refuses before
/// git is touched, the rewrite backup ref is written first, and the recovery
/// path can come back to this state. `branch` is the branch being rewritten and
/// is what the protected-branch guard reads when HEAD is detached.
pub fn drop_commit(
    vcs: &dyn GitExecutor,
    root: &Path,
    commit: &str,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    let plan = targeted_plan(vcs, root, commit, RebaseAction::Drop, None)?;
    execute_with_backup(vcs, root, &plan, settings, branch)
}

/// Reword ONE named commit, leaving its content, author and date alone.
///
/// The same single path as [`drop_commit`], one call apart: the new message
/// rides on the plan row rather than opening a second rewrite machinery, and the
/// engine substitutes it for git's editor (ADR-0025).
pub fn reword_commit(
    vcs: &dyn GitExecutor,
    root: &Path,
    commit: &str,
    message: &str,
    settings: &VcsSettings,
    branch: &str,
) -> TgResult<()> {
    let plan = targeted_plan(
        vcs,
        root,
        commit,
        RebaseAction::Reword,
        Some(message.to_string()),
    )?;
    execute_with_backup(vcs, root, &plan, settings, branch)
}

/// Abort the running rebase and restore the pre-rebase state from the engine's
/// rewrite backup (screen 17's RECOVERY action): an in-progress rebase is
/// aborted first, then the branch returns to the backup, and the spent backup is
/// discarded. Errors when no backup exists, the net is only present while an
/// [`execute_with_backup`] replay is the last rewrite.
pub fn abort_to_backup(vcs: &dyn GitExecutor, root: &Path) -> TgResult<()> {
    if crate::integrate_service::operation_in_progress(vcs.holding_operation(root)) {
        vcs.abort(root, OwnedOperation::Rebase)?;
    }
    vcs.restore_rewrite_backup(root)?;
    vcs.discard_rewrite_backup(root)?;
    Ok(())
}

/// Post-hoc undo of the last targeted rewrite: restore the branch
/// from the engine's rewrite backup and discard the spent backup, the same
/// two verbs [`abort_to_backup`] runs once an abort is out of the way, without
/// the reconstruction a `git reflog` walk would need.
///
/// `expected_tip` is the HEAD the rewrite finished on, captured when the undo
/// was armed. The guard runs HERE, at undo time, never at rewrite time: if
/// HEAD has moved since, a second operation in the same root, the undo is
/// refused and NOTHING is restored, because `restore_rewrite_backup` resets
/// hard and would destroy whatever landed on top. The backup ref survives the
/// refusal; refusing is not discarding.
pub fn undo_last_rewrite(vcs: &dyn GitExecutor, root: &Path, expected_tip: &str) -> TgResult<()> {
    let current = vcs.resolve_revision(root, "HEAD")?;
    if current != expected_tip {
        return Err(TgError::Other(format!(
            "the repository has already been changed since the rewrite (HEAD is {current}, \
             the undo expects {expected_tip})"
        )));
    }
    vcs.restore_rewrite_backup(root)?;
    vcs.discard_rewrite_backup(root)
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
            RebaseAction::UpdateRef => "update-ref",
            RebaseAction::Unknown => "unknown",
        };
        text.push_str(&format!("{} {} {}\n", verb, e.commit, e.subject));
    }
    text
}

/// Parse an edited REBASE-TODO back into a plan (screen 17's Log tab):
/// `<verb> <sha> <subject>` per line; comment (`#`) and blank lines are
/// skipped. A verb the parser does not recognise falls back to
/// [`RebaseAction::Unknown`], the row survives so the editor can render it
/// and the engine can refuse the plan, rather than the line being dropped or
/// silently rewritten as a verb it never was.
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
            "update-ref" | "u" => RebaseAction::UpdateRef,
            _ => RebaseAction::Unknown,
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
            message: None,
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
    pub into: String,
}

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
            RebaseAction::Pick
            | RebaseAction::Reword
            | RebaseAction::Edit
            | RebaseAction::UpdateRef
            | RebaseAction::Unknown => {
                kept.push(e.clone());
            }
            RebaseAction::Squash | RebaseAction::Fixup => {
                match kept.last().map(|k| k.commit.clone()) {
                    Some(into) => match folds.last_mut() {
                        Some(f) if f.into == into => f.folded.push(e.commit.clone()),
                        _ => folds.push(FoldSummary {
                            folded: vec![e.commit.clone()],
                            into,
                        }),
                    },
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebaseCaution {
    /// Files touched by more than one planned commit, replaying them in
    /// sequence may conflict.
    ConflictRisk { files: usize },
    /// The planned commits carry more than one author identity.
    MixedIdentities { authors: usize },
}

/// Compute the CAUTIONS (screen 17) for a plan over `root`: the conflict
/// likelihood (files touched by several planned commits, via
/// [`GitExecutor::commit_files`]) and mixed committer identities (distinct
/// author names, via the log). Only computable cautions are returned, a
/// clean single-author plan yields an empty list.
pub fn cautions(
    vcs: &dyn GitExecutor,
    root: &Path,
    plan: &[RebasePlanEntry],
) -> Vec<RebaseCaution> {
    let mut cautions = Vec::new();

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

/// The selected commit's first parent, the default rebase base when the
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
/// actually replayed commit, dropped entries never run.
pub fn estimate(plan: &[RebasePlanEntry]) -> String {
    let replays = plan
        .iter()
        .filter(|e| e.action != RebaseAction::Drop)
        .count();
    format!("~{}s estimated", 3 * replays)
}
