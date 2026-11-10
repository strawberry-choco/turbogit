//! `New branch from` is a row action. The repository it writes to is the row's,
//! not any other repository the workspace holds.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_painted, painted_text, settle_quiet, shell_harness_over,
};
use turbogit_app::state::{AppState, Dialog, Tab};
use turbogit_domain::model::BranchKind;

fn repo(project: &Path, name: &str, branches: &[&str]) -> PathBuf {
    let dir = project.join(name);
    git(
        project,
        &[
            "-c",
            "init.defaultBranch=main",
            "init",
            "-q",
            dir.to_str().unwrap(),
        ],
    );
    git(dir.as_path(), &["config", "user.email", "t@t"]);
    git(dir.as_path(), &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "x\n").unwrap();
    git(dir.as_path(), &["add", "."]);
    git(dir.as_path(), &["commit", "-q", "-m", "init"]);
    for b in branches {
        git(dir.as_path(), &["branch", b, "main"]);
    }
    dir
}

fn listed(dir: &Path, branch: &str) -> bool {
    git(dir, &["branch", "--list", branch]).contains(branch)
}

fn loaded() -> (TempDir, PathBuf, Harness<'static, AppState>) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    repo(&project, "alpha", &["shared", "alpha-only"]);
    let beta = repo(&project, "beta", &["shared", "beta-only"]);
    // alpha sits on `main`, beta on `beta-only`, so the dialog's `current` badge
    // can only be right when it reads the row's repo rather than any other.
    git(beta.as_path(), &["checkout", "beta-only"]);

    let state = AppState::new(project.clone());
    let mut harness = shell_harness_over(state, KITTEST_DEFAULT_BOX);
    harness.set_size(egui::vec2(1200.0, 900.0));
    harness.state_mut().ui.tab = Tab::Branches;
    for _ in 0..600 {
        let ready = harness.state().multi.roots.iter().any(|r| {
            r.branches.iter().any(|b| {
                b.name == "beta-only" && b.kind == BranchKind::Local && r.path.ends_with("beta")
            })
        });
        if ready {
            break;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().multi.roots.len(),
        2,
        "both repos must be registered for this to be a multi-root workspace"
    );
    (tmp, project, harness)
}

fn open_new_branch_from(h: &mut Harness<'static, AppState>, row: &str) {
    let visible = h
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| n.accesskit_node().label() == Some(row.to_string()))
        .count();
    assert_eq!(
        visible,
        1,
        "`{row}` must be painted exactly once to be aimed at unambiguously; painted={:?}",
        painted_text(h)
    );
    h.get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(row.to_string()))
        .unwrap()
        .click_secondary();
    h.step();
    h.step();
    h.remove_cursor();
    h.step();
    h.get_all_by_label("New branch from")
        .next()
        .unwrap_or_else(|| panic!("no `New branch from` item; painted={:?}", painted_text(h)))
        .click();
    settle_quiet(h);
    assert_eq!(h.state().ui.dialog, Some(Dialog::NewBranch));
}

fn new_branch_from(h: &mut Harness<'static, AppState>, row: &str, name: &str) {
    open_new_branch_from(h, row);

    // Focusing the name field selects its pre-fill, so the typed name replaces it.
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Branch name")
        .focus();
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Branch name")
        .type_text(name);
    settle_quiet(h);
    h.get_by_label("Create").click();
    for _ in 0..400 {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
        if !h.state().ui.busy {
            break;
        }
    }
    settle_quiet(h);
}

fn fold_repo(h: &mut Harness<'static, AppState>, path: &str) {
    let root = h
        .state()
        .multi
        .roots
        .iter()
        .find(|r| r.path.ends_with(path))
        .unwrap_or_else(|| panic!("no root under {path}"))
        .id
        .clone();
    h.state_mut().ui.branches_tree.collapsed_repos.insert(root);
    h.step();
}

#[test]
fn a_new_branch_from_a_row_lands_in_that_rows_repo() {
    let (_tmp, project, mut harness) = loaded();
    let alpha = project.join("alpha");
    let beta = project.join("beta");

    new_branch_from(&mut harness, "beta-only", "cut-from-beta");

    assert!(
        listed(&beta, "cut-from-beta"),
        "the row's repo has no new branch; painted={:?}",
        painted_text(&harness)
    );
    assert!(
        !listed(&alpha, "cut-from-beta"),
        "the other repo gained a branch aimed at a row in beta"
    );
}

#[test]
fn a_new_branch_from_a_shared_name_still_lands_in_that_rows_repo() {
    let (_tmp, project, mut harness) = loaded();
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    assert!(listed(&alpha, "shared") && listed(&beta, "shared"));

    fold_repo(&mut harness, "alpha");
    new_branch_from(&mut harness, "shared", "cut-from-shared");

    assert!(
        listed(&beta, "cut-from-shared"),
        "the row's repo has no new branch; painted={:?}",
        painted_text(&harness)
    );
    assert!(
        !listed(&alpha, "cut-from-shared"),
        "the other repo gained a branch aimed at this row"
    );
}

#[test]
fn the_dialog_states_the_current_branch_of_the_rows_repo() {
    let (_tmp, _project, mut harness) = loaded();

    open_new_branch_from(&mut harness, "beta-only");

    assert_painted(&harness, "(current)");
}
