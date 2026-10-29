use std::time::{Duration, Instant};

use turbogit_domain::model::{MergeOpts, RootId};
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::{self, RowState, RunControl, RunRow};

pub struct BulkRunView {
    pub op: BulkOp,
    pub rebase: bool,
    pub branch: String,
    pub command: String,
    pub merge_opts: MergeOpts,
    pub rows: Vec<RunRow>,
    pub workers: usize,
    pub started_at: Instant,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
    pub control: RunControl,
    pub history_id: u64,
    pub undo: Vec<crate::bulk_history::UndoRow>,
}

impl BulkRunView {
    pub fn tally(&self) -> (usize, usize, usize, usize, usize) {
        bulk_run::tally(&self.rows)
    }

    pub fn done_count(&self) -> usize {
        bulk_run::done_count(&self.rows)
    }

    pub fn total(&self) -> usize {
        bulk_run::total(&self.rows)
    }

    pub fn running_command(&self) -> String {
        match self.op {
            BulkOp::FetchAll => "git fetch --all".to_string(),
            BulkOp::PullAll => {
                if self.rebase {
                    "git pull --rebase".to_string()
                } else {
                    "git pull".to_string()
                }
            }
            BulkOp::PushAll => "git push".to_string(),
            BulkOp::StashAll => "git stash push".to_string(),
            BulkOp::CreateBranch => format!("git checkout -b {}", self.branch),
            BulkOp::Commit => {
                if self.rebase {
                    "git commit --amend".to_string()
                } else {
                    "git commit".to_string()
                }
            }
            BulkOp::Merge => {
                if self.branch.trim().is_empty() {
                    "git merge".to_string()
                } else {
                    format!("git merge {}", self.branch)
                }
            }
            BulkOp::Custom => turbogit_services::bulk_ops::echo_command(&self.command),
        }
    }

    pub fn update_progress(&mut self) {
        let (elapsed, eta) = bulk_run::update_progress(&self.rows, self.workers, self.started_at);
        self.elapsed = elapsed;
        self.eta = eta;
    }

    pub fn requeue(&mut self, roots: &[RootId]) {
        let workers = self.workers.max(1);
        let mut slot = 0usize;
        for row in &mut self.rows {
            if roots.iter().any(|r| r == &row.root) {
                slot += 1;
                row.state = RowState::Queued {
                    slot: bulk_run::worker_slot(slot - 1, workers),
                };
                row.duration = None;
            }
        }
    }
}
