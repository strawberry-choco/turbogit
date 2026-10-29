use std::path::{Path, PathBuf};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{CommitId, DisplayName, RootId};
use turbogit_engine_api::GitExecutor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    NoStagedContent,
}

impl SkipReason {
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::NoStagedContent => "nothing staged",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanRow {
    pub root: RootId,
    pub name: String,
    /// `Ok(staged_count)` when the root will run, the staged-change count
    /// is what the commit rail shows ("Commit M hunks"). `Err(reason)` when
    /// it will be skipped.
    pub outcome: Result<usize, SkipReason>,
}

pub fn plan(vcs: &dyn GitExecutor, paths: &[PathBuf]) -> Vec<PlanRow> {
    paths
        .iter()
        .map(|path| {
            let name = path.display_name_or("?");
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

pub fn run_one(
    vcs: &dyn GitExecutor,
    root: &Path,
    message: &str,
    amend: bool,
) -> TgResult<CommitId> {
    vcs.commit_index(root, message, amend, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_engine::fake::FakeExecutor;

    #[test]
    fn plan_classifies_fake_status_into_will_run_and_skip() {
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
                conflict_kinds: vec![],
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
                conflict_kinds: vec![],
            },
        );
        let rows = plan(&engine, &["/r/conflict".into()]);
        assert_eq!(rows[0].outcome, Err(SkipReason::NoStagedContent));
    }
}
