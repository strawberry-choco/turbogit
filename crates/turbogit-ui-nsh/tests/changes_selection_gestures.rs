//! The changes-list selection reducer, ported from the egui
//! `changes_selection_gestures` suite. The stopped gesture grammar is pure:
//! a plain click replaces, Command toggles, Shift spans the painted rows from
//! one anchor, an unmerged path is previewed but never selected, and a
//! key that is not on screen keeps the selection and moves the anchor.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use turbogit_ui_nsh::screens::changes::{ClickKind, RowSlot, apply, click_kind};

fn slot(name: &str, selectable: bool) -> RowSlot {
    RowSlot {
        key: PathBuf::from(name),
        selectable,
    }
}

fn order(names: &[&str]) -> Vec<RowSlot> {
    names.iter().map(|name| slot(name, true)).collect()
}

fn selected(selection: &HashSet<PathBuf>) -> Vec<String> {
    let mut out: Vec<String> = selection
        .iter()
        .map(|key| key.display().to_string())
        .collect();
    out.sort();
    out
}

fn click(
    selection: &mut HashSet<PathBuf>,
    anchor: &mut Option<PathBuf>,
    slots: &[RowSlot],
    name: &str,
    kind: ClickKind,
) {
    apply(selection, anchor, slots, Path::new(name), kind);
}

#[test]
fn shift_outranks_command_and_command_alone_toggles() {
    assert_eq!(click_kind(true, true), ClickKind::Range);
    assert_eq!(click_kind(false, true), ClickKind::Toggle);
    assert_eq!(click_kind(false, false), ClickKind::Replace);
}

#[test]
fn a_plain_click_selects_one_row_and_clears_a_held_multi_selection() {
    let slots = order(&["a.txt", "b.txt", "c.txt"]);
    let mut selection = HashSet::new();
    let mut anchor = None;

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "a.txt",
        ClickKind::Replace,
    );
    assert_eq!(selected(&selection), vec!["a.txt"]);
    assert_eq!(anchor.as_deref(), Some(Path::new("a.txt")));

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "b.txt",
        ClickKind::Toggle,
    );
    assert_eq!(selected(&selection), vec!["a.txt", "b.txt"]);

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "c.txt",
        ClickKind::Replace,
    );
    assert_eq!(
        selected(&selection),
        vec!["c.txt"],
        "a plain click after a multi-selection leaves exactly the clicked row"
    );
    assert_eq!(anchor.as_deref(), Some(Path::new("c.txt")));
}

#[test]
fn a_command_click_moves_one_row_into_the_set_and_back_out() {
    let slots = order(&["a.txt", "b.txt", "c.txt"]);
    let mut selection = HashSet::new();
    let mut anchor = None;

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "a.txt",
        ClickKind::Replace,
    );
    click(
        &mut selection,
        &mut anchor,
        &slots,
        "c.txt",
        ClickKind::Toggle,
    );
    assert_eq!(
        selected(&selection),
        vec!["a.txt", "c.txt"],
        "both rows are held, and the row nobody clicked stays out"
    );

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "a.txt",
        ClickKind::Toggle,
    );
    assert_eq!(
        selected(&selection),
        vec!["c.txt"],
        "the same click on a selected row takes only that row out"
    );
    assert_eq!(
        anchor.as_deref(),
        Some(Path::new("a.txt")),
        "a row taken out of the set is still the row being looked at"
    );
}

#[test]
fn a_shift_click_spans_the_painted_rows_and_reshapes_from_one_anchor() {
    let slots = order(&["a.txt", "b.txt", "c.txt", "d.txt"]);
    let mut selection = HashSet::new();
    let mut anchor = None;

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "b.txt",
        ClickKind::Replace,
    );
    click(
        &mut selection,
        &mut anchor,
        &slots,
        "d.txt",
        ClickKind::Range,
    );
    assert_eq!(
        selected(&selection),
        vec!["b.txt", "c.txt", "d.txt"],
        "the range is inclusive at both ends and replaces the set rather than widening it"
    );

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "a.txt",
        ClickKind::Range,
    );
    assert_eq!(
        selected(&selection),
        vec!["a.txt", "b.txt"],
        "the second shift-click reshapes the same anchor downwards instead of chaining"
    );
}

#[test]
fn a_shift_range_crosses_two_repository_groups() {
    let slots = order(&["alpha/a1.txt", "alpha/a2.txt", "lib/b1.txt"]);
    let mut selection = HashSet::new();
    let mut anchor = None;

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "alpha/a2.txt",
        ClickKind::Replace,
    );
    click(
        &mut selection,
        &mut anchor,
        &slots,
        "lib/b1.txt",
        ClickKind::Range,
    );
    assert_eq!(
        selected(&selection),
        vec!["alpha/a2.txt", "lib/b1.txt"],
        "one range spans both groups' painted rows, from the anchor rather than the list start"
    );
    assert!(
        !selection.contains(Path::new("alpha/a1.txt")),
        "a row above the anchor is not part of a range that started at it"
    );
}

#[test]
fn a_shift_range_that_spans_the_conflicts_group_leaves_the_conflicted_path_out() {
    let mut slots = order(&["solo/conf.txt", "other/x.txt", "other/y.txt"]);
    slots[0].selectable = false;
    let mut selection = HashSet::new();
    let mut anchor = None;

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "solo/conf.txt",
        ClickKind::Replace,
    );
    assert!(
        selection.is_empty(),
        "a conflicted row is previewed and never selected"
    );
    assert_eq!(
        anchor.as_deref(),
        Some(Path::new("solo/conf.txt")),
        "clicking it is still a browse, and the anchor follows"
    );

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "solo/conf.txt",
        ClickKind::Range,
    );
    click(
        &mut selection,
        &mut anchor,
        &slots,
        "other/y.txt",
        ClickKind::Range,
    );
    assert_eq!(
        selected(&selection),
        vec!["other/x.txt", "other/y.txt"],
        "the files the range passes over are selected and the unmerged path between them is not"
    );
}

#[test]
fn a_command_toggle_on_a_conflicted_row_never_inserts_it() {
    let mut slots = order(&["a.txt", "conf.txt"]);
    slots[1].selectable = false;
    let mut selection = HashSet::new();
    let mut anchor = None;

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "conf.txt",
        ClickKind::Toggle,
    );
    assert!(
        selection.is_empty(),
        "an unmerged path is the one thing Discard and Shelve must never be handed"
    );
}

#[test]
fn a_key_that_is_not_painted_keeps_the_selection_and_moves_the_anchor() {
    let slots = order(&["a.txt", "b.txt"]);
    let mut selection = HashSet::new();
    let mut anchor = None;
    click(
        &mut selection,
        &mut anchor,
        &slots,
        "a.txt",
        ClickKind::Replace,
    );

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "gone.txt",
        ClickKind::Replace,
    );
    assert_eq!(
        selected(&selection),
        vec!["a.txt"],
        "a row that is no longer on screen cannot clear the set"
    );
    assert_eq!(
        anchor.as_deref(),
        Some(Path::new("gone.txt")),
        "but it becomes the anchor the next range starts from"
    );
}

#[test]
fn a_range_without_a_painted_anchor_falls_back_to_the_clicked_row() {
    let slots = order(&["a.txt", "b.txt", "c.txt"]);
    let mut selection = HashSet::new();
    let mut anchor = Some(PathBuf::from("gone.txt"));

    click(
        &mut selection,
        &mut anchor,
        &slots,
        "b.txt",
        ClickKind::Range,
    );
    assert_eq!(
        selected(&selection),
        vec!["b.txt"],
        "an anchor that left the list cannot span, so the click becomes a plain one"
    );
    assert_eq!(anchor.as_deref(), Some(Path::new("b.txt")));
}
