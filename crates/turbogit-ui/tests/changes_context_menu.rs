use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::git_seed::git;
use test_support::harness::{
    assert_menu_item_gated, assert_not_painted, assert_painted, click_menu_item,
    click_with_modifiers, right_click_row, settle, shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::state::{AppState, PendingConfirm};
use turbogit_domain::model::{Change, ChangeStatus, RootId};
use turbogit_ui::ui::changes_menu::{self, ChangesMenuAction as A, Rule};
use turbogit_ui::ui::widgets::MenuItemState;

const STAGE: &str = "Stage file";
const UNSTAGE: &str = "Unstage file";
const STAGE_ALL: &str = "Stage all of file";
const DISCARD: &str = "Discard changes…";
const SHELF: &str = "Shelve…";
const PATCH: &str = "Create Patch from Local Changes";
const COPY_PATCH: &str = "Copy as Patch to Clipboard";
const BLAME: &str = "Show blame";
const RESOLVE: &str = "Resolve…";
const OURS: &str = "Take ours";
const THEIRS: &str = "Take theirs";

fn gate(change: &Change, parkable: bool) -> [MenuItemState; 9] {
    changes_menu::changes_menu_items(&changes_menu::ChangesMenuProps {
        change,
        stage_label: STAGE,
        parkable,
    })
}

fn change(status: ChangeStatus) -> Change {
    Change {
        path: PathBuf::from("base.txt"),
        status,
        chunks: vec![],
        staged: false,
        unstaged: true,
        orig_path: None,
    }
}

#[test]
fn the_file_rows_menu_is_the_designed_order() {
    assert_eq!(
        changes_menu::FILE_ORDER,
        [
            (A::StageToggle, Rule::After),
            (A::Discard, Rule::None),
            (A::Shelve, Rule::After),
            (A::CreatePatch, Rule::None),
            (A::CopyPatch, Rule::After),
            (A::ShowBlame, Rule::None),
        ],
        "the staging verb leads alone, the destructive and parking verbs share a group, the \
         patch pair shares one, and the read-only verb closes the menu"
    );
}

#[test]
fn the_conflicted_rows_menu_is_the_designed_order() {
    assert_eq!(
        changes_menu::CONFLICT_ORDER,
        [
            (A::Resolve, Rule::None),
            (A::TakeOurs, Rule::None),
            (A::TakeTheirs, Rule::After),
            (A::Discard, Rule::After),
            (A::ShowBlame, Rule::None),
        ],
        "an unmerged path has no index state to toggle and never joins the selection, so the \
         resolution verbs replace the staging and parking groups rather than adding to them"
    );
}

#[test]
fn a_tracked_row_gates_nothing() {
    let items = gate(&change(ChangeStatus::Modified), true);
    for action in [
        A::StageToggle,
        A::Discard,
        A::Shelve,
        A::CreatePatch,
        A::CopyPatch,
        A::ShowBlame,
    ] {
        assert_eq!(items[action.index()], MenuItemState::enabled());
    }
}

#[test]
fn an_untracked_row_gates_discard_and_blame_and_leaves_the_selection_verbs_alone() {
    let items = gate(&change(ChangeStatus::Unversioned), true);
    assert_eq!(
        items[A::Discard.index()],
        MenuItemState::disabled("an untracked file has no committed version"),
        "the discard is `git checkout -- <paths>`, which has nothing to restore here"
    );
    assert_eq!(
        items[A::ShowBlame.index()],
        MenuItemState::disabled("untracked files have no committed version to blame")
    );
    assert_eq!(items[A::StageToggle.index()], MenuItemState::enabled());
    assert_eq!(items[A::Shelve.index()], MenuItemState::enabled());
}

#[test]
fn a_selection_of_nothing_but_untracked_rows_gates_the_three_verbs_that_park_it() {
    let items = gate(&change(ChangeStatus::Unversioned), false);
    for action in [A::Shelve, A::CreatePatch, A::CopyPatch] {
        assert_eq!(
            items[action.index()],
            MenuItemState::disabled("untracked files have nothing for git to park"),
            "{action:?} reads a diff or a stash, and git keeps neither for a path it never held"
        );
    }
    assert_eq!(
        gate(&change(ChangeStatus::Modified), false)[A::Discard.index()],
        MenuItemState::enabled(),
        "one row's own verb is never gated by the state of the rows beside it"
    );
}

#[test]
fn an_unmerged_row_gates_discard_and_answers_with_the_three_resolution_verbs() {
    let items = gate(&change(ChangeStatus::Conflicted), true);
    assert_eq!(
        items[A::Discard.index()],
        MenuItemState::disabled("resolve the conflict first"),
        "the same reason the selection guard gives for keeping an unmerged path out of the set"
    );
    for action in [A::Resolve, A::TakeOurs, A::TakeTheirs, A::ShowBlame] {
        assert_eq!(items[action.index()], MenuItemState::enabled());
    }
}

fn init_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    path
}

fn make_dirty(repo: &Path) {
    std::fs::write(repo.join("base.txt"), "work\n").unwrap();
    std::fs::write(repo.join("new.txt"), "fresh\n").unwrap();
}

fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

fn changes_harness(roots: &[PathBuf]) -> Harness<'static, AppState> {
    let mut harness = shell_harness_over_unstyled(app_state(roots), egui::vec2(1280.0, 800.0), 512);
    settle(&mut harness);
    assert!(
        wait_until(15_000, || {
            harness.run();
            harness
                .state()
                .multi
                .roots
                .iter()
                .any(|r| !r.status.changes.is_empty())
        }),
        "the working tree the menu answers for is on disk and read"
    );
    harness
}

fn staged_paths(repo: &Path) -> Vec<String> {
    let mut out: Vec<String> = git(repo, &["diff", "--cached", "--name-only"])
        .lines()
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

fn unmerged_paths(repo: &Path) -> Vec<String> {
    git(repo, &["diff", "--name-only", "--diff-filter=U"])
        .lines()
        .map(str::to_owned)
        .collect()
}

fn root_id(state: &AppState, path: &Path) -> RootId {
    state
        .multi
        .roots
        .iter()
        .find(|r| r.id.as_path() == path)
        .unwrap_or_else(|| panic!("{path:?} is a registered root"))
        .id
        .clone()
}

fn selected_keys(state: &AppState) -> Vec<String> {
    let mut out: Vec<String> = state
        .ui
        .selected
        .iter()
        .map(|k| k.display().to_string())
        .collect();
    out.sort();
    out
}

fn holds_selection(state: &AppState, suffix: &str) -> bool {
    selected_keys(state).iter().any(|k| k.ends_with(suffix))
}

/// The open menu's rows, top to bottom. A painted label is not a locator: the
/// commit card paints its own `Shelve…` button and the conflict pane its own
/// `Resolve…`, so this keeps only the items sharing the sentinel's left edge,
/// which is how `click_menu_item` finds a row of this menu.
fn painted_order(
    harness: &Harness<'_, AppState>,
    sentinel: &str,
    labels: &[&'static str],
) -> Vec<&'static str> {
    let column = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(sentinel.to_string()))
        .unwrap_or_else(|| panic!("the menu is open (no {sentinel:?} item)"))
        .rect();
    let mut rows: Vec<(f32, &'static str)> = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter_map(|n| {
            let label = n.accesskit_node().label()?;
            let designed = labels.iter().find(|l| *l == &label)?;
            let off_column = (n.rect().min.x - column.min.x).abs() >= 2.0;
            (!off_column).then_some((n.rect().min.y, *designed))
        })
        .collect();
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    rows.into_iter().map(|(_, label)| label).collect()
}

#[test]
fn right_clicking_a_changed_row_raises_its_menu_in_the_designed_order() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "raise");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    right_click_row(&mut harness, "base.txt");

    for label in [STAGE, DISCARD, SHELF, PATCH, COPY_PATCH, BLAME] {
        assert_painted(&harness, label);
    }
    assert_eq!(
        painted_order(
            &harness,
            STAGE,
            &[STAGE, DISCARD, SHELF, PATCH, COPY_PATCH, BLAME]
        ),
        vec![STAGE, DISCARD, SHELF, PATCH, COPY_PATCH, BLAME],
        "the array is what the developer sees, top to bottom"
    );
    assert!(
        harness.state().ui.changes_menu.is_some(),
        "the surface owns the open row, as the branches and log menus do"
    );
}

#[test]
fn the_resolution_verbs_are_absent_from_an_ordinary_rows_menu() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "ordinary");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    right_click_row(&mut harness, "base.txt");

    for label in [OURS, THEIRS, RESOLVE] {
        assert_not_painted(&harness, label);
    }
}

#[test]
fn the_staging_verb_names_the_state_the_row_is_in() {
    let parent = tempfile::tempdir().unwrap();
    let unstaged = init_repo(parent.path(), "verb-unstaged");
    std::fs::write(unstaged.join("base.txt"), "work\n").unwrap();
    let mut harness = changes_harness(std::slice::from_ref(&unstaged));
    right_click_row(&mut harness, "base.txt");
    assert_painted(&harness, STAGE);
    assert_not_painted(&harness, UNSTAGE);

    let staged = init_repo(parent.path(), "verb-staged");
    std::fs::write(staged.join("base.txt"), "work\n").unwrap();
    git(&staged, &["add", "base.txt"]);
    let mut harness = changes_harness(std::slice::from_ref(&staged));
    right_click_row(&mut harness, "base.txt");
    assert_painted(&harness, UNSTAGE);
    assert_not_painted(&harness, STAGE);
}

#[test]
fn a_half_staged_row_is_asked_about_as_the_remainder_it_is() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "verb-partial");
    std::fs::write(repo.join("base.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.join("base.txt"), "ONE\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "base.txt"]);
    std::fs::write(repo.join("base.txt"), "ONE\nTWO\nthree\n").unwrap();

    let mut harness = changes_harness(std::slice::from_ref(&repo));
    right_click_row(&mut harness, "base.txt");

    assert_painted(&harness, STAGE_ALL);
}

#[test]
fn right_clicking_a_row_already_in_the_selection_keeps_the_other_rows() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "keep");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    harness.get_by_label("base.txt").click();
    harness.run();
    click_with_modifiers(&mut harness, "new.txt", egui::Modifiers::COMMAND);
    assert!(
        holds_selection(harness.state(), "base.txt") && holds_selection(harness.state(), "new.txt"),
        "two rows are selected before the right-click: {:?}",
        selected_keys(harness.state())
    );

    right_click_row(&mut harness, "base.txt");

    assert!(
        holds_selection(harness.state(), "new.txt"),
        "the menu of a selected row must not drop the rows its selection-scoped verbs \
         were about to act on: {:?}",
        selected_keys(harness.state())
    );
}

#[test]
fn right_clicking_a_row_outside_the_selection_replaces_it_and_previews_the_row() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "replace");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    harness.get_by_label("base.txt").click();
    harness.run();

    right_click_row(&mut harness, "new.txt");

    let keys = selected_keys(harness.state());
    assert_eq!(
        keys.len(),
        1,
        "a right-click outside the set selects the row it landed on: {keys:?}"
    );
    assert!(ends_with_new(&keys));
    assert_eq!(
        harness.state().ui.preview_change.as_ref().map(|c| &c.root),
        Some(&root_id(harness.state(), &repo)),
        "the preview follows the row the menu was opened on"
    );
}

fn ends_with_new(keys: &[String]) -> bool {
    keys.iter().any(|k| k.ends_with("new.txt"))
}

#[test]
fn an_untracked_rows_menu_gates_discard_and_blame_rather_than_hiding_them() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "gated");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    right_click_row(&mut harness, "new.txt");

    assert_menu_item_gated(&mut harness, STAGE, DISCARD);
    assert_menu_item_gated(&mut harness, STAGE, BLAME);
    assert_not_painted(&harness, UNSTAGE);
}

#[test]
fn escape_closes_the_row_menu() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "escape");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    right_click_row(&mut harness, "base.txt");
    assert_painted(&harness, DISCARD);

    harness.key_press(egui::Key::Escape);
    harness.step();

    assert_eq!(harness.state().ui.changes_menu, None);
    harness.step();
    assert_not_painted(&harness, DISCARD);
}

#[test]
fn right_clicking_another_row_moves_the_menu_to_the_row_under_the_pointer() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "retarget");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    // The untracked row is below the tracked one, so its menu hangs over the rows
    // beneath it. The second click is aimed at a row the open menu does not cover.
    right_click_row(&mut harness, "new.txt");
    assert_eq!(
        menu_target(&harness).as_deref(),
        Some("new.txt"),
        "one menu at a time, aimed at the row the pointer is on"
    );
    assert_menu_item_gated(&mut harness, STAGE, DISCARD);

    right_click_row(&mut harness, "base.txt");

    assert_eq!(menu_target(&harness).as_deref(), Some("base.txt"));
    assert_menu_item_live(&mut harness, STAGE, DISCARD);
}

fn menu_target(harness: &Harness<'_, AppState>) -> Option<String> {
    harness
        .state()
        .ui
        .changes_menu
        .as_ref()
        .map(|(_, path)| path.display().to_string())
}

/// The twin of `assert_menu_item_gated`, and the only thing that can falsify a
/// gate keyed on the row: a stale menu would keep the blocked item blocked.
#[track_caller]
fn assert_menu_item_live(harness: &mut Harness<'_, AppState>, sentinel: &str, label: &str) {
    let column = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(sentinel.to_string()))
        .unwrap_or_else(|| panic!("the menu is open (no {sentinel:?} item)"))
        .rect();
    let item = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label() == Some(label.to_string())
                && (n.rect().min.x - column.min.x).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("menu item {label} inside the open menu"));
    assert!(
        !item.accesskit_node().is_disabled(),
        "{label} must answer for this row, and be gated for the row it was gated on"
    );
}

#[test]
fn picking_the_staging_verb_stages_the_whole_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "stage");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    right_click_row(&mut harness, "base.txt");
    click_menu_item(&mut harness, DISCARD, STAGE);

    assert!(
        wait_until(15_000, || {
            harness.run();
            staged_paths(&repo).iter().any(|p| p == "base.txt")
        }),
        "the item reaches the same write the checkbox makes; the index holds {:?}",
        staged_paths(&repo)
    );
}

#[test]
fn picking_discard_asks_about_the_repository_the_row_belongs_to() {
    let parent = tempfile::tempdir().unwrap();
    let focused = init_repo(parent.path(), "focused");
    let other = init_repo(parent.path(), "other");
    make_dirty(&focused);
    std::fs::write(other.join("solo.txt"), "solo\n").unwrap();
    git(&other, &["add", "solo.txt"]);
    git(&other, &["commit", "-q", "-m", "solo"]);
    std::fs::write(other.join("solo.txt"), "solo edited\n").unwrap();
    let mut harness = changes_harness(&[focused.clone(), other.clone()]);

    let other_id = root_id(harness.state(), &other);
    assert_eq!(
        harness.state().selected_root,
        Some(root_id(harness.state(), &focused)),
        "the fixture's workspace focus is the first repository"
    );
    harness
        .state_mut()
        .ui
        .changes_expanded
        .insert(other_id.clone());
    harness.run();

    right_click_row(&mut harness, "solo.txt");
    click_menu_item(&mut harness, STAGE, DISCARD);

    match harness.state().ui.confirm.clone() {
        Some(PendingConfirm::Discard { root, changes }) => {
            assert_eq!(
                root, other_id,
                "the confirm answers for the row's repository, not the workspace focus"
            );
            assert_eq!(
                changes.len(),
                1,
                "the confirm holds the right-clicked row and nothing else"
            );
        }
        None => panic!("the item raises the discard question rather than answering it"),
        Some(_) => panic!("a different confirm was raised, not Discard"),
    }
}

#[test]
fn picking_shelve_names_the_repository_the_row_belongs_to() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "shelve-route");
    make_dirty(&repo);
    let mut harness = changes_harness(std::slice::from_ref(&repo));

    right_click_row(&mut harness, "base.txt");
    click_menu_item(&mut harness, DISCARD, SHELF);

    let state = harness.state();
    assert_eq!(
        state.ui.dlg.shelve_root,
        Some(root_id(state, &repo)),
        "the dialog parks the row's repository rather than whatever the focus is when its \
         button is pressed"
    );
}

#[test]
fn a_conflicted_rows_menu_offers_the_resolution_verbs_and_one_of_them_resolves_the_path() {
    let parent = tempfile::tempdir().unwrap();
    let repo = init_repo(parent.path(), "conflict-menu");
    let branch = git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    std::fs::write(repo.join("clash.txt"), "one\n").unwrap();
    git(&repo, &["add", "clash.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("clash.txt"), "side\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "side"]);
    git(&repo, &["checkout", "-q", &branch]);
    std::fs::write(repo.join("clash.txt"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main"]);
    let _ = std::process::Command::new("git")
        .args(["merge", "--no-edit", "side"])
        .current_dir(&repo)
        .output();
    assert_eq!(
        unmerged_paths(&repo),
        vec!["clash.txt".to_owned()],
        "the fixture is an unresolved clash before the menu is opened"
    );

    let mut harness = changes_harness(std::slice::from_ref(&repo));
    right_click_row(&mut harness, "clash.txt");

    assert_eq!(
        painted_order(&harness, OURS, &[RESOLVE, OURS, THEIRS, DISCARD, BLAME]),
        vec![RESOLVE, OURS, THEIRS, DISCARD, BLAME],
        "the conflicted order, and no staging or parking verb"
    );
    assert_menu_item_gated(&mut harness, OURS, DISCARD);
    assert_not_painted(&harness, UNSTAGE);
    assert_not_painted(&harness, PATCH);

    click_menu_item(&mut harness, OURS, OURS);
    assert!(
        wait_until(15_000, || {
            harness.run();
            unmerged_paths(&repo).is_empty()
        }),
        "the row's own resolution verb resolves the row: {:?}",
        unmerged_paths(&repo)
    );
}
