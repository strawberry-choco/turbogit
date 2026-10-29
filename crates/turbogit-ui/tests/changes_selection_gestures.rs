use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::{repo_with_conflict, repo_with_one_commit};
use test_support::harness::{click_with_modifiers, shell_harness_over_unstyled};
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, RootId};

fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

fn change(repo: &Path, rel: &str) -> ChangeRef {
    ChangeRef::new(RootId(repo.to_path_buf().into()), rel)
}

fn add_worktree_file(repo: &Path, name: &str) {
    std::fs::write(repo.join(name), format!("work in {name}\n")).unwrap();
}

fn selected_names(h: &Harness<'_, AppState>) -> Vec<String> {
    let mut out: Vec<String> = h
        .state()
        .ui
        .selected
        .iter()
        .map(|k| k.display().to_string())
        .collect();
    out.sort();
    out
}

#[track_caller]
fn click_row(h: &mut Harness<'_, AppState>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

#[track_caller]
fn command_click(h: &mut Harness<'_, AppState>, label: &str) {
    click_with_modifiers(h, label, egui::Modifiers::COMMAND);
}

#[track_caller]
fn shift_click(h: &mut Harness<'_, AppState>, label: &str) {
    click_with_modifiers(h, label, egui::Modifiers::SHIFT);
}

#[test]
fn a_plain_click_selects_one_row_and_clears_a_held_multi_selection() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "replace-repo");
    for name in ["a.txt", "b.txt", "c.txt"] {
        add_worktree_file(&repo, name);
    }

    let mut h = harness(app_state(std::slice::from_ref(&repo)));
    click_row(&mut h, "a.txt");
    assert_eq!(
        selected_names(&h),
        vec![repo.join("a.txt").display().to_string()],
        "a first plain click selects the row it landed on"
    );

    command_click(&mut h, "b.txt");
    assert_eq!(
        selected_names(&h).len(),
        2,
        "Command/Ctrl adds a second row without dropping the first"
    );

    click_row(&mut h, "c.txt");
    assert_eq!(
        selected_names(&h),
        vec![repo.join("c.txt").display().to_string()],
        "a plain click after a multi-selection leaves exactly the clicked row"
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo, "c.txt")),
        "and previews it"
    );
}

#[test]
fn a_command_click_moves_one_row_into_the_set_and_back_out() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "toggle-repo");
    for name in ["a.txt", "b.txt", "c.txt"] {
        add_worktree_file(&repo, name);
    }

    let mut h = harness(app_state(std::slice::from_ref(&repo)));
    click_row(&mut h, "a.txt");
    command_click(&mut h, "c.txt");
    assert_eq!(
        selected_names(&h),
        vec![
            repo.join("a.txt").display().to_string(),
            repo.join("c.txt").display().to_string(),
        ],
        "both rows are held, and the row nobody clicked stays out"
    );

    command_click(&mut h, "a.txt");
    assert_eq!(
        selected_names(&h),
        vec![repo.join("c.txt").display().to_string()],
        "the same click on a selected row takes only that row out"
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo, "a.txt")),
        "a row taken out of the set is still the row being looked at"
    );
}

#[test]
fn a_shift_click_spans_the_painted_rows_and_reshapes_from_one_anchor() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "range-repo");
    for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        add_worktree_file(&repo, name);
    }

    let mut h = harness(app_state(std::slice::from_ref(&repo)));
    click_row(&mut h, "b.txt");
    shift_click(&mut h, "d.txt");
    assert_eq!(
        selected_names(&h),
        vec![
            repo.join("b.txt").display().to_string(),
            repo.join("c.txt").display().to_string(),
            repo.join("d.txt").display().to_string(),
        ],
        "the range is inclusive at both ends and replaces the set rather than widening it"
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo, "d.txt")),
        "a range click previews the row it landed on"
    );

    shift_click(&mut h, "a.txt");
    assert_eq!(
        selected_names(&h),
        vec![
            repo.join("a.txt").display().to_string(),
            repo.join("b.txt").display().to_string(),
        ],
        "the second shift-click reshapes the same anchor downwards instead of chaining"
    );
}

#[test]
fn a_shift_range_crosses_two_repository_groups() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_with_one_commit(parent.path(), "alpha");
    let lib = repo_with_one_commit(parent.path(), "lib");
    add_worktree_file(&alpha, "a1.txt");
    add_worktree_file(&alpha, "a2.txt");
    add_worktree_file(&lib, "b1.txt");

    let mut h = harness(app_state(&[alpha.clone(), lib.clone()]));
    click_row(&mut h, "Expand all groups");
    click_row(&mut h, "a2.txt");
    shift_click(&mut h, "b1.txt");

    assert_eq!(
        selected_names(&h),
        vec![
            alpha.join("a2.txt").display().to_string(),
            lib.join("b1.txt").display().to_string(),
        ],
        "one range spans both groups' painted rows, from the anchor rather than the list start"
    );
    assert!(
        !h.state().ui.selected.contains(&alpha.join("a1.txt")),
        "a row above the anchor is not part of a range that started at it"
    );
}

#[test]
fn a_shift_range_that_spans_the_conflicts_group_leaves_the_conflicted_path_out() {
    let parent = tempfile::tempdir().unwrap();
    let solo = repo_with_conflict(parent.path(), "solo");
    let other = repo_with_one_commit(parent.path(), "other");
    add_worktree_file(&other, "x.txt");
    add_worktree_file(&other, "y.txt");

    let mut h = harness(app_state(&[solo.clone(), other.clone()]));
    click_row(&mut h, "Expand all groups");

    click_row(&mut h, "C conf.txt");
    assert!(
        h.state().ui.selected.is_empty(),
        "a conflicted row is previewed and never selected: {:?}",
        selected_names(&h)
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&solo, "conf.txt")),
        "clicking it is still a browse"
    );

    shift_click(&mut h, "y.txt");
    assert_eq!(
        selected_names(&h),
        vec![
            other.join("x.txt").display().to_string(),
            other.join("y.txt").display().to_string(),
        ],
        "the files the range passes over are selected and the unmerged path between them is not"
    );
    assert!(
        !h.state().ui.selected.contains(&solo.join("conf.txt")),
        "the guard against Discard and Shelve being handed an unmerged path"
    );
}
