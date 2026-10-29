use std::collections::HashSet;

use turbogit_domain::model::{CommitId, RootId};

use crate::root_caches::LogScope;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum InflightFetch {
    Log(RootId),
    ScopedLog(RootId, LogScope),
    Refs(RootId),
    FileStats(RootId, CommitId),
    Submodules(RootId),
    Stashes(RootId),
    IncomingPoll(RootId),
}

#[derive(Default)]
pub struct FetchAdmission {
    in_flight: HashSet<InflightFetch>,
}

impl FetchAdmission {
    #[must_use = "a refused admit means this fetch is already in flight; the caller must return"]
    pub fn admit(&mut self, key: InflightFetch) -> bool {
        self.in_flight.insert(key)
    }

    pub fn release(&mut self, key: InflightFetch) {
        self.in_flight.remove(&key);
    }
}
