use std::path::Path;
use std::time::Duration;

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{BranchKind, DisplayName, RootId};
use turbogit_engine_api::GitExecutor;

use crate::events::AppEvent;
use crate::state::AppState;

impl AppState {
    pub fn tick_incoming_poll(&mut self, now: std::time::Instant) -> Option<Duration> {
        if !self.settings.incoming_poll {
            self.incoming_poll_last = None;
            return None;
        }
        if self.ui.busy {
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

    fn dispatch_incoming_poll(&mut self) {
        let roots: Vec<RootId> = self.multi.roots.iter().map(|r| r.id.clone()).collect();
        for root in roots {
            if !self
                .fetch_admission
                .admit(crate::fetch_admission::InflightFetch::IncomingPoll(
                    root.clone(),
                ))
            {
                continue;
            }
            let path = root.as_path().to_path_buf();
            self.pump_read(move |executor, tx| {
                let result = poll_root(executor.as_ref(), &path);
                let _ = tx.send(AppEvent::IncomingPolled { root, result });
            });
        }
    }

    pub(crate) fn settle_incoming_poll(
        &mut self,
        root: RootId,
        result: TgResult<Option<(usize, usize)>>,
    ) {
        self.fetch_admission
            .release(crate::fetch_admission::InflightFetch::IncomingPoll(
                root.clone(),
            ));
        match result {
            Ok(Some((ahead, behind))) => {
                self.caches
                    .store_ahead_behind(root.clone(), (ahead, behind));
                if behind > 0 {
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

fn poll_root(exec: &dyn GitExecutor, root: &Path) -> TgResult<Option<(usize, usize)>> {
    let Some((branch, upstream)) = current_branch_upstream(exec, root)? else {
        return Ok(None);
    };
    exec.fetch(root, None)?;
    exec.ahead_behind(root, &branch, &upstream).map(Some)
}

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

    fn nameless_root() -> RootId {
        RootId(std::sync::Arc::from(std::path::PathBuf::from(
            std::path::MAIN_SEPARATOR_STR,
        )))
    }

    #[test]
    fn an_unnameable_root_records_its_incoming_check_as_workspace_wide() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = AppState::for_roots(dir.path(), &[]);
        let root = nameless_root();

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
