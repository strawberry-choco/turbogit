use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::{
    Commit, DiffOpts, LogOpts, Patch, PatchFile, PatchHeaderLine, RootId, short_commit_ref,
    subject_of_message,
};
use turbogit_engine_api::{GitExecutor, OwnedOperation};

use crate::integrate_service;

const PRESENT_SCAN: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickForecast {
    CleanApply,
    CleanApplyDrifted,
    Conflict { failed_hunks: usize },
    AlreadyPresent,
}

#[derive(Clone, Debug)]
pub struct TargetForecast {
    pub root: RootId,
    pub name: String,
    pub total: usize,
    pub applies: usize,
    pub risk: TargetRisk,
    pub blocked: Option<String>,
    pub picks: Vec<PickForecast>,
}

pub fn forecast(
    exec: &dyn GitExecutor,
    source: &Path,
    picks: &[Commit],
    targets: &[(RootId, String, PathBuf)],
) -> Vec<TargetForecast> {
    let source_subjects: HashSet<String> = picks.iter().map(|c| subject(&c.message)).collect();
    let mut patches: HashMap<String, Patch> = HashMap::new();

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
                            exec.diff_patch(
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
                        Err(_) => PickForecast::Conflict {
                            failed_hunks: failed_hunks(exec, path, &patch).max(1),
                        },
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

fn touched_files(exec: &dyn GitExecutor, source: &Path, pick: &Commit) -> Vec<PathBuf> {
    exec.commit_files(source, &pick.id)
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.path)
        .collect()
}

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

fn is_dirty(status: &turbogit_domain::model::RootStatus) -> bool {
    use turbogit_domain::model::ChangeStatus::*;
    status
        .changes
        .iter()
        .any(|c| !matches!(c.status, Unversioned | Ignored))
}

/// A hunk that checks clean alone can still fail as part of the whole, so a
/// caller that already knows the whole failed keeps `.max(1)`.
pub fn failed_hunks(exec: &dyn GitExecutor, root: &Path, patch: &Patch) -> usize {
    patch
        .files
        .iter()
        .flat_map(|file| file.hunks.iter().map(move |hunk| (file, hunk)))
        .filter(|(file, hunk)| {
            let one = Patch {
                files: vec![PatchFile {
                    old_path: file.old_path.clone(),
                    new_path: file.new_path.clone(),
                    combined: file.combined,
                    headers: file
                        .headers
                        .iter()
                        .filter(|h| !matches!(h, PatchHeaderLine::Index { .. }))
                        .cloned()
                        .collect(),
                    hunks: vec![(*hunk).clone()],
                }],
            };
            exec.check_patch(root, &one).is_err()
        })
        .count()
}

pub fn subject(message: &str) -> String {
    subject_of_message(message).to_string()
}

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

/// `stop_on_conflict` is the run's policy: on a conflicted pick the held
/// state is left in place for manual resolution and the remaining commits
/// are not attempted (`Err`); with the policy off the held pick is aborted
/// and the remaining commits still run, the summary `Err` then reports how
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
            "Working tree is dirty — commit or shelve your changes first".into(),
        ));
    }
    exec.fetch(root, Some(&source.to_string_lossy()))?;
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
                let held = integrate_service::operation_in_progress(exec.holding_operation(root));
                if held && !stop_on_conflict {
                    let _ = exec.abort(root, OwnedOperation::CherryPick);
                }
                if stop_on_conflict {
                    return Err(e);
                }
                failures.push(format!("{}: {}", short_commit_ref(commit), first_line(&e)));
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

fn first_line(e: &TgError) -> String {
    subject_of_message(&e.to_string()).to_string()
}
