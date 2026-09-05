//! Issue 10 — cascade run monitor: the bounded worker-pool engine.
//!
//! The engine is pure orchestration over an injected per-root step closure,
//! so these tests need no git at all: plain `RootId`s, a counting step, and
//! the event stream. The git-specific step and the live monitor view are
//! covered at the app and UI seams.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::RootId;
use turbogit_services::bulk_run::{
    RowState, RunControl, RunEvent, RunOutcome, monitor_rows, run_cascade,
};

fn rid(name: &str) -> RootId {
    RootId(PathBuf::from(name).into())
}

#[test]
fn run_cascade_executes_every_root_on_a_bounded_pool_and_reports_each_outcome() {
    let roots: Vec<RootId> = (0..8).map(|i| rid(&format!("r{i}"))).collect();
    let failed = rid("r3");
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));

    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let inflight = in_flight.clone();
    let maxf = max_in_flight.clone();
    let failed_path = failed.0.clone();
    let step = move |root: &RootId| -> TgResult<()> {
        let now = inflight.fetch_add(1, Ordering::SeqCst) + 1;
        maxf.fetch_max(now, Ordering::SeqCst);
        // Give overlap a chance to happen before the bound is observed.
        std::thread::sleep(Duration::from_millis(15));
        inflight.fetch_sub(1, Ordering::SeqCst);
        if root.0 == failed_path {
            Err(TgError::Other("boom".into()))
        } else {
            Ok(())
        }
    };

    let control = RunControl::default();
    let results = run_cascade(&roots, 3, &step, &|e| ev.lock().unwrap().push(e), &control);

    // Every root ran exactly once, in plan order; the failure is reported
    // per root and nothing reports as stopped.
    let names: Vec<String> = results
        .iter()
        .map(|(r, _)| r.0.display().to_string())
        .collect();
    assert_eq!(names, (0..8).map(|i| format!("r{i}")).collect::<Vec<_>>());
    for (root, outcome) in &results {
        match outcome {
            RunOutcome::Done(res) if root.as_path() == failed.as_path() => {
                assert!(res.is_err(), "r3 should fail");
            }
            RunOutcome::Done(res) => assert!(res.is_ok(), "{root:?} should succeed"),
            RunOutcome::Stopped => panic!("nothing stopped without a stop request"),
        }
    }

    // The pool is bounded: never more than the requested worker slots.
    assert!(
        max_in_flight.load(Ordering::SeqCst) <= 3,
        "concurrency exceeded the worker bound"
    );

    // Event stream: exactly one Started then one Finished per root, and the
    // finished events carry per-row durations.
    let ev = events.lock().unwrap();
    let started: HashSet<RootId> = ev
        .iter()
        .filter_map(|e| match e {
            RunEvent::Started { root } => Some(root.clone()),
            _ => None,
        })
        .collect();
    let finished: Vec<&RunEvent> = ev
        .iter()
        .filter(|e| matches!(e, RunEvent::Finished { .. }))
        .collect();
    assert_eq!(started.len(), 8, "one start per root");
    assert_eq!(finished.len(), 8, "one finish per root");
    for e in finished {
        let RunEvent::Finished {
            root,
            result,
            duration,
        } = e
        else {
            unreachable!()
        };
        assert_eq!(result.is_err(), root.as_path() == failed.as_path());
        assert!(*duration > Duration::ZERO, "durations are measured");
    }
}

#[test]
fn monitor_rows_seed_queued_slots_and_skips_in_fleet_order() {
    let fleet = vec![
        (rid("a"), "a".to_string(), None),
        (
            rid("b"),
            "b".to_string(),
            Some("dirty worktree".to_string()),
        ),
        (rid("c"), "c".to_string(), None),
        (rid("d"), "d".to_string(), None),
        (rid("e"), "e".to_string(), None),
        (rid("f"), "f".to_string(), None),
    ];

    let rows = monitor_rows(&fleet, 4);

    // One row per repo, in the caller's (fleet) order.
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c", "d", "e", "f"]);

    // Skipped rows keep their reason; queued rows carry the worker slot
    // they will land on, cycling 1..=workers over the queued sequence
    // ("waiting for worker slot 2/4").
    assert_eq!(
        rows[1].state,
        RowState::Skipped {
            reason: "dirty worktree".to_string()
        }
    );
    assert_eq!(rows[0].state, RowState::Queued { slot: 1 });
    assert_eq!(rows[2].state, RowState::Queued { slot: 2 });
    assert_eq!(rows[3].state, RowState::Queued { slot: 3 });
    assert_eq!(rows[4].state, RowState::Queued { slot: 4 });
    assert_eq!(rows[5].state, RowState::Queued { slot: 1 });

    // Nothing has run yet: no row carries a duration.
    assert!(rows.iter().all(|r| r.duration.is_none()));
}

#[test]
fn stop_remaining_halts_queued_work_without_touching_completed_roots() {
    let roots: Vec<RootId> = (0..6).map(|i| rid(&format!("r{i}"))).collect();
    let control = RunControl::default();
    let stopper = control.clone();
    let ran = Arc::new(Mutex::new(Vec::new()));
    let ran_in_step = ran.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();

    // One worker slot; the first (and only the first) completed step asks
    // for a stop, so every still-queued root must never start.
    let step = move |root: &RootId| -> TgResult<()> {
        ran_in_step.lock().unwrap().push(root.name());
        stopper.stop();
        Ok(())
    };
    let results = run_cascade(&roots, 1, &step, &|e| ev.lock().unwrap().push(e), &control);

    // Exactly the root that ran before the stop reports Done; the queued
    // rest report Stopped, in submission order.
    let states: Vec<bool> = results
        .iter()
        .map(|(_, outcome)| matches!(outcome, RunOutcome::Done(_)))
        .collect();
    assert_eq!(
        states,
        vec![true, false, false, false, false, false],
        "only the completed root survives a stop"
    );

    // The step ran exactly once — completed work is never undone.
    assert_eq!(ran.lock().unwrap().len(), 1);

    // The event stream reports each halted root as Stopped.
    let stopped = events.lock().unwrap();
    let stopped_count = stopped
        .iter()
        .filter(|e| matches!(e, RunEvent::Stopped { .. }))
        .count();
    assert_eq!(stopped_count, 5, "one Stopped event per halted root");
}
