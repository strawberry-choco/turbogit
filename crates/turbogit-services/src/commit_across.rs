//! Cascade commit (issue 21, screen 06): fan out one commit (or amend) to
//! every selected root, skipping roots with nothing staged and reporting
//! per-repo outcomes through the same `Result`-shaped channel the rest of
//! the cascade fleet uses (issue 09's `run_bulk`).
//!
//! Two seams:
//! - [`plan`] classifies every selected root by whether its index has any
//!   staged content (the `git status` snapshot read through the engine
//!   port, never cached — the cascade button must answer against live state).
//! - [`run_one`] executes the per-repo commit through [`GitExecutor::commit`]
//!   (the `-a` form, which only commits tracked changes that are also
//!   staged; untracked files are not in the index, so they cannot be
//!   committed in the first place).
//!
//! The fan-out itself — pool, monitor, events, toasts — is composed from the
//! existing [`crate::bulk_run`] engine by the app layer; this module is the
//! pure per-repo step.

use std::path::{Path, PathBuf};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{CommitId, RootId};
use turbogit_engine_api::GitExecutor;

/// Why a root will not take part in the cascade commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The root's index has nothing staged — `git commit` would refuse, and
    /// skipping rather than failing is the contract (issue 21 "repos with
    /// nothing staged are skipped and reported, not failed").
    NoStagedContent,
}

impl SkipReason {
    /// The preflight label — the parenthesized reason the monitor renders on
    /// a skipped row.
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::NoStagedContent => "nothing staged",
        }
    }
}

/// One row of the cascade-commit preflight: a selected repo, its current
/// state, and whether the commit will run (with the staged-change count for
/// the rail footer) or be skipped (with reason).
#[derive(Debug, Clone)]
pub struct PlanRow {
    pub root: RootId,
    /// Repo display name (the directory's file name).
    pub name: String,
    /// `Ok(staged_count)` when the root will run — the staged-change count
    /// is what the commit rail shows ("Commit M hunks"). `Err(reason)` when
    /// it will be skipped.
    pub outcome: Result<usize, SkipReason>,
}

/// Classify every selected root for a cascade commit: live-read each root's
/// status through the engine port and report which roots have anything
/// staged. Roots the engine cannot read surface as a `NoStagedContent`
/// skip — the cascade step is doomed and a `run_one` later would fail with
/// the same "nothing to commit" error, so skipping here is honest
/// reporting, not a policy choice.
pub fn plan(vcs: &dyn GitExecutor, paths: &[PathBuf]) -> Vec<PlanRow> {
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let staged = vcs
                .status(path)
                .ok()
                .map(|status| {
                    status
                        .changes
                        .iter()
                        .filter(|c| {
                            c.staged
                                && !matches!(
                                    c.status,
                                    turbogit_domain::model::ChangeStatus::Conflicted
                                )
                        })
                        .count()
                })
                .unwrap_or(0);
            let outcome = if staged == 0 {
                Err(SkipReason::NoStagedContent)
            } else {
                Ok(staged)
            };
            PlanRow {
                root: RootId(path.as_path().into()),
                name,
                outcome,
            }
        })
        .collect()
}

/// Execute the cascade commit's step on one root: commit (or amend) whatever
/// is in the index with `message`. `amend` passes through to
/// [`GitExecutor::commit`]. Returns the new HEAD SHA on success; an empty
/// index surfaces the engine's "nothing to commit" as a `TgError` for the
/// caller to surface through the cascade-run monitor.
pub fn run_one(
    vcs: &dyn GitExecutor,
    root: &Path,
    message: &str,
    amend: bool,
) -> TgResult<CommitId> {
    vcs.commit(root, message, amend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_engine::fake::FakeExecutor;

    #[test]
    fn plan_classifies_fake_status_into_will_run_and_skip() {
        // The pure classification logic on the fake executor — independent
        // of real git. Two roots, one with a staged file, one clean.
        let mut engine = FakeExecutor::default();
        engine.status.insert(
            "/r/staged".into(),
            turbogit_domain::model::RootStatus {
                changes: vec![turbogit_domain::model::Change {
                    path: "feature.txt".into(),
                    status: turbogit_domain::model::ChangeStatus::Modified,
                    chunks: vec![],
                    staged: true,
                    unstaged: false,
                    orig_path: None,
                }],
                conflicted: vec![],
            },
        );
        engine.status.insert(
            "/r/empty".into(),
            turbogit_domain::model::RootStatus::default(),
        );

        let paths: Vec<PathBuf> = vec!["/r/staged".into(), "/r/empty".into()];
        let rows = plan(&engine, &paths);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].outcome, Ok(1));
        assert_eq!(rows[1].outcome, Err(SkipReason::NoStagedContent));
        assert_eq!(rows[1].name, "empty");
    }

    #[test]
    fn plan_treats_conflicted_files_as_not_staged() {
        // Conflicted files cannot be committed (issue 22 owns the resolver).
        let mut engine = FakeExecutor::default();
        engine.status.insert(
            "/r/conflict".into(),
            turbogit_domain::model::RootStatus {
                changes: vec![turbogit_domain::model::Change {
                    path: "merged.txt".into(),
                    status: turbogit_domain::model::ChangeStatus::Conflicted,
                    chunks: vec![],
                    staged: true,
                    unstaged: true,
                    orig_path: None,
                }],
                conflicted: vec!["merged.txt".into()],
            },
        );
        let rows = plan(&engine, &["/r/conflict".into()]);
        assert_eq!(rows[0].outcome, Err(SkipReason::NoStagedContent));
    }
}
