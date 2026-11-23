//! The rename dialog, ported from the egui `branch_rename_dialog` suite: the
//! upstream option and its consequence, the untracked row's silence, the
//! guarded confirm, and the unset decision.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Project, branches_tab, open_branch_menu, press_menu, shows_part};
use test_support::git_seed::git;

mod support;

fn tracked_repo(project: &Project) -> PathBuf {
    let repo = support::seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", "tracked"]);
    support::commit_into(&repo, "t.txt", "tracked work");
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["branch", "--set-upstream-to=main", "tracked"]);
    repo
}

fn listed(repo: &Path, branch: &str) -> bool {
    git(repo, &["branch", "--list", branch]).contains(branch)
}

fn wait_listed(repo: &Path, branch: &str, want: bool) {
    for _ in 0..400 {
        if listed(repo, branch) == want {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("{branch}: wanted listed={want}");
}

#[test]
fn a_tracked_branch_states_what_the_rename_leaves_behind() {
    let project = Project::new();
    let repo = tracked_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "tracked");
    press_menu(&mut app, "Rename branch");
    app.wait_for("id:dialog:rename-branch").unwrap();
    app.wait_for("id:dialog:rename-branch:unset").unwrap();
    assert!(
        shows_part(
            &app,
            "Only the local branch is renamed — your next push creates and tracks a new remote branch, and main is left behind."
        ),
        "the consequence is stated"
    );
}

#[test]
fn an_untracked_branch_gets_the_name_field_and_no_upstream_promise() {
    let project = Project::new();
    let repo = support::branch_repo(project.path(), "alpha", &["untracked"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "untracked");
    press_menu(&mut app, "Rename branch");
    app.wait_for("id:dialog:rename-branch").unwrap();
    assert!(
        !app.screen()
            .matches("id:dialog:rename-branch:unset")
            .unwrap_or(false),
        "no upstream option without an upstream"
    );
    assert!(
        !shows_part(&app, "Only the local branch is renamed"),
        "no upstream promise without an upstream"
    );
}

#[test]
fn an_empty_or_unchanged_name_renames_nothing() {
    let project = Project::new();
    let repo = support::branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "Rename branch");
    app.wait_for("id:dialog:rename-branch").unwrap();

    app.press("id:dialog:rename-branch:action").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(listed(&repo, "feature-a"), "the branch keeps its name");
    assert!(
        app.screen()
            .matches("id:dialog:rename-branch")
            .unwrap_or(false),
        "the dialog stays open on an empty name"
    );
}

#[test]
fn checking_the_option_unsets_the_upstream_the_rename_would_keep() {
    let project = Project::new();
    let repo = tracked_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "tracked");
    press_menu(&mut app, "Rename branch");
    app.wait_for("id:dialog:rename-branch").unwrap();
    app.press("id:dialog:rename-branch:unset").unwrap();
    app.type_text("id:dialog:rename-branch:name", "renamed")
        .unwrap();
    app.press("id:dialog:rename-branch:action").unwrap();

    wait_listed(&repo, "renamed", true);
    let config = git(&repo, &["config", "--list"]);
    assert!(
        !config
            .lines()
            .any(|line| line.starts_with("branch.renamed.remote=")),
        "the upstream is cleared"
    );
    assert!(
        !config
            .lines()
            .any(|line| line.starts_with("branch.renamed.")),
        "no upstream configuration survives on the renamed branch"
    );
}
