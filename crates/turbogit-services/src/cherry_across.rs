//! Cherry-pick across repositories (issue 16, screen 05): apply one or more
//! source commits onto several target repositories.
//!
//! Cross-repo picks transport the source commits by fetching the source
//! repository's objects into each target (a local-path fetch through the
//! port's `fetch`) and running a real cherry-pick there. A conflicted pick
//! therefore leaves a genuine held cherry-pick state — conflicted files and
//! `CHERRY_PICK_HEAD` — which the run monitor's Resolve deep link opens;
//! nothing is ever auto-resolved.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{Commit, DiffOpts, LogOpts, RootId};
use turbogit_engine_api::GitExecutor;

use crate::integrate_service;

/// How far back the already-present scan looks.
const PRESENT_SCAN: usize = 500;

/// The predicted risk of applying a commit selection to one target repo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetRisk {
    /// Every pick checks clean against identical file content.
    Low,
    /// The picks apply, but the touched files drifted from the source's
    /// parent versions — offsets and surprises are likely.
    Medium,
    /// At least one pick is predicted to conflict.
    High,
}

/// The prediction for one commit against one target repo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickForecast {
    /// `git apply --check` passes and the touched files match the source's
    /// parent versions.
    CleanApply,
    /// The patch applies, but the touched files drifted — medium risk.
    CleanApplyDrifted,
    /// The patch does not apply; `failed_hunks` is how many of the patch's
    /// hunks overlap. The run will stop for manual resolution.
    Conflict { failed_hunks: usize },
    /// A commit with the same subject is already in the target's history.
    AlreadyPresent,
}

/// The prediction for one target repo (one row of the targets table).
#[derive(Clone, Debug)]
pub struct TargetForecast {
    pub root: RootId,
    pub name: String,
    /// How many commits the selection holds.
    pub total: usize,
    /// How many will actually be attempted (not already present).
    pub applies: usize,
    pub risk: TargetRisk,
    /// A repo-level reason the run will skip this target entirely
    /// ("working tree is dirty"); `None` when the target can run.
    pub blocked: Option<String>,
    /// One forecast per picked commit, in application order.
    pub picks: Vec<PickForecast>,
}

/// Predict, for every target repo, how the picked commits will land: how
/// many apply, at what risk, and per-commit outcome. Reads only — a check
/// never mutates the target. `targets` are (id, name, path) triples.
pub fn forecast(
    exec: &dyn GitExecutor,
    source: &Path,
    picks: &[Commit],
    targets: &[(RootId, String, PathBuf)],
) -> Vec<TargetForecast> {
    let source_subjects: HashSet<String> = picks.iter().map(|c| subject(&c.message)).collect();
    // Patch texts are per source commit, shared across targets.
    let mut patches: HashMap<String, String> = HashMap::new();

    targets
        .iter()
        .map(|(root, name, path)| {
            let mut forecast = TargetForecast {
                root: root.clone(),
                name: name.clone(),
                total: picks.len(),
                applies: 0,
                risk: TargetRisk::Low,
                blocked: None,
                picks: Vec::new(),
            };
            let dirty = matches!(exec.status(path), Ok(s) if is_dirty(&s));
            if dirty {
                forecast.blocked = Some("working tree is dirty".to_string());
                return forecast;
            }
            let present: HashSet<String> = match history_subjects(exec, path) {
                Ok(s) => s.into_iter().collect(),
                Err(_) => HashSet::new(),
            };
            for pick in picks {
                let pick_forecast = if present.contains(&subject(&pick.message))
                    && source_subjects.contains(&subject(&pick.message))
                {
                    PickForecast::AlreadyPresent
                } else {
                    let patch = patches
                        .entry(pick.id.clone())
                        .or_insert_with(|| {
                            exec.diff(
                                source,
                                &DiffOpts {
                                    commit: Some(pick.id.clone()),
                                    ..Default::default()
                                },
                            )
                            .unwrap_or_default()
                        })
                        .clone();
                    match exec.check_patch(path, &patch) {
                        Ok(()) => {
                            if files_drifted(exec, source, path, pick) {
                                PickForecast::CleanApplyDrifted
                            } else {
                                PickForecast::CleanApply
                            }
                        }
                        Err(e) => {
                            let stderr = match &e {
                                TgError::Cli { stderr, .. } => stderr.clone(),
                                other => other.to_string(),
                            };
                            PickForecast::Conflict {
                                failed_hunks: failed_hunks(&patch, &stderr).max(1),
                            }
                        }
                    }
                };
                if !matches!(pick_forecast, PickForecast::AlreadyPresent) {
                    forecast.applies += 1;
                }
                forecast.risk = match (&pick_forecast, forecast.risk) {
                    (PickForecast::Conflict { .. }, _) => TargetRisk::High,
                    (PickForecast::CleanApplyDrifted, TargetRisk::Low) => TargetRisk::Medium,
                    (_, risk) => risk,
                };
                forecast.picks.push(pick_forecast);
            }
            forecast
        })
        .collect()
}

/// The files a commit touches, for the drift comparison.
fn touched_files(exec: &dyn GitExecutor, source: &Path, pick: &Commit) -> Vec<PathBuf> {
    exec.commit_files(source, &pick.id)
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.path)
        .collect()
}

/// Whether any touched file's content at the target's HEAD differs from the
/// source commit's parent version (or its presence differs). A drifted file
/// still applies — with offsets — so it rates medium risk.
fn files_drifted(exec: &dyn GitExecutor, source: &Path, target: &Path, pick: &Commit) -> bool {
    let parent: Option<String> = pick.parents.first().cloned();
    touched_files(exec, source, pick).iter().any(|path| {
        let base = parent
            .as_deref()
            .and_then(|rev| show(exec, source, rev, path));
        let head = show(exec, target, "HEAD", path);
        base != head
    })
}

fn show(exec: &dyn GitExecutor, root: &Path, rev: &str, path: &Path) -> Option<String> {
    exec.show_file(root, rev, path).ok()
}

/// Anything tracked that is modified, staged, or conflicted counts as a
/// dirty tree; unversioned and ignored files do not block a pick.
fn is_dirty(status: &turbogit_domain::model::RootStatus) -> bool {
    use turbogit_domain::model::ChangeStatus::*;
    status
        .changes
        .iter()
        .any(|c| !matches!(c.status, Unversioned | Ignored))
}

/// How many of the patch's hunks git reported failing. The stderr's
/// "error: patch failed: <file>:<line>" locations are matched against the
/// patch's `@@ -start,count` headers (per-file line numbers can alias, so
/// this is a count of distinct matching hunks, not an exact per-file map);
/// any unparsable non-empty stderr counts as one.
pub fn failed_hunks(patch: &str, stderr: &str) -> usize {
    let hunks: Vec<(usize, usize)> = patch
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("@@ -")?;
            let mut it = rest.split(&[',', ' '][..]);
            let start: usize = it.next()?.parse().ok()?;
            let count: usize = it.next().and_then(|c| c.parse().ok()).unwrap_or(1);
            Some((start, start.saturating_sub(1) + count))
        })
        .collect();
    if stderr.trim().is_empty() {
        return 0;
    }
    let failed: HashSet<usize> = stderr
        .lines()
        .filter_map(|l| l.split("patch failed: ").nth(1))
        .filter_map(|loc| loc.split(':').nth(1)?.trim().parse::<usize>().ok())
        .filter_map(|line| {
            hunks
                .iter()
                .position(|&(start, end)| line >= start && line < end)
        })
        .collect();
    if failed.is_empty() { 1 } else { failed.len() }
}

/// The subject (first message line) of a commit — the identity used for
/// already-present detection across repositories, where SHAs always differ.
pub fn subject(message: &str) -> String {
    message.lines().next().unwrap_or_default().to_string()
}

/// The subjects of `root`'s recent history, for already-present detection.
fn history_subjects(exec: &dyn GitExecutor, root: &Path) -> TgResult<Vec<String>> {
    Ok(exec
        .log(
            root,
            &LogOpts {
                max_count: Some(PRESENT_SCAN),
                ..Default::default()
            },
        )?
        .iter()
        .map(|c| subject(&c.message))
        .collect())
}

/// Apply `commits` (SHAs, in the given application order) onto `root`, whose
/// repository does not share history with `source`. The source's objects are
/// fetched once up front; a commit whose subject already exists in the
/// target's history is skipped rather than duplicated.
///
/// `stop_on_conflict` is the run's policy: on a conflicted pick the held
/// state is left in place for manual resolution and the remaining commits
/// are not attempted (`Err`); with the policy off the held pick is aborted
/// and the remaining commits still run — the summary `Err` then reports how
/// many applied and what failed.
pub fn apply_to_root(
    exec: &dyn GitExecutor,
    source: &Path,
    root: &Path,
    commits: &[String],
    stop_on_conflict: bool,
) -> TgResult<()> {
    if matches!(exec.status(root), Ok(s) if is_dirty(&s)) {
        return Err(TgError::Other(
            "working tree is dirty — commit or shelve your changes first".into(),
        ));
    }
    exec.fetch(root, Some(&source.to_string_lossy()))?;
    // sha -> subject, resolved once from the source's own history.
    let subjects: HashMap<String, String> = exec
        .log(
            source,
            &LogOpts {
                max_count: Some(PRESENT_SCAN),
                ..Default::default()
            },
        )?
        .into_iter()
        .map(|c| (c.id, subject(&c.message)))
        .collect();

    let mut applied = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for commit in commits {
        let subj = subjects.get(commit).cloned().unwrap_or_default();
        if !subj.is_empty() && history_subjects(exec, root)?.contains(&subj) {
            continue;
        }
        match exec.cherry_pick(root, commit) {
            Ok(()) => applied += 1,
            Err(e) => {
                let held = integrate_service::in_progress(root);
                if held && !stop_on_conflict {
                    // Policy off: release the held pick and move on.
                    let _ = exec.abort(root, "cherry-pick");
                }
                if stop_on_conflict {
                    // Hold the conflict for manual resolution; this repo's
                    // remaining commits are not attempted.
                    return Err(e);
                }
                failures.push(format!("{}: {}", short(commit), first_line(&e)));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(TgError::Other(format!(
            "applied {applied}/{} — {}",
            commits.len(),
            failures.join("; ")
        )))
    }
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn first_line(e: &TgError) -> String {
    e.to_string().lines().next().unwrap_or_default().to_string()
}
