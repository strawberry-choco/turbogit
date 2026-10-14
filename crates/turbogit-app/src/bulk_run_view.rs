//! Live cascade-run monitor state (issue 10, screen 03): plain data owned by
//! the app crate, seeded when a confirmed bulk plan dispatches and advanced
//! by [`crate::events::AppEvent::BulkRunProgress`] in the drain loop. The UI
//! renders it; it never derives run state itself.

use std::time::{Duration, Instant};

use turbogit_domain::model::{MergeOpts, RootId};
use turbogit_services::bulk_ops::BulkOp;
use turbogit_services::bulk_run::{self, RowState, RunControl, RunRow};

/// The live monitor for one fleet run (or one retry pass of it).
pub struct BulkRunView {
    pub op: BulkOp,
    /// Pull policy, needed to label the running git command. For a
    /// create-&-checkout run it is the apply-broadly policy.
    pub rebase: bool,
    /// Target branch of a create-&-checkout run (empty for other ops).
    pub branch: String,
    /// The user-typed git command of a custom-command run (issue 13, empty
    /// for built-in ops), verbatim as entered.
    pub command: String,
    /// The merge dialog's chosen options for a cascade-merge run (issue 28,
    /// default for other ops). Kept on the view so Retry-skipped can
    /// re-dispatch exactly what was confirmed.
    pub merge_opts: MergeOpts,
    pub rows: Vec<RunRow>,
    /// Worker-slot count of the run (the "/4" of "slot 2/4").
    pub workers: usize,
    /// When the run (first pass) started; the elapsed ticker reads it live.
    pub started_at: Instant,
    /// Elapsed snapshot taken at the last progress event.
    pub elapsed: Duration,
    /// ETA snapshot: average finished duration × remaining work / workers.
    /// `None` while nothing has finished or nothing remains.
    pub eta: Option<Duration>,
    /// Stop handle for the in-flight pass.
    pub control: RunControl,
    /// Identity of the run in the recent-bulk-operations history (issue 12):
    /// fixed at dispatch so retry passes merge into one record.
    pub history_id: u64,
    /// Per-repo undo information captured at dispatch (issue 12): the prior
    /// checkout and whether the run creates the branch. Empty for operations
    /// with no rollback.
    pub undo: Vec<crate::bulk_history::UndoRow>,
}

impl BulkRunView {
    /// Footer tallies `(done, running, queued, skipped, failed)`, live.
    pub fn tally(&self) -> (usize, usize, usize, usize, usize) {
        bulk_run::tally(&self.rows)
    }

    /// The header's "N of M done" numerator.
    pub fn done_count(&self) -> usize {
        bulk_run::done_count(&self.rows)
    }

    /// The header's "M": one row per selected repo.
    pub fn total(&self) -> usize {
        bulk_run::total(&self.rows)
    }

    /// The git command the running rows are executing right now — the live
    /// output line of the monitor (the engine port has no streaming, so the
    /// command plus per-row stderr on failure is the output surface).
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

    /// Recompute the elapsed/ETA snapshots from the finished rows. Called by
    /// the drain loop on every progress event.
    pub fn update_progress(&mut self) {
        let (elapsed, eta) = bulk_run::update_progress(&self.rows, self.workers, self.started_at);
        self.elapsed = elapsed;
        self.eta = eta;
    }

    /// Reset rows to queued for a retry pass of `roots` (issue 10: Retry
    /// skipped re-dispatches skipped rows through the pool).
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
