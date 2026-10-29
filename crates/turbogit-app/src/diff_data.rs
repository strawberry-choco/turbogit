use crate::events::{DecodedImage, FetchedBlob};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

pub struct PaneSide {
    pub byte_len: u64,
    pub image: Option<DecodedImage>,
}

impl PaneSide {
    pub fn from_blob(blob: FetchedBlob) -> Self {
        Self {
            byte_len: blob.byte_len,
            image: blob.decoded,
        }
    }
}

#[derive(Default)]
pub struct PaneEntry {
    pub old: Option<PaneSide>,
    pub new: Option<PaneSide>,
}

const PANE_CACHE_CAP: usize = 4;

#[derive(Default)]
pub struct PaneCache {
    map: HashMap<String, Arc<PaneEntry>>,
    order: VecDeque<String>,
}

impl PaneCache {
    pub fn get(&self, key: &str) -> Option<Arc<PaneEntry>> {
        self.map.get(key).map(Arc::clone)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    /// Insert (or replace), evicting the oldest entry past the cap.
    pub fn store(&mut self, key: String, entry: Arc<PaneEntry>) {
        if !self.map.contains_key(&key) {
            self.order.push_back(key.clone());
            while self.order.len() > PANE_CACHE_CAP
                && let Some(evicted) = self.order.pop_front()
            {
                self.map.remove(&evicted);
            }
        }
        self.map.insert(key, entry);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

/// How long the second edge press may follow the first to cross files.
pub const EDGE_WINDOW: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Next,
    Prev,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_cache_evicts_oldest_beyond_cap() {
        let mut cache = PaneCache::default();
        for i in 0..6 {
            cache.store(format!("k{i}"), Arc::new(PaneEntry::default()));
        }
        assert_eq!(cache.len(), PANE_CACHE_CAP);
        assert!(cache.get("k0").is_none());
        assert!(cache.get("k1").is_none());
        for i in 2..6 {
            assert!(cache.get(&format!("k{i}")).is_some());
        }
        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}
