//! Delete targets its own repository, ported from the egui
//! `branch_delete_targets_its_own_repo` suite: two repositories sharing a
//! branch name, and a delete that must reach only the row it was aimed at.

use std::path::Path;
use std::time::Duration;

use nsh_testing::DialogAnswer;
use support::{Project, branches_tab, open_branch_menu, press_menu};
use test_support::git_seed::git;

mod support;

fn listed(repo: &Path, branch: &str) -> bool {
    git(repo, &["branch", "--list", branch]).contains(branch)
}

fn wait_gone(repo: &Path, branch: &str) {
    for _ in 0..400 {
        if !listed(repo, branch) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{branch} was never deleted from {}", repo.display());
}

#[test]
fn a_delete_empties_the_repository_of_its_own_row() {
    let project = Project::new();
    let alpha = support::branch_repo(project.path(), "alpha", &["alpha-only", "shared"]);
    let beta = support::branch_repo(project.path(), "beta", &["beta-only", "shared"]);
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));

    open_branch_menu(&mut app, "beta", "beta-only");
    press_menu(&mut app, "Delete branch");
    wait_gone(&beta, "beta-only");
    assert!(!listed(&alpha, "beta-only"));
    assert!(listed(&alpha, "shared") && listed(&beta, "shared"));
}

#[test]
fn a_delete_does_not_reach_into_the_other_repository_for_a_shared_name() {
    let project = Project::new();
    let alpha = support::branch_repo(project.path(), "alpha", &["shared"]);
    let beta = support::branch_repo(project.path(), "beta", &["shared"]);
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));

    app.press("id:branches:band:alpha:repo").unwrap();
    open_branch_menu(&mut app, "beta", "shared");
    press_menu(&mut app, "Delete branch");
    wait_gone(&beta, "shared");
    assert!(
        listed(&alpha, "shared"),
        "alpha's shared branch must survive a delete aimed at beta's row"
    );
}
