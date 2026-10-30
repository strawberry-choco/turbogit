use std::ops::Range;
pub(crate) use turbogit_app::diff_model::{
    DiffModel, DisplayRow, FileMeta, Gap, Hidden, Highlight, PaneKind, Row, RowKind, pane_kind,
};
use turbogit_app::reveals::{Ask, Edge};

pub(super) fn is_file_row(row: &Row) -> bool {
    row.kind == RowKind::Meta
        && (row.text.starts_with("diff --git ") || row.text.starts_with("diff --cc "))
}

pub(super) const ROW_H: f32 = 22.0;
pub(super) const PANE_HEADER_H: f32 = 28.0;
pub(super) const SIGN_W: f32 = 16.0;
pub(super) const NUM_W: f32 = 40.0;
pub(super) const TEXT_X: f32 = SIGN_W + NUM_W + 12.0;

pub(super) fn mono_font() -> egui::FontId {
    crate::theme::data_font(crate::theme::TYPE_BODY)
}

/// A gap this close to empty collapses to one bar: one reveal step is all it has
/// left, so a single tap finishes it. Derived from the app's own step rather than
/// copied, or the two drift and the collapse fires on a gap that needs two taps.
pub(crate) const COLLAPSED_STEP: u32 = turbogit_app::reveals::FIRST_STEP;

/// Which affordance a gap's bar is: which edge it opens, and which way the block
/// of lines it owns grows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BarKind {
    /// Sits after the hunk above, over the run opened from the gap's top edge.
    /// One tap appends a line under that run.
    Next,
    /// Sits over the run opened from the gap's bottom edge. One tap prepends a
    /// line at the top of that run, so the run keeps reading into the hunk below.
    Previous,
    /// A gap one step from empty has this as its only bar, and it owns every
    /// opened line: the previous bar it replaces gave its lines to this one, so
    /// one ascending run still reads into the hunk below.
    Collapsed,
}

impl BarKind {
    /// The edge a tap on this bar opens.
    pub(crate) fn edge(self) -> Edge {
        match self {
            Self::Next | Self::Collapsed => Edge::Top,
            Self::Previous => Edge::Bottom,
        }
    }
}

/// How much of one context gap is open right now, and what it offers.
///
/// The row plan and every bar above the gap both derive this from the same two
/// counts, so a bar cannot report a number the rows do not paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GapShape {
    /// Offsets opened from the gap's top edge: `[0, top)`.
    top: u32,
    /// Offsets opened from its bottom edge: `[bottom_from, bottom_to)`, held
    /// above `top` so the two runs never name one line twice.
    bottom_from: u32,
    bottom_to: u32,
    /// Lines still hidden, or `None` while a run that ends at the file's end has
    /// no text to size it.
    hidden_left: Option<u32>,
    /// The gap's bars in paint order. An exhausted gap has none, which is how it
    /// lays out no bar row at all.
    bars: [Option<BarKind>; 2],
}

impl GapShape {
    /// The bars this gap offers, in paint order.
    pub(crate) fn bars(self) -> impl Iterator<Item = BarKind> {
        self.bars.into_iter().flatten()
    }

    /// Nothing left to open: no bar, and so no row for a bar.
    pub(crate) fn offers_nothing(self) -> bool {
        self.bars == [None, None]
    }

    /// The offsets one bar's block paints, ascending, as two runs. Only a
    /// collapsed bar fills the second: it owns the run of the bar it replaced.
    pub(crate) fn block(self, kind: BarKind) -> (Range<u32>, Range<u32>) {
        match kind {
            BarKind::Next => (0..self.top, 0..0),
            BarKind::Previous => (self.bottom_from..self.bottom_to, 0..0),
            BarKind::Collapsed => (0..self.top, self.bottom_from..self.bottom_to),
        }
    }

    /// Every opened offset, ascending: what a gap with no bar left still paints.
    pub(crate) fn offsets(self) -> impl Iterator<Item = u32> {
        (0..self.top).chain(self.bottom_from..self.bottom_to)
    }

    pub(crate) fn hidden_left(self) -> Option<u32> {
        self.hidden_left
    }
}

/// The bars a gap offers. One per edge while more than a step is hidden, one
/// collapsed bar once a single step finishes the gap, and one bar only for a run
/// at a file's edge, which has one end to open from however long it is.
fn bars_for(gap: &Gap, hidden_left: Option<u32>) -> [Option<BarKind>; 2] {
    match hidden_left {
        Some(0) => [None, None],
        _ if gap.above.is_none() => [Some(BarKind::Previous), None],
        _ if gap.below.is_none() => [Some(BarKind::Next), None],
        Some(left) if left <= COLLAPSED_STEP => [Some(BarKind::Collapsed), None],
        _ => [Some(BarKind::Next), Some(BarKind::Previous)],
    }
}

/// What one gap looks like under the current reveal counts.
///
/// Counts are intent, and intent cannot paint a line the read has not brought,
/// so a gap whose text has not landed opens nothing even when it has been tapped.
pub(crate) fn gap_shape(gap: &Gap, edges: [u32; 2], side: &Ask) -> GapShape {
    let empty = |hidden_left| GapShape {
        top: 0,
        bottom_from: 0,
        bottom_to: 0,
        hidden_left,
        bars: bars_for(gap, hidden_left),
    };
    let total = match gap.hidden {
        Hidden::Run(n) => n,
        // A run to the end of the file is as long as the file says it is.
        Hidden::ToEof => match side {
            Ask::Have(text) => text.lines().saturating_add(1).saturating_sub(gap.new_first),
            _ => return empty(None),
        },
    };
    if !matches!(side, Ask::Have(_)) {
        return empty(Some(total));
    }
    let top = edges[0].min(total);
    let bottom_from = top.max(total.saturating_sub(edges[1].min(total)));
    let hidden_left = Some(bottom_from - top);
    GapShape {
        top,
        bottom_from,
        bottom_to: total,
        hidden_left,
        bars: bars_for(gap, hidden_left),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use turbogit_app::reveals::SideText;

    fn run(hidden: u32) -> Gap {
        Gap {
            file: 0,
            above: Some(0),
            below: Some(1),
            old_first: 19,
            new_first: 20,
            hidden: Hidden::Run(hidden),
        }
    }

    fn have(text: &str) -> Ask {
        Ask::Have(Arc::new(
            SideText::new(text.as_bytes().to_vec()).expect("utf-8"),
        ))
    }

    /// A side of `n` terminated lines, the way a file that ends in a newline reads.
    fn side(n: u32) -> Ask {
        let text: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
        have(&format!("{}\n", text.join("\n")))
    }

    /// A tail gap: no hunk below, so one bar and nothing else.
    fn tail() -> Gap {
        Gap {
            file: 0,
            above: Some(1),
            below: None,
            old_first: 49,
            new_first: 50,
            hidden: Hidden::ToEof,
        }
    }

    /// A leading gap: no hunk above.
    fn leading(hidden: u32) -> Gap {
        Gap {
            file: 0,
            above: None,
            below: Some(0),
            old_first: 1,
            new_first: 1,
            hidden: Hidden::Run(hidden),
        }
    }

    fn bars(shape: GapShape) -> Vec<BarKind> {
        shape.bars().collect()
    }

    /// The offsets a plan would paint by walking each bar's own block.
    fn through_bars(shape: GapShape) -> Vec<u32> {
        shape
            .bars()
            .flat_map(|kind| {
                let (head, tail) = shape.block(kind);
                head.chain(tail)
            })
            .collect()
    }

    #[test]
    fn a_long_interior_gap_offers_one_bar_per_edge_and_each_owns_its_own_run() {
        let side = have("x\n");
        let shape = gap_shape(&run(23), [4, 4], &side);
        assert_eq!(bars(shape), [BarKind::Next, BarKind::Previous]);
        assert_eq!(
            shape.block(BarKind::Next),
            (0..4, 0..0),
            "the next bar owns the run that follows the hunk above"
        );
        assert_eq!(
            shape.block(BarKind::Previous),
            (19..23, 0..0),
            "the previous bar owns the run that reads into the hunk below"
        );
        assert_eq!(shape.hidden_left(), Some(15));
        assert_eq!(
            through_bars(shape),
            shape.offsets().collect::<Vec<_>>(),
            "every opened line belongs to a bar, and to exactly one"
        );

        let shape = gap_shape(&run(23), [4, 4], &Ask::None);
        assert_eq!(
            through_bars(shape),
            Vec::<u32>::new(),
            "no text, nothing to paint"
        );
        assert_eq!(shape.hidden_left(), Some(23), "the gap is still counted");
        assert_eq!(bars(shape), [BarKind::Next, BarKind::Previous]);
    }

    #[test]
    fn a_gap_one_step_from_empty_collapses_to_one_bar_that_owns_both_runs() {
        let side = have("x\n");
        let shape = gap_shape(&run(23), [10, 10], &side);
        assert_eq!(
            shape.hidden_left(),
            Some(3),
            "three left is within one step of empty"
        );
        assert_eq!(bars(shape), [BarKind::Collapsed]);
        assert_eq!(
            shape.block(BarKind::Collapsed),
            (0..10, 13..23),
            "the suppressed bar's lines joined the surviving one's"
        );
        assert_eq!(through_bars(shape), shape.offsets().collect::<Vec<_>>());

        // One tap on the collapsed bar is an `Edge::Top` tap, and the clamping
        // above already carries the top edge into the bottom run, so the run ends
        // fully open and no bar survives it.
        let shape = gap_shape(&run(23), [23, 10], &side);
        assert_eq!(shape.hidden_left(), Some(0), "one tap emptied the run");
        assert!(
            shape.offers_nothing(),
            "and an exhausted gap lays out no bar"
        );
        assert_eq!(
            shape.offsets().collect::<Vec<_>>(),
            (0..23).collect::<Vec<_>>(),
            "two edges that meet paint the run once"
        );
    }

    #[test]
    fn a_run_at_a_files_edge_keeps_its_one_bar_however_long_it_is() {
        let lead = gap_shape(&leading(40), [0, 10], &have("x\n"));
        assert_eq!(
            bars(lead),
            [BarKind::Previous],
            "a leading run has no hunk above it, so no next bar"
        );
        assert_eq!(
            lead.block(BarKind::Previous),
            (30..40, 0..0),
            "and its one block still reads into the hunk below"
        );

        let tail = gap_shape(&tail(), [10, 0], &side(61));
        assert_eq!(
            bars(tail),
            [BarKind::Next],
            "a trailing run has no hunk below it, so no previous bar"
        );
        assert_eq!(tail.block(BarKind::Next), (0..10, 0..0));
    }

    #[test]
    fn a_tail_sizes_itself_from_the_text_and_reports_unknown_before_that() {
        let shape = gap_shape(&tail(), [0, 0], &Ask::None);
        assert_eq!(shape.hidden_left(), None, "no text, no length");
        assert_eq!(
            bars(shape),
            [BarKind::Next],
            "it cannot yet prove it is not needed"
        );

        let full = side(61);
        let shape = gap_shape(&tail(), [0, 3], &full);
        assert_eq!(
            shape.offsets().collect::<Vec<_>>(),
            vec![9, 10, 11],
            "the last three of the twelve lines between line 50 and the file's end"
        );
        assert_eq!(shape.hidden_left(), Some(9));

        let shape = gap_shape(&tail(), [10, 0], &side(49));
        assert_eq!(
            shape.offsets().collect::<Vec<_>>(),
            Vec::<u32>::new(),
            "the file ended where the last hunk's context did, so the run is empty"
        );
        assert!(
            shape.offers_nothing(),
            "and a gap that hides nothing offers no bar at all"
        );
    }
}
