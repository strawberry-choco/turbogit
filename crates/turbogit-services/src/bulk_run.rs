//! Cascade run engine (issue 10, screen 03): a bounded worker pool executing
//! one step per root with live per-row state, replacing the strictly serial
//! bulk fan-out for monitored runs.
//!
//! The engine is pure orchestration: the git-specific step is injected as a
//! closure, so the pool bound, the event stream, and the stop semantics are
//! testable without git. Row-state *rendering* stays in the UI; the app
//! layer turns [`RunEvent`]s into monitor rows.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use turbogit_domain::error::TgResult;
use turbogit_domain::model::RootId;

/// Default worker-slot count for a cascade run (the "2/4" of the queued
/// row's "waiting for worker slot 2/4").
pub const DEFAULT_WORKERS: usize = 4;

/// Shared stop flag for one pool pass. `stop` is idempotent and safe to call
/// from any thread; workers check it between jobs, never mid-job, so
/// completed and running roots are never undone.
#[derive(Clone, Debug, Default)]
pub struct RunControl(Arc<AtomicBool>);

impl RunControl {
    /// Halt the pass: queued jobs will not start.
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Has a stop been requested for this pass?
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The final outcome of one root in a pass.
#[derive(Debug)]
pub enum RunOutcome {
    /// The step ran to completion (success or failure).
    Done(TgResult<()>),
    /// The root was still queued when a stop landed; it never started.
    Stopped,
}

/// Live transitions of one pass, posted from worker threads.
#[derive(Debug)]
pub enum RunEvent {
    /// A worker took the root's job.
    Started { root: RootId },
    /// The step finished, with its wall-clock duration.
    Finished {
        root: RootId,
        result: TgResult<()>,
        duration: std::time::Duration,
    },
    /// A queued root was halted by a stop before starting.
    Stopped { root: RootId },
}

/// Live state of one monitor row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    /// Waiting for a worker slot; `slot` is 1-based ("waiting for worker
    /// slot 2/4").
    Queued { slot: usize },
    /// A worker is executing the step.
    Running,
    /// The step succeeded.
    Done,
    /// Skipped before the run (preflight) or stopped while queued; `reason`
    /// is display-ready.
    Skipped { reason: String },
    /// The step failed; `error` is display-ready.
    Failed { error: String },
}

/// One row of the live monitor (screen 03).
#[derive(Clone, Debug)]
pub struct RunRow {
    pub root: RootId,
    pub name: String,
    pub state: RowState,
    /// Wall-clock duration of the step, once finished.
    pub duration: Option<std::time::Duration>,
}

/// Seed the monitor rows for a run: one row per fleet entry (will-run roots
/// and pre-skipped roots interleaved in the caller's order). Queued rows
/// cycle over `workers` slots; skipped rows carry their reason.
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
                        slot: (slot - 1) % workers + 1,
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

    // Reorder into submission order: one entry per requested root.
    let mut collected = results.into_inner().unwrap();
    collected.sort_by_key(|(root, _)| roots.iter().position(|r| r == root).unwrap_or(usize::MAX));
    collected
}
