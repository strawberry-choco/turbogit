use std::path::{Path, PathBuf};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{CommitId, DisplayName, LogOpts, RootId};
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

/// The head message every checked repo agrees on. `None` when the set is empty,
/// any repo has no head or cannot be read, or the heads differ: a repo this
/// rule could not ask disqualifies the seed instead of being dropped from the
/// comparison, so `--amend` can never rewrite a HEAD the rule never saw.
pub fn shared_head_message(vcs: &dyn GitExecutor, paths: &[PathBuf]) -> Option<String> {
    let mut shared: Option<String> = None;
    for path in paths {
        let head = vcs
            .log(
                path,
                &LogOpts {
                    max_count: Some(1),
                    ..Default::default()
                },
            )
            .ok()?
            .first()?
            .message
            .trim()
            .to_owned();
        match &shared {
            Some(existing) if *existing != head => return None,
            Some(_) => {}
            None => shared = Some(head),
        }
    }
    shared
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

    fn head_commit(message: &str) -> turbogit_domain::model::Commit {
        turbogit_domain::model::Commit {
            id: "abc".into(),
            parents: vec![],
            author: turbogit_domain::model::Signature {
                name: "t".into(),
                email: "t@t".into(),
                time: 0,
            },
            committer: turbogit_domain::model::Signature {
                name: "t".into(),
                email: "t@t".into(),
                time: 0,
            },
            message: message.into(),
            time: 0,
            root: RootId(Path::new("/r").into()),
            signature: Default::default(),
        }
    }

    #[test]
    fn shared_head_message_seeds_a_single_target_and_trims() {
        let mut engine = FakeExecutor::default();
        engine
            .logs
            .insert("/r/a".into(), vec![head_commit("  fix: thing  \n\n  ")]);
        assert_eq!(
            shared_head_message(&engine, &["/r/a".into()]).as_deref(),
            Some("fix: thing"),
            "the seeded value is the trimmed full message"
        );
    }

    #[test]
    fn shared_head_message_seeds_once_for_agreeing_targets() {
        let mut engine = FakeExecutor::default();
        engine.logs.insert(
            "/r/a".into(),
            vec![head_commit("chore: shared"), head_commit("old a")],
        );
        engine.logs.insert(
            "/r/b".into(),
            vec![head_commit("chore: shared"), head_commit("old b")],
        );
        assert_eq!(
            shared_head_message(&engine, &["/r/a".into(), "/r/b".into()]).as_deref(),
            Some("chore: shared"),
            "only each head votes; deeper history differing proves the one-commit bound"
        );
    }

    #[test]
    fn shared_head_message_withholds_when_one_of_three_disagrees() {
        let mut engine = FakeExecutor::default();
        engine.logs.insert("/r/a".into(), vec![head_commit("same")]);
        engine.logs.insert("/r/b".into(), vec![head_commit("same")]);
        engine
            .logs
            .insert("/r/c".into(), vec![head_commit("different")]);
        assert_eq!(
            shared_head_message(&engine, &["/r/a".into(), "/r/b".into(), "/r/c".into()]),
            None
        );
    }

    #[test]
    fn shared_head_message_withholds_for_a_repo_with_no_head() {
        let mut engine = FakeExecutor::default();
        engine.logs.insert("/r/a".into(), vec![head_commit("same")]);
        engine.logs.insert("/r/unborn".into(), vec![]);
        assert_eq!(
            shared_head_message(&engine, &["/r/a".into(), "/r/unborn".into()]),
            None,
            "an unborn repo disqualifies the seed instead of being skipped"
        );
    }

    #[test]
    fn shared_head_message_seeds_nothing_for_an_empty_target_list() {
        let engine = FakeExecutor::default();
        assert_eq!(shared_head_message(&engine, &[]), None);
    }
}
