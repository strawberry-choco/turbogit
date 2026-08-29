//! Pure hunk-navigation decision (spec R7): `F7` / `Shift+F7` step the
//! current hunk through the open diff and, at the edges, cross into the
//! neighboring changed file IntelliJ-style. The whole decision is a pure
//! function over plain inputs so the edge rules are unit-testable headlessly;
//! [`crate::ui::shell`] reads the keys and applies the outcome, and
//! [`crate::ui::diff`] renders the resulting scroll from the cached model.
//!
//! Terminology follows CONTEXT.md "Current hunk": exactly one selection that
//! buttons, hover, and keyboard navigation all aim and the granular verbs
//! consume.

use std::time::{Duration, Instant};

// The plain-data edge-nudge vocabulary ([`Dir`], [`EDGE_WINDOW`]) lives in
// [`turbogit_app::diff_data`] beside the app state — the UI imports it back up
// (DDD split issue 04). Re-exported here so the historical `ui::hunk_nav`
// paths keep resolving.
pub use turbogit_app::diff_data::{Dir, EDGE_WINDOW};

/// What the UI layer should do after one key press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Select this hunk index within the open file.
    Moved(usize),
    /// First press past the edge (or an expired window): show the transient
    /// hint only — nothing moves yet.
    Nudge,
    /// Same-direction edge press inside the window: cross to the adjacent
    /// changed file.
    CrossFile,
}

/// Nearest changed-file index strictly before/after `from` in list order
/// whose entry carries hunks (`count > 0`) — the cross-file target. `None`
/// means no adjacent file in that direction: navigation never wraps.
pub fn adjacent_file_with_hunks(hunk_counts: &[usize], from: usize, dir: Dir) -> Option<usize> {
    match dir {
        Dir::Next => ((from + 1)..hunk_counts.len()).find(|&i| hunk_counts[i] > 0),
        Dir::Prev => (0..from).rev().find(|&i| hunk_counts[i] > 0),
    }
}

/// The whole `F7` / `Shift+F7` decision (spec R7):
///
/// - inside the range: step one hunk, clamped to `[0, total)`;
/// - at the edge, first press: [`Outcome::Nudge`] — nothing moves but the
///   transient hint;
/// - same-direction press within `window` of the armed edge: cross files
///   when an adjacent changed file exists;
/// - expired window, opposite direction, or no adjacent file: nudge again —
///   never wrap.
pub fn advance_hunk(
    current: usize,
    total: usize,
    dir: Dir,
    now: Instant,
    armed_edge: Option<(Dir, Instant)>,
    window: Duration,
    has_adjacent_file: bool,
) -> Outcome {
    if total == 0 {
        return Outcome::Nudge;
    }
    // A stale cursor (a freshly loaded diff may have fewer hunks than the
    // outgoing one had) clamps back inside the range before stepping.
    let current = current.min(total - 1);
    let in_range = match dir {
        Dir::Next => current + 1 < total,
        Dir::Prev => current > 0,
    };
    if in_range {
        return Outcome::Moved(match dir {
            Dir::Next => current + 1,
            Dir::Prev => current - 1,
        });
    }
    match armed_edge {
        Some((armed_dir, at))
            if armed_dir == dir && now.duration_since(at) <= window && has_adjacent_file =>
        {
            Outcome::CrossFile
        }
        _ => Outcome::Nudge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Instant {
        Instant::now()
    }

    #[test]
    fn hunk_nav_moves_within_range() {
        let t = base();
        // Next steps forward one hunk…
        assert_eq!(
            advance_hunk(0, 3, Dir::Next, t, None, EDGE_WINDOW, true),
            Outcome::Moved(1)
        );
        // …and Prev steps back one.
        assert_eq!(
            advance_hunk(2, 3, Dir::Prev, t, None, EDGE_WINDOW, true),
            Outcome::Moved(1)
        );
    }

    #[test]
    fn hunk_nav_clamps_stale_selection_inside_range() {
        let t = base();
        // A stale cursor beyond the range (freshly loaded shorter diff)
        // clamps before stepping: it reads as the last hunk, so Next sits
        // at the edge…
        assert_eq!(
            advance_hunk(5, 3, Dir::Next, t, None, EDGE_WINDOW, true),
            Outcome::Nudge
        );
        // …and Prev steps back from that clamp.
        assert_eq!(
            advance_hunk(5, 3, Dir::Prev, t, None, EDGE_WINDOW, true),
            Outcome::Moved(1)
        );
    }

    #[test]
    fn hunk_nav_first_press_at_edge_nudges_only() {
        let t = base();
        // Next at the last hunk: hint only, selection unchanged.
        assert_eq!(
            advance_hunk(2, 3, Dir::Next, t, None, EDGE_WINDOW, true),
            Outcome::Nudge
        );
        // Prev at the first hunk likewise.
        assert_eq!(
            advance_hunk(0, 3, Dir::Prev, t, None, EDGE_WINDOW, true),
            Outcome::Nudge
        );
    }

    #[test]
    fn hunk_nav_edge_nudge_then_cross_file_within_window() {
        let armed_at = base();
        let now = armed_at.checked_add(EDGE_WINDOW).expect("in range");
        assert_eq!(
            advance_hunk(
                2,
                3,
                Dir::Next,
                now,
                Some((Dir::Next, armed_at)),
                EDGE_WINDOW,
                true
            ),
            Outcome::CrossFile
        );
    }

    #[test]
    fn hunk_nav_expired_edge_window_nudges_again() {
        let armed_at = base();
        // One tick past the window the arm is stale: nudge again.
        let now = armed_at
            .checked_add(EDGE_WINDOW + Duration::from_millis(1))
            .expect("in range");
        assert_eq!(
            advance_hunk(
                2,
                3,
                Dir::Next,
                now,
                Some((Dir::Next, armed_at)),
                EDGE_WINDOW,
                true
            ),
            Outcome::Nudge
        );
    }

    #[test]
    fn hunk_nav_opposite_direction_does_not_arm_cross_file() {
        let armed_at = base();
        let now = armed_at.checked_add(EDGE_WINDOW).expect("in range");
        // Armed with Next but pressed Prev at the first hunk: nudge only.
        assert_eq!(
            advance_hunk(
                0,
                3,
                Dir::Prev,
                now,
                Some((Dir::Next, armed_at)),
                EDGE_WINDOW,
                true
            ),
            Outcome::Nudge
        );
    }

    #[test]
    fn hunk_nav_skips_files_without_hunks() {
        // List order with a binary (hunk-less) file in the middle.
        let counts = [2, 0, 1];
        assert_eq!(adjacent_file_with_hunks(&counts, 0, Dir::Next), Some(2));
        assert_eq!(adjacent_file_with_hunks(&counts, 2, Dir::Prev), Some(0));
        // A hunk-less neighbor in the pressed direction is skipped over.
        assert_eq!(adjacent_file_with_hunks(&[0, 0, 1], 0, Dir::Next), Some(2));
    }

    #[test]
    fn hunk_nav_without_adjacent_file_keeps_nudging() {
        let armed_at = base();
        let now = armed_at.checked_add(EDGE_WINDOW).expect("in range");
        // At the very last file: the second press nudges again, never wraps.
        assert_eq!(
            advance_hunk(
                1,
                2,
                Dir::Next,
                now,
                Some((Dir::Next, armed_at)),
                EDGE_WINDOW,
                false
            ),
            Outcome::Nudge
        );
        // And the adjacency helper agrees there is no target.
        assert_eq!(adjacent_file_with_hunks(&[1], 0, Dir::Next), None);
        assert_eq!(adjacent_file_with_hunks(&[1], 0, Dir::Prev), None);
    }
}
