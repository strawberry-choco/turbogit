//! Multi-selection over the Local Changes file list, as a reducer over one
//! row-body click.
//!
//! Pure presentation logic (the `tree_selection` precedent): the selection is
//! plain data, a `HashSet<PathBuf>` of canonical root-scoped keys held in
//! `UiState`, the range anchor is an `Option<PathBuf>` beside it, and every rule
//! here takes that data plus the rows in paint order. The module owns no state
//! and names no egui type, so the caller reads the held modifier keys at the
//! click site and hands the two it cares about to [`click_kind`].
//!
//! A [`RowSlot`] is one row **in the order it was painted**. Registering slots
//! during rendering rather than deriving the order from the status snapshot is
//! what makes a range span exactly the rows the developer can see: the inline
//! file filter drops rows, and a collapsed repository group paints none, so any
//! second derivation would be a second truth a drift could hide in.
//!
//! `UiState::selected` is the input to Discard Changes, Shelve and Create Patch.
//! None of those three guards an unmerged path, they filter on set membership
//! alone, so a conflicted key inside it is handed straight to `git checkout --
//! <paths>` or into a whole-tree `git stash push` that git refuses. That is the
//! whole reason [`RowSlot::selectable`] exists: a Merge-conflicts row keeps its
//! position in a range, so a drag that passes over the conflicts group still
//! spans it, and it is never admitted to the set. The row still previews.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Which gesture a row-body click is, decided by the modifier keys held at the
/// click. Shift outranks command and command outranks a plain click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickKind {
    /// Select this row alone; everything else leaves the set.
    Replace,
    /// Move this row between the set and outside it, touching nothing else.
    Toggle,
    /// Replace the set with the painted range from the anchor to this row.
    Range,
}

/// One visible row of the file list, in paint order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowSlot {
    /// The row's canonical key: `<root path>/<repo-relative path>`.
    pub key: PathBuf,
    /// Whether a click may put this row in the set. `false` is the
    /// Merge-conflicts row, which holds a range position and stays out.
    pub selectable: bool,
}

/// Map the two modifier keys a row-body click was taken with onto a gesture.
pub fn click_kind(shift: bool, command: bool) -> ClickKind {
    if shift {
        ClickKind::Range
    } else if command {
        ClickKind::Toggle
    } else {
        ClickKind::Replace
    }
}

/// Apply one row-body click to the selection and the anchor.
///
/// The anchor is the shift-range origin. A replace or a toggle moves it to the
/// clicked row; a range leaves it where it is, so repeated shift-clicks reshape
/// one span instead of walking the selection down the list. A range whose anchor
/// no longer resolves to a painted row falls back to a plain replace, which is
/// the answer for a file that vanished, a group that collapsed or a filter that
/// hid it.
pub fn apply(
    selection: &mut HashSet<PathBuf>,
    anchor: &mut Option<PathBuf>,
    order: &[RowSlot],
    clicked: &Path,
    kind: ClickKind,
) {
    let Some(clicked_idx) = position(order, clicked) else {
        *anchor = Some(clicked.to_path_buf());
        return;
    };
    match kind {
        ClickKind::Replace => {
            select_one(selection, clicked, order[clicked_idx].selectable);
            *anchor = Some(clicked.to_path_buf());
        }
        ClickKind::Toggle => {
            if order[clicked_idx].selectable && !selection.remove(clicked) {
                selection.insert(clicked.to_path_buf());
            }
            *anchor = Some(clicked.to_path_buf());
        }
        ClickKind::Range => match anchor.as_ref().and_then(|a| position(order, a)) {
            Some(anchor_idx) => {
                selection.clear();
                let span = anchor_idx.min(clicked_idx)..=anchor_idx.max(clicked_idx);
                for slot in &order[span] {
                    if slot.selectable {
                        selection.insert(slot.key.clone());
                    }
                }
            }
            None => {
                select_one(selection, clicked, order[clicked_idx].selectable);
                *anchor = Some(clicked.to_path_buf());
            }
        },
    }
}

/// The whole set, and nothing but this row. A row that cannot be selected
/// clears the set and adds nothing to it.
fn select_one(selection: &mut HashSet<PathBuf>, clicked: &Path, selectable: bool) {
    selection.clear();
    if selectable {
        selection.insert(clicked.to_path_buf());
    }
}

/// Where a key sits in the painted rows, if it is painted at all.
fn position(order: &[RowSlot], key: &Path) -> Option<usize> {
    order.iter().position(|slot| slot.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paint order `a.txt b.txt C/conf.txt c.txt d.txt`: two files, the
    /// conflicts row, then two more files, so every range test can put an
    /// endpoint on either side of the row that cannot be selected.
    fn order() -> Vec<RowSlot> {
        ["a.txt", "b.txt"]
            .iter()
            .map(|name| RowSlot {
                key: PathBuf::from(name),
                selectable: true,
            })
            .chain([RowSlot {
                key: PathBuf::from("conf.txt"),
                selectable: false,
            }])
            .chain(["c.txt", "d.txt"].iter().map(|name| RowSlot {
                key: PathBuf::from(name),
                selectable: true,
            }))
            .collect()
    }

    fn key(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn selected(names: &[&str]) -> HashSet<PathBuf> {
        names.iter().map(|n| key(n)).collect()
    }

    /// The set as sorted names, so a failure reads as the list rather than as
    /// two hash orders.
    fn names(selection: &HashSet<PathBuf>) -> Vec<String> {
        let mut out: Vec<String> = selection.iter().map(|k| k.display().to_string()).collect();
        out.sort();
        out
    }

    #[track_caller]
    fn run(
        selection: &mut HashSet<PathBuf>,
        anchor: &mut Option<PathBuf>,
        clicked: &str,
        kind: ClickKind,
    ) {
        let order = order();
        apply(selection, anchor, &order, &key(clicked), kind);
    }

    #[test]
    fn click_kind_puts_shift_over_command_and_command_over_plain() {
        assert_eq!(
            click_kind(true, false),
            ClickKind::Range,
            "shift alone is a range"
        );
        assert_eq!(
            click_kind(true, true),
            ClickKind::Range,
            "shift outranks command when both are held"
        );
        assert_eq!(
            click_kind(false, true),
            ClickKind::Toggle,
            "command alone is a toggle"
        );
        assert_eq!(
            click_kind(false, false),
            ClickKind::Replace,
            "nothing held is a replace"
        );
    }

    #[test]
    fn a_plain_click_clears_a_multi_selection_down_to_the_clicked_row() {
        let mut selection = selected(&["a.txt", "b.txt", "c.txt"]);
        let mut anchor = None;

        run(&mut selection, &mut anchor, "b.txt", ClickKind::Replace);
        assert_eq!(
            names(&selection),
            ["b.txt"],
            "a plain click selects exactly the row it landed on"
        );
        assert_eq!(anchor, Some(key("b.txt")), "the click is the anchor");
    }

    #[test]
    fn a_plain_click_on_the_conflicts_row_clears_the_set_and_selects_nothing() {
        let mut selection = selected(&["a.txt", "c.txt"]);
        let mut anchor = None;

        run(&mut selection, &mut anchor, "conf.txt", ClickKind::Replace);
        assert!(
            selection.is_empty(),
            "an unmerged path must never reach the set Discard and Shelve read: {:?}",
            names(&selection)
        );
        assert_eq!(
            anchor,
            Some(key("conf.txt")),
            "the anchor is where the pointer was, even on a row that cannot be selected"
        );
    }

    #[test]
    fn a_toggle_click_adds_one_row_and_then_removes_it_again() {
        let mut selection = selected(&["a.txt"]);
        let mut anchor = Some(key("a.txt"));

        run(&mut selection, &mut anchor, "c.txt", ClickKind::Toggle);
        assert_eq!(
            names(&selection),
            ["a.txt", "c.txt"],
            "a toggle leaves the rows it was not aimed at alone"
        );

        run(&mut selection, &mut anchor, "a.txt", ClickKind::Toggle);
        assert_eq!(
            names(&selection),
            ["c.txt"],
            "the same click on a selected row takes it out"
        );
        assert_eq!(anchor, Some(key("a.txt")), "a toggle moves the anchor");
    }

    #[test]
    fn a_toggle_click_on_the_conflicts_row_never_admits_it() {
        let mut selection = selected(&["a.txt"]);
        let mut anchor = None;

        run(&mut selection, &mut anchor, "conf.txt", ClickKind::Toggle);
        assert_eq!(
            names(&selection),
            ["a.txt"],
            "the conflicts row stays out of the set and takes nothing with it"
        );
    }

    #[test]
    fn a_range_takes_every_row_between_the_anchor_and_the_target_both_ways() {
        let mut selection = HashSet::new();
        let mut anchor = Some(key("b.txt"));

        run(&mut selection, &mut anchor, "d.txt", ClickKind::Range);
        assert_eq!(
            names(&selection),
            ["b.txt", "c.txt", "d.txt"],
            "a forward range includes both endpoints"
        );
        assert_eq!(
            anchor,
            Some(key("b.txt")),
            "a range leaves the anchor alone, so the next shift-click reshapes this span"
        );

        let mut backwards = HashSet::new();
        run(&mut backwards, &mut anchor, "a.txt", ClickKind::Range);
        assert_eq!(
            names(&backwards),
            ["a.txt", "b.txt"],
            "a range aimed above the anchor spans the same rows"
        );
    }

    #[test]
    fn a_range_replaces_the_selection_instead_of_widening_it() {
        let mut selection = selected(&["a.txt", "d.txt"]);
        let mut anchor = Some(key("b.txt"));

        run(&mut selection, &mut anchor, "c.txt", ClickKind::Range);
        assert_eq!(
            names(&selection),
            ["b.txt", "c.txt"],
            "the range is the whole set: rows outside it leave, whether or not they were selected"
        );
    }

    #[test]
    fn a_range_spans_the_conflicts_row_without_selecting_it() {
        let mut selection = HashSet::new();
        let mut anchor = Some(key("conf.txt"));

        run(&mut selection, &mut anchor, "d.txt", ClickKind::Range);
        assert_eq!(
            names(&selection),
            ["c.txt", "d.txt"],
            "the files the range passes over below the unmerged path are selected, it is not, \
             and nothing above it joins them"
        );
    }

    #[test]
    fn a_range_without_an_anchor_is_a_plain_replace() {
        let mut selection = selected(&["a.txt", "c.txt"]);
        let mut anchor = None;

        run(&mut selection, &mut anchor, "d.txt", ClickKind::Range);
        assert_eq!(
            names(&selection),
            ["d.txt"],
            "with no origin there is no span to take, so the click selects one row"
        );
        assert_eq!(anchor, Some(key("d.txt")), "and becomes the anchor");
    }

    #[test]
    fn a_range_whose_anchor_left_the_list_is_a_plain_replace() {
        let mut selection = selected(&["a.txt"]);
        let mut anchor = Some(key("gone.txt"));

        run(&mut selection, &mut anchor, "c.txt", ClickKind::Range);
        assert_eq!(
            names(&selection),
            ["c.txt"],
            "a stale anchor cannot anchor a range"
        );
        assert_eq!(anchor, Some(key("c.txt")), "and is replaced by the click");
    }

    #[test]
    fn a_click_on_a_row_that_is_not_painted_changes_only_the_anchor() {
        let mut selection = selected(&["a.txt", "b.txt"]);
        let mut anchor = Some(key("a.txt"));

        run(&mut selection, &mut anchor, "hidden.txt", ClickKind::Toggle);
        assert_eq!(
            names(&selection),
            ["a.txt", "b.txt"],
            "a row the filter hid is neither added nor removed"
        );
        assert_eq!(anchor, Some(key("hidden.txt")));
    }
}
