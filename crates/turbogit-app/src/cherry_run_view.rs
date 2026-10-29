use std::time::{Duration, Instant};

use turbogit_services::bulk_run::{self, RunControl, RunRow};

pub struct CherryRunView {
    pub source_name: String,
    pub commit_count: usize,
    pub stop_on_conflict: bool,
    pub rows: Vec<RunRow>,
    pub workers: usize,
    pub started_at: Instant,
    pub elapsed: Duration,
    pub eta: Option<Duration>,
    pub control: RunControl,
}

impl CherryRunView {
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

    pub fn update_progress(&mut self) {
        let (elapsed, eta) = bulk_run::update_progress(&self.rows, self.workers, self.started_at);
        self.elapsed = elapsed;
        self.eta = eta;
    }
}
