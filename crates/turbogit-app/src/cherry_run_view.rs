//! Live cherry-pick-across run monitor (issue 16, screen 05): plain data
//! owned by the app crate, seeded when the dialog's run dispatches and
//! advanced by [`crate::events::AppEvent::BulkRunProgress`] in the drain
//! loop — the same pool events that drive the bulk monitor, one row per
//! target repo with its commits applied in order inside the row's step.

use std::time::{Duration, Instant};

use turbogit_services::bulk_run::{RowState, RunControl, RunRow};

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
        let mut t = (0, 0, 0, 0, 0);
        for row in &self.rows {
            match row.state {
                RowState::Done => t.0 += 1,
                RowState::Running => t.1 += 1,
                RowState::Queued { .. } => t.2 += 1,
                RowState::Skipped { .. } => t.3 += 1,
                RowState::Failed { .. } => t.4 += 1,
            }
        }
        t
    }

    /// The header's "N of M done" numerator.
    pub fn done_count(&self) -> usize {
        self.tally().0
    }

    /// The header's "M": one row per target repo.
    pub fn total(&self) -> usize {
        self.rows.len()
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
        self.elapsed = self.started_at.elapsed();
        let finished: Vec<Duration> = self.rows.iter().filter_map(|r| r.duration).collect();
        let remaining = self.total() - self.done_count() - self.tally().3 - self.tally().4;
        self.eta = if finished.is_empty() || remaining == 0 {
            None
        } else {
            let avg = finished.iter().sum::<Duration>() / finished.len() as u32;
            Some(avg * remaining as u32 / self.workers.max(1) as u32)
        };
    }
}
