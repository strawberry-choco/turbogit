use std::path::PathBuf;
use std::time::Duration;

use egui::{Color32, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, painted_text, settle_quiet, shell_harness_over,
};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::BranchKind;
use turbogit_ui::theme::Palette;

fn disclosure_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = project.join("alpha");
    git(
        &project,
        &["-c", "init.defaultBranch=main", "init", "alpha"],
    );
    git(&alpha, &["config", "user.email", "t@t"]);
    git(&alpha, &["config", "user.name", "t"]);
    std::fs::write(alpha.join("README.md"), "x\n").unwrap();
    git(&alpha, &["add", "."]);
    git(&alpha, &["commit", "-m", "init"]);
    git(&project, &["clone", "--bare", "alpha", "origin.git"]);
    git(&alpha, &["remote", "add", "origin", "../origin.git"]);
    git(&alpha, &["push", "-q", "origin", "main"]);
    git(&alpha, &["branch", "tidy"]);

    git(&alpha, &["checkout", "-q", "-b", "work"]);
    for (file, msg) in [
        ("w1.txt", "work one: first change"),
        ("w2.txt", "work two: second change"),
        ("w3.txt", "work three: third change"),
    ] {
        std::fs::write(alpha.join(file), "x\n").unwrap();
        git(&alpha, &["add", "."]);
        git(&alpha, &["commit", "-m", msg]);
    }
    git(&alpha, &["push", "-q", "-u", "origin", "work"]);
    git(&alpha, &["checkout", "-q", "main"]);
    git(&alpha, &["fetch", "-q", "origin"]);
    (tmp, project)
}

fn shell_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut harness = shell_harness_over(state, KITTEST_DEFAULT_BOX);
    harness.set_size(egui::vec2(1024.0, 768.0));
    harness
}

fn branch<'a>(state: &'a AppState, name: &str) -> Option<&'a turbogit_domain::model::Branch> {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .and_then(|r| {
            r.branches
                .iter()
                .find(|b| b.name == name && b.kind == BranchKind::Local)
        })
}

/// Is `name` still listed as a remote-tracking branch? A `git push --delete`
/// drops the local remote-tracking ref with the remote's, so this is the app's
/// own answer to "did the remote branch go", what the pump below waits on
/// instead of a wall clock. The kind-scoped twin of [`branch_gone`], because the
/// engine lists `origin/work` under the short name `work`.
fn remote_listed(state: &AppState, name: &str) -> bool {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .is_some_and(|r| {
            r.branches
                .iter()
                .any(|b| b.kind == BranchKind::Remote && b.name == name)
        })
}

fn branch_gone(state: &AppState, name: &str) -> bool {
    state
        .selected_root
        .as_ref()
        .and_then(|id| state.multi.by_id(id))
        .is_some_and(|r| {
            !r.branches
                .iter()
                .any(|b| b.name == name && b.kind == BranchKind::Local)
        })
}

fn pump_until(h: &mut Harness<'_, AppState>, what: &str, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(h.state()) {
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for: {what}\n  painted={:?}",
        painted_text(h)
    );
}

fn loaded() -> (TempDir, PathBuf, Harness<'static, AppState>) {
    let (project, dir) = disclosure_project();
    let mut harness = shell_harness(dir.clone());
    harness.state_mut().ui.tab = Tab::Branches;
    pump_until(&mut harness, "both branches listed", |s| {
        branch(s, "work").is_some() && branch(s, "tidy").is_some()
    });
    settle_quiet(&mut harness);
    (project, dir, harness)
}

fn row_node<'t>(harness: &'t Harness<'_, AppState>, name: &str) -> egui_kittest::Node<'t> {
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(name.to_string()))
        .unwrap_or_else(|| {
            panic!(
                "row button for {name}; painted={:?}, labels={:?}",
                painted_text(harness),
                harness
                    .get_all_by_role(egui::accesskit::Role::Button)
                    .filter_map(|n| n.accesskit_node().label())
                    .collect::<Vec<_>>()
            )
        })
}

fn menu_action(h: &mut Harness<'_, AppState>, branch: &str, label: &str) {
    row_node(h, branch).click_secondary();
    h.step();
    h.step();
    h.remove_cursor();
    h.step();
    h.get_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("menu item {label}"))
        .click();
    settle_quiet(h);
}

fn delete(h: &mut Harness<'static, AppState>, branch: &str) {
    menu_action(h, branch, "Delete branch");
    h.get_by_label("OK").click();
    pump_until(h, "the delete notification is up", |s| {
        branch_gone(s, branch) && s.ui.branches_undo.is_some()
    });
}

fn galley_rect(h: &Harness<'_, AppState>, text: &str) -> Rect {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == text)
        .unwrap_or_else(|| panic!("`{text}` was not painted"))
        .rect
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct NamedFill(Color32);

fn fill_behind(h: &Harness<'_, AppState>, point: egui::Pos2) -> Option<NamedFill> {
    filled_rects(h)
        .iter()
        .filter(|(r, _)| r.contains(point))
        .min_by(|(a, _), (b, _)| a.area().total_cmp(&b.area()))
        .map(|(_, fill)| NamedFill(*fill))
}

#[test]
fn an_unmerged_delete_offers_its_commit_count_as_a_link_before_and_after() {
    let (_project, _dir, mut harness) = loaded();

    menu_action(&mut harness, "work", "Delete branch");
    assert_painted(&harness, "Delete local branch 'work'?");
    assert_painted(&harness, "View 3 unmerged commits");

    delete(&mut harness, "work");

    assert_painted(&harness, "Deleted 'work'");
    assert_painted(&harness, "View 3 unmerged commits");
    assert_painted(&harness, "Undo delete");
}

#[test]
fn a_fully_merged_delete_offers_no_unmerged_link() {
    let (_project, _dir, mut harness) = loaded();

    menu_action(&mut harness, "tidy", "Delete branch");
    assert_painted(&harness, "Delete local branch 'tidy'?");
    assert_not_painted(&harness, "unmerged commit");

    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

    delete(&mut harness, "tidy");
    assert_painted(&harness, "Deleted 'tidy'");
    assert_not_painted(&harness, "unmerged commit");
}

#[test]
fn the_link_counts_one_unmerged_commit_in_the_singular() {
    let (_project, dir, mut harness) = loaded();
    let repo = dir.join("alpha");
    git(&repo, &["checkout", "-q", "-b", "edge"]);
    std::fs::write(repo.join("e.txt"), "x\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "edge one"]);
    git(&repo, &["checkout", "-q", "main"]);
    let root = harness.state().selected_root.clone().unwrap();
    harness.state_mut().refresh(Affected::Root(root));
    pump_until(&mut harness, "edge listed", |s| branch(s, "edge").is_some());
    settle_quiet(&mut harness);

    menu_action(&mut harness, "edge", "Delete branch");
    assert_painted(&harness, "View 1 unmerged commit");
    assert_not_painted(&harness, "View 1 unmerged commits");
}

#[test]
fn the_unmerged_viewer_names_the_branch_and_the_reference_it_was_not_merged_into() {
    let (_project, _dir, mut harness) = loaded();
    menu_action(&mut harness, "work", "Delete branch");
    harness.get_by_label("View 3 unmerged commits").click();
    settle_quiet(&mut harness);

    assert_painted(&harness, "Branch Was Not Fully Merged");
    assert_painted(
        &harness,
        "The branch 'work' was not fully merged to main. Below is the list of unmerged commits.",
    );
    assert_painted(&harness, "work one: first change");
    assert_painted(&harness, "work two: second change");
    assert_painted(&harness, "work three: third change");
}

#[test]
fn the_viewer_offers_restore_only_where_a_branch_is_actually_gone() {
    let (_project, _dir, mut harness) = loaded();
    menu_action(&mut harness, "work", "Delete branch");
    harness.get_by_label("View 3 unmerged commits").click();
    settle_quiet(&mut harness);

    assert_painted(&harness, "Branch Was Not Fully Merged");
    assert_not_painted(&harness, "Restore");
    harness.get_by_label("Close").click();
    settle_quiet(&mut harness);
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

    delete(&mut harness, "work");
    harness.get_by_label("View 3 unmerged commits").click();
    settle_quiet(&mut harness);

    assert_painted(&harness, "Branch Was Not Fully Merged");
    assert_painted(&harness, "Restore");
    harness.get_by_label("Restore").click();
    pump_until(&mut harness, "work restored", |s| {
        branch(s, "work").is_some() && s.ui.branches_undo.is_none()
    });
}

#[test]
fn the_unmerged_link_and_the_restore_control_are_not_the_same_kind_of_control() {
    let (_project, _dir, mut harness) = loaded();
    delete(&mut harness, "work");

    let undo = harness.get_by_label("Undo delete").rect();
    let said = galley_rect(&harness, "Deleted 'work'");
    let link = galley_rect(&harness, "View 3 unmerged commits");

    assert!(
        !undo.expand(1.0).contains(link.center()),
        "the link is not inside the restore control's slot: link {link:?}, undo {undo:?}"
    );

    assert_eq!(
        fill_behind(&harness, undo.center()),
        Some(NamedFill(Palette::RAISED)),
        "the restore control keeps the notification's raised fill"
    );

    assert_ne!(
        fill_behind(&harness, link.center()),
        Some(NamedFill(Palette::ACCENT)),
        "the unmerged-commits link must not paint as a primary button"
    );

    let band = said.union(link).union(undo).expand(6.0);
    let accents: Vec<_> = filled_rects(&harness)
        .iter()
        .filter(|(r, c)| *c == Palette::ACCENT && band.contains_rect(*r))
        .cloned()
        .collect();
    assert!(
        accents.is_empty(),
        "the notification carries no accent-filled control, got {accents:?} in {band:?}"
    );
}

#[test]
fn a_deleted_tracking_branch_offers_to_remove_the_remote_it_left_behind() {
    let (_project, dir, mut harness) = loaded();
    let repo = dir.join("alpha");
    assert!(remote_listed(harness.state(), "work"));

    delete(&mut harness, "work");
    assert_painted(&harness, "Remove the tracking remote branch origin/work");

    harness
        .get_by_label("Remove the tracking remote branch origin/work")
        .click();
    settle_quiet(&mut harness);
    assert_painted(&harness, "Delete remote branch 'origin/work'?");
    harness.get_by_label("OK").click();
    pump_until(&mut harness, "the remote delete settles", |s| {
        s.ui.confirm.is_none() && !remote_listed(s, "work")
    });

    let heads = git(&repo, &["ls-remote", "--heads", "origin"]);
    assert!(
        !heads.contains("refs/heads/work"),
        "the remote branch is gone; origin carries {heads}"
    );
}

#[test]
fn an_untracked_delete_offers_no_remote_removal() {
    let (_project, _dir, mut harness) = loaded();
    delete(&mut harness, "tidy");

    assert_painted(&harness, "Deleted 'tidy'");
    assert_not_painted(&harness, "Remove the tracking remote branch");
}

#[test]
fn the_unmerged_link_is_painted_inside_the_notification_strip() {
    let (_project, _dir, mut harness) = loaded();
    delete(&mut harness, "work");

    let said = galley_origin(&harness, "Deleted 'work'")
        .expect("the notification names the branch it deleted");
    let link = galley_origin(&harness, "View 3 unmerged commits").expect("the link is painted");
    let undo = harness.get_by_label("Undo delete").rect();

    assert!(
        (link.y - said.y).abs() < 24.0,
        "the link rides the notification's own row: message at {said:?}, link at {link:?}"
    );
    assert!(
        (link.y - undo.center().y).abs() < 24.0,
        "the link and the restore control share the row: undo {undo:?}, link at {link:?}"
    );
}
