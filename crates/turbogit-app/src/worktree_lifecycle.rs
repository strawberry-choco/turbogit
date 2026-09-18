//! Worktree-list freshness policy and dirty-probe dispatch (ticket 02 + 03,
//! ADR 0019).
//!
//! The app's worktree data has one policy owner: this module admits list
//! fetches (one per root in flight) and per-row dirty probes, stamps the list
//! fetches with the root's mutation epoch, decides at settlement time whether a
//! result may still land, and owns the worker dispatch for both. Root caches
//! retains list storage and the invalidation interface; `AppState` routes each
//! settlement's outcome to the caches and to the last-error surface. Dirty
//! probes are admitted per row (deduped, independent workers) and settle into
//! exactly their own row — a probe for a removed worktree is a harmless no-op
//! because `RootCaches::update_worktree_dirty` only updates an existing row.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::Sender;
use turbogit_domain::model::RootId;
use turbogit_engine_api::GitExecutor;

use crate::events::AppEvent;

/// Coordinate per-root list admission, mutation epochs, and settlement (ticket 02).
#[derive(Default)]
pub struct WorktreeLifecycle {
    /// The root's one in-flight fetch, stamped with the epoch current at
    /// dispatch. The stamp is what makes a slot's owner identifiable: only the
    /// request holding it may release it.
    fetching: HashMap<RootId, u64>,
    /// Stamp fetches so pre-mutation settlements cannot restore stale lists.
    epochs: HashMap<RootId, u64>,
    /// In-flight per-worktree dirty probes keyed by (root, path) (ticket 03):
    /// dedups re-probing the same row every frame while a probe is pending, and
    /// keeps probe demand scoped to its own root so a sibling root's probes
    /// never collide (root isolation).
    probing: HashSet<(RootId, PathBuf)>,
}

impl WorktreeLifecycle {
    /// The root's current mutation epoch — the stamp a dispatch must carry.
    fn epoch_of(&self, root: &RootId) -> u64 {
        self.epochs.get(root).copied().unwrap_or(0)
    }

    /// admit one fetch per root and stamp its mutation epoch (ticket 02).
    pub fn admit(&mut self, root: &RootId) -> Option<u64> {
        if self.fetching.contains_key(root) {
            return None;
        }
        let epoch = self.epoch_of(root);
        self.fetching.insert(root.clone(), epoch);
        Some(epoch)
    }

    /// dispatch an admitted fetch with the app's current executor clone (ticket 02).
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

    /// release the slot this settlement owns and report whether it is current
    /// (ticket 02).
    ///
    /// The two answers are independent. A stale result is still *this* request
    /// finishing, so it must free the slot for the refetch its own staleness
    /// made necessary — otherwise a mutation would block the refetch forever.
    /// But a settlement that no longer owns the slot must not free the live
    /// one: a zombie from before a reset would otherwise hand the slot back,
    /// and the per-frame fetch-on-miss would dispatch a duplicate worker for a
    /// root that already has one, breaking the one-fetch-per-root guarantee.
    pub fn settle(&mut self, root: &RootId, epoch: u64) -> bool {
        if self.fetching.get(root) == Some(&epoch) {
            self.fetching.remove(root);
        }
        epoch == self.epoch_of(root)
    }

    /// Admit one dirty probe per row (ticket 03). Returns `true` when this call
    /// owns the probe — the row was not already being probed — and `false` when
    /// a probe for it is already in flight. Keyed by `(root, path)` so sibling
    /// roots' probes never collide (root isolation).
    pub fn admit_probe(&mut self, root: &RootId, path: &Path) -> bool {
        self.probing.insert((root.clone(), path.to_path_buf()))
    }

    /// Dispatch per-row dirty probes for `candidates`, admitting each
    /// independently so one slow or failing probe never blocks the others
    /// (ticket 03). The executor and event sender are the app's current clones,
    /// so an executor rebuild is picked up by the next probe pass — the same
    /// rebuild lifecycle as the list fetch.
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

    /// Release the in-flight probe slot for a settled row (ticket 03). Returns
    /// whether this module owned that probe. The cache update that follows is a
    /// no-op when the row is gone, so a probe for a removed worktree can never
    /// resurrect a stale row.
    pub fn settle_dirty(&mut self, root: &RootId, path: &Path) -> bool {
        self.probing.remove(&(root.clone(), path.to_path_buf()))
    }

    /// bump the epoch and require targeted RootCaches invalidation (ticket 02).
    #[must_use = "apply the targeted worktree-list invalidation when true"]
    pub fn on_mutation(&mut self, root: &RootId) -> bool {
        *self.epochs.entry(root.clone()).or_default() += 1;
        true
    }

    /// drop per-root policy state alongside all-list invalidation (ticket 02).
    ///
    /// Epochs of roots with a fetch still in flight are bumped, never wiped: a
    /// stamp issued before the reset must stay stale afterwards, otherwise the
    /// pre-reset list could settle over the fresh refetch that follows. Their
    /// slots are released so the refill can be admitted immediately, and the
    /// bump is what keeps those now-untracked zombies from settling *and* from
    /// freeing the admission that replaced them. In-flight dirty probes are
    /// released too (ticket 03): a probe from the previous project settling
    /// afterwards is harmless because the row it targets is already gone.
    pub fn clear(&mut self) {
        let inflight: Vec<RootId> = self.fetching.drain().map(|(root, _)| root).collect();
        for root in inflight {
            *self.epochs.entry(root).or_default() += 1;
        }
        self.probing.clear();
    }
}
