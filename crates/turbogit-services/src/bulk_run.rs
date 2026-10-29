use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use turbogit_domain::error::TgResult;
use turbogit_domain::model::RootId;

pub const DEFAULT_WORKERS: usize = 4;

/// Shared stop flag for one pool pass. `stop` is idempotent and safe to call
/// from any thread; workers check it between jobs, never mid-job, so
/// completed and running roots are never undone.
#[derive(Clone, Debug, Default)]
pub struct RunControl(Arc<AtomicBool>);

impl RunControl {
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub enum RunOutcome {
    Done(TgResult<()>),
    Stopped,
}

#[derive(Debug)]
pub enum RunEvent {
    Started {
        root: RootId,
    },
    Finished {
        root: RootId,
        result: TgResult<()>,
        duration: std::time::Duration,
    },
    Stopped {
        root: RootId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    /// Waiting for a worker slot; `slot` is 1-based.
    Queued {
        slot: usize,
    },
    Running,
    Done,
    Skipped {
        reason: String,
    },
    /// The step failed. `error` is display-ready, and `protected_branch` says
    /// whether that error was a protected-branch guard's refusal.
    Failed {
        error: String,
        protected_branch: bool,
    },
}

#[derive(Clone, Debug)]
pub struct RunRow {
    pub root: RootId,
    pub name: String,
    pub state: RowState,
    pub duration: Option<std::time::Duration>,
}

pub fn worker_slot(index: usize, workers: usize) -> usize {
    index % workers.max(1) + 1
}

pub fn tally(rows: &[RunRow]) -> (usize, usize, usize, usize, usize) {
    let mut t = (0, 0, 0, 0, 0);
    for row in rows {
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

pub fn done_count(rows: &[RunRow]) -> usize {
    tally(rows).0
}

pub fn total(rows: &[RunRow]) -> usize {
    rows.len()
}

/// Recompute the elapsed/ETA snapshot from the finished rows: elapsed runs
/// from `started_at`, ETA is the average finished duration × remaining work /
/// `workers`. `None` while nothing has finished or nothing remains.
pub fn update_progress(
    rows: &[RunRow],
    workers: usize,
    started_at: Instant,
) -> (Duration, Option<Duration>) {
    let elapsed = started_at.elapsed();
    let finished: Vec<Duration> = rows.iter().filter_map(|r| r.duration).collect();
    let remaining = total(rows) - done_count(rows) - tally(rows).3 - tally(rows).4;
    let eta = if finished.is_empty() || remaining == 0 {
        None
    } else {
        let avg = finished.iter().sum::<Duration>() / finished.len() as u32;
        Some(avg * remaining as u32 / workers.max(1) as u32)
    };
    (elapsed, eta)
}

pub fn monitor_rows(fleet: &[(RootId, String, Option<String>)], workers: usize) -> Vec<RunRow> {
    let workers = workers.max(1);
    let mut slot = 0usize;
    fleet
        .iter()
        .map(|(root, name, skip)| {
            let state = match skip {
                Some(reason) => RowState::Skipped {
                    reason: reason.clone(),
                },
                None => {
                    slot += 1;
                    RowState::Queued {
                        slot: worker_slot(slot - 1, workers),
                    }
                }
            };
            RunRow {
                root: root.clone(),
                name: name.clone(),
                state,
                duration: None,
            }
        })
        .collect()
}

/// Execute `step` for every root on a bounded pool of `workers` threads,
/// posting [`RunEvent`]s from the workers and returning one outcome per
/// root, in submission order. Workers pull from a shared queue, so the
/// concurrency never exceeds `workers` (at least one); a stop request
/// halts not-yet-started jobs without touching running or completed ones.
pub fn run_cascade(
    roots: &[RootId],
    workers: usize,
    step: &(dyn Fn(&RootId) -> TgResult<()> + Sync),
    on_event: &(dyn Fn(RunEvent) + Sync),
    control: &RunControl,
) -> Vec<(RootId, RunOutcome)> {
    let workers = workers.max(1).min(roots.len().max(1));
    let queue = Mutex::new(VecDeque::from_iter(roots.iter().cloned()));
    let results: Mutex<Vec<(RootId, RunOutcome)>> = Mutex::new(Vec::with_capacity(roots.len()));

    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some(root) = queue.lock().unwrap().pop_front() else {
                        break;
                    };
                    if control.stopped() {
                        on_event(RunEvent::Stopped { root: root.clone() });
                        results.lock().unwrap().push((root, RunOutcome::Stopped));
                        continue;
                    }
                    on_event(RunEvent::Started { root: root.clone() });
                    let started = Instant::now();
                    let result = step(&root);
                    let duration = started.elapsed();
                    on_event(RunEvent::Finished {
                        root: root.clone(),
                        result: result.clone(),
                        duration,
                    });
                    results
                        .lock()
                        .unwrap()
                        .push((root, RunOutcome::Done(result)));
                }
            });
        }
    });

    let mut collected = results.into_inner().unwrap();
    collected.sort_by_key(|(root, _)| roots.iter().position(|r| r == root).unwrap_or(usize::MAX));
    collected
}
