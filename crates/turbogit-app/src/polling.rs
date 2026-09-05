//! The background incoming check (issue #27, screen 11) as one deep module.
//!
//! Callers pass pure intent — the current [`Instant`] — and the module owns
//! everything else: which roots are due, the busy/worker-pool guard, the
//! per-root in-flight guard, the fetch + ahead/behind computation, and the
//! settlement into the root caches and the activity log. The clock is
//! injected so headless tests drive the schedule deterministically; the
//! frame loop (`src/app.rs` in the composition root) ticks the scheduler
//! every frame and idles on the returned duration.
//!
//! Polling is upstream-only: a root whose current branch has no upstream is
//! never fetched, and findings surface through the existing ahead/behind
//! caches plus an activity entry — the UI is never blocked.

use std::path::Path;
use std::time::Duration;

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{BranchKind, RootId};
use turbogit_engine_api::GitExecutor;

use crate::events::AppEvent;
use crate::state::AppState;

impl AppState {
    /// Drive one scheduler tick (issue #27). Returns how long the frame
    /// loop may idle before the next tick is due — `None` while the
    /// setting is off, a short retry while the worker pool is busy, and
    /// otherwise the remaining time to the next interval boundary.
    pub fn tick_incoming_poll(&mut self, now: std::time::Instant) -> Option<Duration> {
        if !self.settings.incoming_poll {
            self.incoming_poll_last = None;
            return None;
        }
        if self.ui.busy {
            // Respect the worker pool: an operation is in flight; park the
            // poll without advancing the schedule so it fires when idle.
            return Some(Duration::from_secs(1));
        }
        let interval = Duration::from_secs(self.settings.incoming_interval.minutes() * 60);
        if let Some(last) = self.incoming_poll_last {
            let elapsed = now.duration_since(last);
            if elapsed < interval {
                return Some(interval - elapsed);
            }
        }
        self.incoming_poll_last = Some(now);
        self.dispatch_incoming_poll();
        Some(interval)
    }

    /// Dispatch one poll per registered root: synchronously on the headless
    /// harness, one worker thread per root in production. A root with a
    /// poll already in flight is skipped (the cache entry only lands with
    /// the event, mirroring the worktree fetch guard).
    fn dispatch_incoming_poll(&mut self) {
        let roots: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        if self.sync_refresh {
            let executor = self.executor.clone();
            for root in roots {
                let result = poll_root(executor.as_ref(), &root.0);
                self.settle_incoming_poll(root, result);
            }
        } else {
            for root in roots {
                if !self.incoming_poll_inflight.insert(root.clone()) {
                    continue;
                }
                let executor = self.executor.clone();
                let tx = self.tx.clone();
                let path = root.0.clone();
                std::thread::spawn(move || {
                    let result = poll_root(executor.as_ref(), &path);
                    let _ = tx.send(AppEvent::IncomingPolled {
                        root: RootId(path),
                        result,
                    });
                });
            }
        }
    }

    /// Land one finished poll (shared by the synchronous harness path and
    /// the event pump): release the root's in-flight slot, store the fresh
    /// ahead/behind counts, and log an activity entry when the poll found
    /// incoming commits.
    pub(crate) fn settle_incoming_poll(
        &mut self,
        root: RootId,
        result: TgResult<Option<(usize, usize)>>,
    ) {
        self.incoming_poll_inflight.remove(&root);
        match result {
            Ok(Some((ahead, behind))) => {
                self.caches
                    .store_ahead_behind(root.clone(), (ahead, behind));
                if behind > 0 {
                    let repo = root.0.file_name().and_then(|s| s.to_str());
                    let plural = if behind == 1 { "" } else { "s" };
                    self.ui.activity.push(crate::activity::ActivityEntry {
                        at: chrono::Local::now(),
                        repo: repo.map(str::to_string),
                        message: format!("Incoming check · {behind} incoming commit{plural}"),
                        kind: crate::activity::ActivityKind::Success,
                    });
                }
            }
            Ok(None) => {}
            Err(e) => self.last_error = Some(e.to_string()),
        }
    }
}

/// Fetch `root`'s remotes and recompute the current branch's ahead/behind
/// counts against its upstream. `Ok(None)` when there is nothing to check:
/// the current branch has no upstream, so no fetch happens at all.
fn poll_root(exec: &dyn GitExecutor, root: &Path) -> TgResult<Option<(usize, usize)>> {
    let Some((branch, upstream)) = current_branch_upstream(exec, root)? else {
        return Ok(None);
    };
    exec.fetch(root, None)?;
    exec.ahead_behind(root, &branch, &upstream).map(Some)
}

/// The checked-out local branch and its upstream tracking ref, if any.
/// Shared with the refresh path's ahead/behind computation so both agree on
/// what "has an upstream" means.
pub(crate) fn current_branch_upstream(
    exec: &dyn GitExecutor,
    root: &Path,
) -> TgResult<Option<(String, String)>> {
    let branches = exec.branches(root)?;
    let cur = exec.current_branch(root)?;
    Ok(branches
        .iter()
        .find(|b| b.kind == BranchKind::Local && cur.as_deref() == Some(&b.name))
        .and_then(|b| b.tracking.clone().map(|up| (b.name.clone(), up))))
}
