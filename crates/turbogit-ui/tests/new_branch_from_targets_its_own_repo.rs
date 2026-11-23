//! A new branch created from a row lands in that row's repository, ported
//! from the egui `new_branch_from_targets_its_own_repo` suite.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Project, branches_tab, open_branch_menu, press_menu, shows_part};
use test_support::git_seed::git;

mod support;

fn listed(repo: &Path, branch: &str) -> bool {
    git(repo, &["branch", "--list", branch]).contains(branch)
}

fn wait_listed(repo: &Path, branch: &str) {
    for _ in 0..400 {
        if listed(repo, branch) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{branch} never landed in {}", repo.display());
}

fn two_repos(project: &Project) -> (PathBuf, PathBuf) {
    let alpha = support::branch_repo(project.path(), "alpha", &["shared", "alpha-only"]);
    let beta = support::branch_repo(project.path(), "beta", &["shared", "beta-only"]);
    (alpha, beta)
}

#[test]
fn a_new_branch_from_a_row_lands_in_that_rows_repository() {
    let project = Project::new();
    let (alpha, beta) = two_repos(&project);
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    open_branch_menu(&mut app, "beta", "beta-only");
    press_menu(&mut app, "New branch from");
    app.wait_for("id:dialog:new-branch").unwrap();
    app.clear("id:dialog:new-branch:name").unwrap();
    app.type_text("id:dialog:new-branch:name", "from-beta")
        .unwrap();
    app.press("id:dialog:new-branch:action").unwrap();

    wait_listed(&beta, "from-beta");
    assert!(!listed(&alpha, "from-beta"), "alpha never sees the branch");
}

#[test]
fn a_new_branch_from_a_shared_name_still_lands_in_that_rows_repository() {
    let project = Project::new();
    let (alpha, beta) = two_repos(&project);
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    open_branch_menu(&mut app, "beta", "shared");
    press_menu(&mut app, "New branch from");
    app.wait_for("id:dialog:new-branch").unwrap();
    app.clear("id:dialog:new-branch:name").unwrap();
    app.type_text("id:dialog:new-branch:name", "shared-new")
        .unwrap();
    app.press("id:dialog:new-branch:action").unwrap();

    wait_listed(&beta, "shared-new");
    assert!(!listed(&alpha, "shared-new"));
}

#[test]
fn the_dialog_states_the_rows_repository_and_branch() {
    let project = Project::new();
    let (alpha, beta) = two_repos(&project);
    let mut app = branches_tab(project.path(), &[alpha, beta]);
    open_branch_menu(&mut app, "beta", "shared");
    press_menu(&mut app, "New branch from");
    app.wait_for("id:dialog:new-branch").unwrap();
    app.wait_until(
        "the start point names the row's branch",
        Duration::from_secs(10),
        |screen| screen.texts().contains(&"shared"),
    )
    .unwrap();
    assert!(
        shows_part(&app, "Include beta"),
        "the dialog's scope names the row's repository"
    );
}
