//! The app's in-flight fetch admission (ticket 02).
//!
//! Every fetch the app dispatches off the render thread shares one rule: one
//! fetch in flight per key, released on that fetch's settlement event — Ok and
//! Err alike, because the worker always posts the event. The owner states that
//! rule once, keyed by fetch kind, so a new kind is one key variant and not a
//! new field plus an insert in one function and a remove in another. This
//! generalises the one-kind admission [`crate::worktree_lifecycle::WorktreeLifecycle`]
//! owns; it neither absorbs nor changes that module.

use std::collections::HashSet;

use turbogit_domain::model::{CommitId, RootId};

use crate::root_caches::LogScope;

/// The key of one in-flight fetch — the fetch kind and the key that kind is
/// keyed by, in one typed value rather than a bare tuple. The variant carries
/// its own key, so two kinds never collide on a shared shape and two keys of
/// one kind stay independent.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum InflightFetch {
    /// A root's log window (`AppState::fetch_log`), one per root.
    Log(RootId),
    /// One scoped listing's next batch (`AppState::load_more_scoped_log`), one
    /// per (root, scope) so two scopes of one root can batch at once.
    ScopedLog(RootId, LogScope),
    /// A root's ref decorations (`AppState::fetch_refs`), one per root.
    Refs(RootId),
    /// One commit's per-file line counts (`AppState::fetch_file_stats`), one per
    /// (root, commit).
    FileStats(RootId, CommitId),
    /// A root's submodule list (`AppState::fetch_submodules`), one per root.
    Submodules(RootId),
    /// A root's stash listing (`AppState::fetch_stashes`), one per root.
    Stashes(RootId),
    /// A root's background incoming-check poll (`AppState::tick_incoming_poll`),
    /// one per root.
    IncomingPoll(RootId),
}

/// The one admission owner for the app's in-flight fetches: the keys that are
/// pending. The whole rule is [`Self::admit`] and [`Self::release`]; the values
/// the fetches fill stay in [`crate::root_caches::RootCaches`].
#[derive(Default)]
pub struct FetchAdmission {
    in_flight: HashSet<InflightFetch>,
}

impl FetchAdmission {
    /// Admit one fetch for `key`, answering whether the slot was free. A second
    /// admit for a key whose fetch is still pending is refused — the no-op that
    /// keeps a per-frame ask to at most one dispatch.
    #[must_use = "a refused admit means this fetch is already in flight; the caller must return"]
    pub fn admit(&mut self, key: InflightFetch) -> bool {
        self.in_flight.insert(key)
    }

    /// Release `key`'s slot. A settlement arm calls this once, *before* it
    /// matches on the outcome, so an Ok and an Err settlement free the slot
    /// alike and a failed fetch can be asked for again.
    pub fn release(&mut self, key: InflightFetch) {
        self.in_flight.remove(&key);
    }
}
