//! The merge dialog, ported from the egui `merge_dialog` suite: the source
//! picker, the strategy and option rows, the preview, and the merge commit
//! the verb makes.

use std::path::PathBuf;
use std::time::Duration;

use support::{Project, branches_tab, open_branch_menu, press_menu, shows_part};
use test_support::git_seed::git;

mod support;

fn merge_repo(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    support::commit_into(&repo, "feature.txt", "feature work");
    git(&repo, &["checkout", "-q", "main"]);
    support::commit_into(&repo, "main.txt", "main work");
    repo
}

fn open_merge(app: &mut support::Mounted, branch: &str) {
    open_branch_menu(app, "alpha", branch);
    press_menu(app, "Merge into main");
    app.wait_for("id:dialog:merge").unwrap();
}

#[test]
fn the_dialog_paints_the_picker_strategy_and_options() {
    let project = Project::new();
    let repo = merge_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_merge(&mut app, "feature");
    for id in [
        "merge:body",
        "merge:source",
        "merge:strategy-No-commit",
        "merge:strategy-Commit",
        "merge:strategy-Squash",
        "merge:strategy-Fast-forward",
    ] {
        assert!(
            app.screen().matches(&format!("id:{id}")).unwrap_or(false),
            "the dialog paints {id}"
        );
    }
    assert!(shows_part(&app, "Merging into main in alpha"));
}

#[test]
fn picking_a_source_shows_the_preview_totals() {
    let project = Project::new();
    let repo = merge_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_merge(&mut app, "feature");
    app.press("text:feature").unwrap();
    app.wait_for("id:merge:preview-commits").unwrap();
    app.wait_for("id:merge:preview-counts").unwrap();
    assert!(
        app.screen().texts().iter().any(|t| t.contains("file")),
        "the preview names the changed files"
    );
}

#[test]
fn the_merge_verb_creates_a_merge_commit() {
    let project = Project::new();
    let repo = merge_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_merge(&mut app, "feature");
    app.press("text:feature").unwrap();
    app.press("id:merge:strategy-Commit").unwrap();
    app.press("id:dialog:merge:action").unwrap();

    for _ in 0..600 {
        if repo.join("feature.txt").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(repo.join("feature.txt").exists(), "the branch landed");
    let parents = git(&repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "a merge commit carries two parents: {parents}"
    );
}

#[test]
fn a_squash_strategy_lands_the_work_without_a_merge_commit() {
    let project = Project::new();
    let repo = merge_repo(&project, "alpha");
    let before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_merge(&mut app, "feature");
    app.press("text:feature").unwrap();
    app.press("id:merge:strategy-Squash").unwrap();
    app.press("id:dialog:merge:action").unwrap();

    for _ in 0..600 {
        if repo.join("feature.txt").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        repo.join("feature.txt").exists(),
        "the branch's file landed"
    );
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        before,
        "a squash merge stages the work and writes no commit"
    );
    assert!(
        git(&repo, &["status", "--porcelain"]).contains("feature.txt"),
        "the squashed work sits staged in the index"
    );
}

#[test]
fn a_cascade_banner_names_the_sibling_repositories() {
    let project = Project::new();
    let alpha = merge_repo(&project, "alpha");
    let beta = merge_repo(&project, "beta");
    let mut app = branches_tab(project.path(), &[alpha, beta]);
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Merge into main");
    app.wait_for("id:dialog:merge").unwrap();
    app.wait_for("id:merge:cascade").unwrap();
    assert!(
        shows_part(&app, "Cascade with 1 other repo after this merge"),
        "the banner counts the sibling"
    );
    app.press("id:merge:view-plan").unwrap();
    assert!(
        app.screen().matches("id:dialog:merge").unwrap_or(false),
        "the dialog stays while the plan is armed"
    );
}
