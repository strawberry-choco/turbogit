use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::Sender;
use turbogit_domain::model::RootId;
use turbogit_engine_api::GitExecutor;

use crate::events::AppEvent;

#[derive(Default)]
pub struct WorktreeLifecycle {
    fetching: HashMap<RootId, u64>,
    epochs: HashMap<RootId, u64>,
    probing: HashSet<(RootId, PathBuf)>,
}

impl WorktreeLifecycle {
    fn epoch_of(&self, root: &RootId) -> u64 {
        self.epochs.get(root).copied().unwrap_or(0)
    }

    pub fn admit(&mut self, root: &RootId) -> Option<u64> {
        if self.fetching.contains_key(root) {
            return None;
        }
        let epoch = self.epoch_of(root);
        self.fetching.insert(root.clone(), epoch);
        Some(epoch)
    }

    pub fn fetch(&mut self, root: RootId, executor: Arc<dyn GitExecutor>, tx: Sender<AppEvent>) {
        let Some(epoch) = self.admit(&root) else {
            return;
        };
        std::thread::spawn(move || {
            let res = executor.worktree_list(&root.0);
            let _ = tx.send(AppEvent::WorktreesLoaded {
                root,
                worktrees: res,
                epoch,
            });
        });
    }

    pub fn settle(&mut self, root: &RootId, epoch: u64) -> bool {
        if self.fetching.get(root) == Some(&epoch) {
            self.fetching.remove(root);
        }
        epoch == self.epoch_of(root)
    }

    pub fn admit_probe(&mut self, root: &RootId, path: &Path) -> bool {
        self.probing.insert((root.clone(), path.to_path_buf()))
    }

    pub fn ensure_probes(
        &mut self,
        root: RootId,
        candidates: Vec<PathBuf>,
        executor: Arc<dyn GitExecutor>,
        tx: Sender<AppEvent>,
    ) {
        for path in candidates {
            if self.admit_probe(&root, &path) {
                let executor = executor.clone();
                let tx = tx.clone();
                let root = root.clone();
                std::thread::spawn(move || {
                    let dirty = executor.worktree_dirty(&path);
                    let _ = tx.send(AppEvent::WorktreeDirty { root, path, dirty });
                });
            }
        }
    }

    pub fn settle_dirty(&mut self, root: &RootId, path: &Path) -> bool {
        self.probing.remove(&(root.clone(), path.to_path_buf()))
    }

    pub fn on_mutation(&mut self, root: &RootId) {
        *self.epochs.entry(root.clone()).or_default() += 1;
    }

    pub fn clear(&mut self) {
        let inflight: Vec<RootId> = self.fetching.drain().map(|(root, _)| root).collect();
        for root in inflight {
            *self.epochs.entry(root).or_default() += 1;
        }
        self.probing.clear();
    }
}
