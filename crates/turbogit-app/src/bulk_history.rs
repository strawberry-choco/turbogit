use serde::{Deserialize, Serialize};

use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::RowState;

pub const MAX_HISTORY: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoOutcome {
    Done,
    Failed { error: String },
    Skipped { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRecord {
    pub root: std::path::PathBuf,
    pub name: String,
    pub prior_branch: Option<String>,
    pub created: bool,
    pub outcome: RepoOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BulkRunRecord {
    pub id: u64,
    pub at: i64,
    pub op: BulkOp,
    pub branch: String,
    pub repos: Vec<RepoRecord>,
    pub rolled_back: bool,
}

impl BulkRunRecord {
    pub fn summary(&self) -> String {
        let ok = self
            .repos
            .iter()
            .filter(|r| r.outcome == RepoOutcome::Done)
            .count();
        let failed = self
            .repos
            .iter()
            .filter(|r| matches!(r.outcome, RepoOutcome::Failed { .. }))
            .count();
        let mut parts = vec![format!("{ok} ok")];
        let mut reasons: Vec<(String, usize)> = Vec::new();
        for r in &self.repos {
            if let RepoOutcome::Skipped { reason } = &r.outcome {
                match reasons.iter_mut().find(|(k, _)| k == reason) {
                    Some((_, n)) => *n += 1,
                    None => reasons.push((reason.clone(), 1)),
                }
            }
        }
        reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (reason, n) in reasons {
            parts.push(format!("{n} skipped ({reason})"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        parts.join(" · ")
    }

    pub fn reversible(&self) -> bool {
        self.op == BulkOp::CreateBranch
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndoRow {
    pub root: std::path::PathBuf,
    pub prior_branch: Option<String>,
    pub created: bool,
}

pub fn from_run_view(view: &crate::bulk_run_view::BulkRunView, at: i64) -> BulkRunRecord {
    let repos = view
        .rows
        .iter()
        .map(|row| {
            let undo = view
                .undo
                .iter()
                .find(|u| u.root == row.root.as_path().to_path_buf());
            RepoRecord {
                root: row.root.as_path().to_path_buf(),
                name: row.name.clone(),
                prior_branch: undo.and_then(|u| u.prior_branch.clone()),
                created: undo.map(|u| u.created).unwrap_or(false),
                outcome: match &row.state {
                    RowState::Done => RepoOutcome::Done,
                    RowState::Failed { error, .. } => RepoOutcome::Failed {
                        error: error.clone(),
                    },
                    RowState::Skipped { reason } => RepoOutcome::Skipped {
                        reason: reason.clone(),
                    },
                    RowState::Queued { .. } | RowState::Running => RepoOutcome::Skipped {
                        reason: "stopped".to_string(),
                    },
                },
            }
        })
        .collect();
    BulkRunRecord {
        id: view.history_id,
        at,
        op: view.op,
        branch: view.branch.clone(),
        repos,
        rolled_back: false,
    }
}

pub fn record_into(history: &mut Vec<BulkRunRecord>, record: BulkRunRecord) {
    if let Some(existing) = history.iter_mut().find(|r| r.id == record.id) {
        *existing = record;
        return;
    }
    history.insert(0, record);
    history.truncate(MAX_HISTORY);
}

pub fn format_time(unix_millis: i64) -> String {
    let t = chrono::DateTime::<chrono::Local>::from(
        chrono::DateTime::from_timestamp_millis(unix_millis)
            .unwrap_or(chrono::DateTime::UNIX_EPOCH),
    );
    t.format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u64, op: BulkOp, repos: Vec<RepoRecord>) -> BulkRunRecord {
        BulkRunRecord {
            id,
            at: id as i64,
            op,
            branch: String::new(),
            repos,
            rolled_back: false,
        }
    }

    fn done(name: &str) -> RepoRecord {
        RepoRecord {
            root: std::path::PathBuf::from(format!("/w/{name}")),
            name: name.to_string(),
            prior_branch: None,
            created: false,
            outcome: RepoOutcome::Done,
        }
    }

    fn skipped(name: &str, reason: &str) -> RepoRecord {
        RepoRecord {
            root: std::path::PathBuf::from(format!("/w/{name}")),
            name: name.to_string(),
            prior_branch: None,
            created: false,
            outcome: RepoOutcome::Skipped {
                reason: reason.to_string(),
            },
        }
    }

    #[test]
    fn a_completed_run_summarizes_ok_and_skipped_counts() {
        let rec = record(
            1,
            BulkOp::FetchAll,
            vec![done("alpha"), done("ui"), skipped("lib", "offline remote")],
        );
        assert_eq!(rec.summary(), "2 ok · 1 skipped (offline remote)");
    }

    #[test]
    fn skipped_reasons_group_in_the_summary() {
        let rec = record(
            1,
            BulkOp::PullAll,
            vec![
                done("alpha"),
                skipped("ui", "dirty worktree"),
                skipped("lib", "no upstream"),
                skipped("cli", "dirty worktree"),
            ],
        );
        assert_eq!(
            rec.summary(),
            "1 ok · 2 skipped (dirty worktree) · 1 skipped (no upstream)"
        );
    }

    #[test]
    fn failed_repos_total_in_the_summary() {
        let rec = record(
            1,
            BulkOp::PushAll,
            vec![
                done("alpha"),
                RepoRecord {
                    root: std::path::PathBuf::from("/w/ui"),
                    name: "ui".to_string(),
                    prior_branch: None,
                    created: false,
                    outcome: RepoOutcome::Failed {
                        error: "rejected".to_string(),
                    },
                },
            ],
        );
        assert_eq!(rec.summary(), "1 ok · 1 failed");
    }

    #[test]
    fn only_branch_cascades_are_reversible() {
        let rec = record(1, BulkOp::CreateBranch, vec![done("alpha")]);
        assert!(rec.reversible(), "branch create/checkout can be undone");

        let fetch = record(2, BulkOp::FetchAll, vec![done("alpha")]);
        assert!(!fetch.reversible(), "fetch has no undo");
    }

    #[test]
    fn recording_keeps_the_list_newest_first_and_capped() {
        let mut history: Vec<BulkRunRecord> = Vec::new();
        for i in 0..(MAX_HISTORY as i64 + 5) {
            record_into(&mut history, record(i as u64, BulkOp::FetchAll, vec![]));
        }
        assert_eq!(history.len(), MAX_HISTORY, "history is capped");
        assert_eq!(history[0].id, MAX_HISTORY as u64 + 4, "newest first");
        assert_eq!(history.last().unwrap().id, 5, "oldest dropped");
    }

    #[test]
    fn time_formats_as_the_clock_label_of_the_history_row() {
        let label = format_time(chrono::Local::now().timestamp_millis());
        assert_eq!(label.len(), 5, "HH:MM: {label}");
        assert_eq!(&label[2..3], ":");
    }
}
