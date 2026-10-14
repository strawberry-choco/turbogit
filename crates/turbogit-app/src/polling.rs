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
use turbogit_domain::model::{BranchKind, DisplayName, RootId};
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

    /// Dispatch one poll per registered root on the dispatch seam, so a tick
    /// never spawns a thread the host did not ask for: a headless test that only
    /// meant to advance time does real git work inline and settles it with the
    /// drain that follows. A root with a poll already in flight is skipped (the
    /// cache entry only lands with the event, mirroring the worktree fetch
    /// guard).
    fn dispatch_incoming_poll(&mut self) {
        let roots: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        for root in roots {
            if !self.incoming_poll_inflight.insert(root.clone()) {
                continue;
            }
            let path = root.as_path().to_path_buf();
            self.pump_read(move |executor, tx| {
                let result = poll_root(executor.as_ref(), &path);
                let _ = tx.send(AppEvent::IncomingPolled { root, result });
            });
        }
    }

    /// Land one finished poll: release the root's in-flight slot, store the
    /// fresh ahead/behind counts, and log an activity entry when the poll found
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
                    // `None` here means "this root's directory name is not
                    // readable", and the feed reads `None` as "every project" —
                    // a preserved mislabel with a ruling still owed, pinned by
                    // this module's own tests.
                    let repo = root.utf8_name();
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
        .and_then(|b| b.tracking.clone().map(|up| (b.name.clone(), up.git_ref()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root whose path has no final component at all — the filesystem root,
    /// spelled with the platform's own separator so the shape is portable. It is
    /// registered nowhere: nothing about settling a poll needs a repository on
    /// disk, and the point of the pin is the *label*, not the fetch.
    fn nameless_root() -> RootId {
        RootId(std::sync::Arc::from(std::path::PathBuf::from(
            std::path::MAIN_SEPARATOR_STR,
        )))
    }

    /// A root the feed cannot name is recorded as an operation against **every
    /// project in the workspace**: the poller's `None` means "this directory
    /// name is not readable", and the activity feed has always read `None` as
    /// "this operation spanned every project". So a background refresh of one
    /// repository lands in a durable, searchable log under the wrong scope, and
    /// stays there.
    ///
    /// This is a characterization pin, not an approval: whether a nameless root
    /// gets a placeholder label, or a distinct "unknown scope" value, is a human
    /// ruling still owed — and it is a change to `ActivityEntry`'s vocabulary,
    /// not to the display-name owner. This assertion is written to **fail** the
    /// moment somebody attributes the entry to the root, because that failure is
    /// where the ruling lands. In the same edit, update this settle arm's
    /// repository derivation and [`crate::settle::activity_repo_label`]'s
    /// contract, which is where `None` is defined to mean the whole workspace.
    #[test]
    fn an_unnameable_root_records_its_incoming_check_as_workspace_wide() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_roots(dir.path(), &[]);
        let root = nameless_root();

        // Three behind, so the entry is recorded at all: a poll that found
        // nothing says nothing, and asserting an absence would prove nothing.
        state.settle_incoming_poll(root, Ok(Some((0, 3))));

        let entry = state
            .ui
            .activity
            .entries
            .last()
            .expect("the poll recorded an entry");
        assert_eq!(
            entry.message, "Incoming check · 3 incoming commits",
            "the entry that lands is the per-repository fetch's own message"
        );
        assert_eq!(
            entry.repo, None,
            "…attributed to no repository at all, which the activity feed reads \
             as the whole workspace: a per-repository background refresh recorded \
             as a workspace-wide one, permanently"
        );
    }
}
