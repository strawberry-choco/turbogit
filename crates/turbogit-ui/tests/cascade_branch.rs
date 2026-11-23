//! A cascade is planned only after the merge that names it, ported from the
//! egui `cascade_branch` suite at the reduced fidelity N4 owns: the dialog
//! predicts the cascade and arms its plan, and cancelling runs nothing.

use std::path::PathBuf;
use std::time::Duration;

use support::{Project, branches_tab, open_branch_menu, press_menu};
use test_support::git_seed::git;

mod support;

fn two_repo_project(project: &Project) -> (PathBuf, PathBuf) {
    let mut repos = Vec::new();
    for name in ["alpha", "beta"] {
        let repo = support::seeded_repo(project.path(), name);
        git(&repo, &["checkout", "-q", "-b", "feature"]);
        support::commit_into(&repo, "feature.txt", "feature work");
        git(&repo, &["checkout", "-q", "main"]);
        repos.push(repo);
    }
    (repos[0].clone(), repos[1].clone())
}

#[test]
fn the_merge_dialog_predicts_the_cascade_and_arms_the_plan_on_request() {
    let project = Project::new();
    let (alpha, beta) = two_repo_project(&project);
    let before_beta = git(&beta, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Merge into main");
    app.wait_for("id:merge:cascade").unwrap();
    app.press("id:merge:view-plan").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        git(&beta, &["rev-parse", "HEAD"]).trim(),
        before_beta,
        "arming the plan merges in no sibling repository"
    );
    assert!(
        !beta.join("feature.txt").exists(),
        "beta is untouched until the plan is confirmed"
    );
}

#[test]
fn cancelling_the_merge_changes_nothing() {
    let project = Project::new();
    let (alpha, beta) = two_repo_project(&project);
    let before_alpha = git(&alpha, &["rev-parse", "HEAD"]).trim().to_string();
    let before_beta = git(&beta, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Merge into main");
    app.wait_for("id:dialog:merge").unwrap();
    app.press("text:Cancel").unwrap();
    app.wait_until("the dialog closes", Duration::from_secs(10), |screen| {
        !screen.matches("id:dialog:merge").unwrap_or(false)
    })
    .unwrap();
    assert_eq!(git(&alpha, &["rev-parse", "HEAD"]).trim(), before_alpha);
    assert_eq!(git(&beta, &["rev-parse", "HEAD"]).trim(), before_beta);
}
