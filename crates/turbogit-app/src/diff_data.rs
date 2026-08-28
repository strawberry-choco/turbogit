//! Plain-data diff-viewer types owned by the application state, kept beside
//! [`crate::state`] instead of inside the UI modules (DDD split issue 04:
//! invert the state→UI type leak). The UI modules import them back up;
//! nothing here references egui, which is what makes an egui-free
//! application crate possible later.
//!
//! Two groups live here:
//! - the non-text diff pane cache ([`PaneCache`], spec R8): decoded image
//!   bytes and binary sizes keyed by load key. GPU textures are deliberately
//!   NOT part of these types — they are a UI-layer concern, built lazily at
//!   first paint in [`crate::ui::diff`] and dropped with the pane generation.
//! - the hunk-navigation edge-nudge vocabulary ([`Dir`], [`EDGE_WINDOW`],
//!   spec R7): the direction and timing window the pure decision in
//!   [`crate::ui::hunk_nav`] consumes.

use crate::events::{DecodedImage, FetchedBlob};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

// --- non-text pane cache (spec R8) -------------------------------------------

/// One resolved side of a non-text pane: byte length plus the decoded image
/// when the side was decodable within the cap. Constructed off the frame
/// path via [`PaneSide::from_blob`].
pub struct PaneSide {
    pub byte_len: u64,
    pub image: Option<DecodedImage>,
}

impl PaneSide {
    /// Plain-data half of a fetched blob: the byte length and the decoded
    /// image. Any GPU upload happens later, in the UI layer.
    pub fn from_blob(blob: FetchedBlob) -> Self {
        Self {
            byte_len: blob.byte_len,
            image: blob.decoded,
        }
    }
}

/// Both sides of one non-text pane result, keyed by load key.
#[derive(Default)]
pub struct PaneEntry {
    pub old: Option<PaneSide>,
    pub new: Option<PaneSide>,
}

/// Cache bound: a few entries cover back-and-forth file switching without
/// pinning every visited image in memory.
const PANE_CACHE_CAP: usize = 4;

/// Bounded cache of non-text pane results (CONTEXT.md "Root caches"
/// philosophy): invalidated wholesale with root refreshes through
/// [`crate::state::AppState::refresh`], never poked field-by-field; evicts
/// oldest beyond [`PANE_CACHE_CAP`].
#[derive(Default)]
pub struct PaneCache {
    map: HashMap<String, PaneEntry>,
    order: VecDeque<String>,
}

impl PaneCache {
    pub fn get(&self, key: &str) -> Option<&PaneEntry> {
        self.map.get(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// Live load keys, oldest first — lets the UI layer prune its GPU
    /// texture cache to exactly the entries still cached here.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    /// Insert (or replace), evicting the oldest entry past the cap.
    pub fn store(&mut self, key: String, entry: PaneEntry) {
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

// --- hunk-navigation edge-nudge vocabulary (spec R7) --------------------------

/// How long the second edge press may follow the first to cross files.
pub const EDGE_WINDOW: Duration = Duration::from_millis(500);

/// Direction of a hunk-navigation step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// `F7` — next hunk / next file.
    Next,
    /// `Shift+F7` — previous hunk / previous file.
    Prev,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_cache_evicts_oldest_beyond_cap() {
        let mut cache = PaneCache::default();
        for i in 0..6 {
            cache.store(format!("k{i}"), PaneEntry::default());
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
