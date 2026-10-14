//! Live cherry-pick-across run monitor (issue 16, screen 05): plain data
//! owned by the app crate, seeded when the dialog's run dispatches and
//! advanced by [`crate::events::AppEvent::BulkRunProgress`] in the drain
//! loop — the same pool events that drive the bulk monitor, one row per
//! target repo with its commits applied in order inside the row's step.

use std::time::{Duration, Instant};

use turbogit_services::bulk_run::{self, RunControl, RunRow};

/// The live monitor for one cherry-pick-across run. There is no retry
/// pass and no undo: a conflicted repo is held for manual resolution and
/// re-run through the dialog, not the monitor.
pub struct CherryRunView {
    /// The source repository the picks were fetched from (for the header).
    pub source_name: String,
    /// How many commits the run carries.
    pub commit_count: usize,
    /// Whether a conflicted pick halts that repo's remaining commits.
    pub stop_on_conflict: bool,
    pub rows: Vec<RunRow>,
    /// Worker-slot count of the run (the "/4" of "slot 2/4").
    pub workers: usize,
    /// When the run started; the elapsed ticker reads it live.
    pub started_at: Instant,
    /// Elapsed snapshot taken at the last progress event.
    pub elapsed: Duration,
    /// ETA snapshot: average finished duration × remaining work / workers.
    pub eta: Option<Duration>,
    /// Stop handle for the in-flight pass.
    pub control: RunControl,
    /// Identity of the run in the recent-operations history (unused for
    /// cherry runs today, kept for parity with the bulk monitor).
    pub history_id: u64,
}

impl CherryRunView {
    /// Footer tallies `(done, running, queued, skipped, failed)`, live.
    pub fn tally(&self) -> (usize, usize, usize, usize, usize) {
        bulk_run::tally(&self.rows)
    }

    /// The header's "N of M done" numerator.
    pub fn done_count(&self) -> usize {
        bulk_run::done_count(&self.rows)
    }

    /// The header's "M": one row per target repo.
    pub fn total(&self) -> usize {
        bulk_run::total(&self.rows)
    }

    /// The git work the running rows are executing right now.
    pub fn running_command(&self) -> String {
        format!(
            "git fetch {} · git cherry-pick ({})",
            self.source_name,
            if self.stop_on_conflict {
                "stop on first conflict"
            } else {
                "continue past conflicts"
            }
        )
    }

    /// Recompute the elapsed/ETA snapshots from the finished rows.
    pub fn update_progress(&mut self) {
        let (elapsed, eta) = bulk_run::update_progress(&self.rows, self.workers, self.started_at);
        self.elapsed = elapsed;
        self.eta = eta;
    }
}
