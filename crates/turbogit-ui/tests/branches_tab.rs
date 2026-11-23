//! The Branches tab, ported from the egui `branches_tab` suite: the groups
//! and counts, selection without checkout, the live filter and its dead end,
//! delete and undo, rename, the dirty-tree preflight, and remote reveal.

use std::path::Path;
use std::time::Duration;

use nsh_testing::DialogAnswer;
use support::{Project, branch_repo, branches_tab, open_branch_menu, press_menu};
use test_support::git_seed::git;

mod support;

fn listed(dir: &Path, branch: &str) -> bool {
    git(dir, &["branch", "--list", branch]).contains(branch)
}

fn wait_listed(dir: &Path, branch: &str, want: bool) {
    for _ in 0..400 {
        if listed(dir, branch) == want {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "{branch}: wanted listed={want}, git says {:?}",
        git(dir, &["branch", "--list", branch])
    );
}

#[test]
fn the_tab_paints_the_groups_and_their_counts() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a", "feature-z"]);
    let app = branches_tab(project.path(), std::slice::from_ref(&repo));
    assert!(support::shows_part(&app, "LOCAL"), "the local band paints");
    assert!(
        support::shows_part(&app, "REMOTE"),
        "the remote rollup paints"
    );
    assert!(
        support::shows_part(&app, "TAGS"),
        "the collapsed tags band paints"
    );
    assert!(
        support::shows_part(&app, "LOCAL (3)"),
        "the local band counts every leaf"
    );
    assert!(
        app.screen().shows("main") && app.screen().shows("feature-a"),
        "the rows paint"
    );
}

#[test]
fn a_rows_press_selects_and_never_checks_out() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));

    app.press("id:branches:row:alpha:feature-a")
        .expect("the branch row");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        git(&repo, &["branch", "--show-current"]).trim(),
        "main",
        "a single press selects, it never moves HEAD"
    );
}

#[test]
fn typing_filters_the_rows_live_and_a_dead_end_offers_the_typed_name() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a", "hotfix"]);
    let mut app = branches_tab(project.path(), &[repo]);

    app.type_text("id:branches:search", "feature").unwrap();
    app.wait_until("the list narrows", Duration::from_secs(10), |screen| {
        screen
            .matches("id:branches:row:alpha:feature-a")
            .unwrap_or(false)
            && !screen
                .matches("id:branches:row:alpha:hotfix")
                .unwrap_or(false)
    })
    .unwrap();

    app.clear("id:branches:search").unwrap();
    app.type_text("id:branches:search", "nope-nothing").unwrap();
    app.wait_for("id:branches:no-match").unwrap();
    assert!(
        support::shows_part(&app, "no branch called"),
        "the dead end names the query"
    );
    app.press("id:branches:create-query").unwrap();
    app.wait_for("id:dialog:new-branch").unwrap();
}

#[test]
fn delete_asks_and_the_undo_strip_restores_the_branch() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));

    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "Delete branch");
    wait_listed(&repo, "feature-a", false);

    app.wait_for("id:branches:undo").unwrap();
    assert!(
        support::shows_part(&app, "Deleted 'feature-a'"),
        "the strip names the branch"
    );
    app.press("id:branches:undo-action").unwrap();
    wait_listed(&repo, "feature-a", true);
}

#[test]
fn rename_goes_through_the_dialog_and_the_tree_follows() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));

    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "Rename branch");
    app.wait_for("id:dialog:rename-branch").unwrap();
    app.type_text("id:dialog:rename-branch:name", "renamed")
        .unwrap();
    for i in 0..10 {
        std::thread::sleep(Duration::from_millis(200));
        eprintln!(
            "tick {i}: input={:?} enabled={:?}",
            app.screen().input_text("id:dialog:rename-branch:name"),
            app.screen().enabled("id:dialog:rename-branch:action")
        );
    }
    app.wait_for_enabled("id:dialog:rename-branch:action")
        .unwrap();
    app.press("id:dialog:rename-branch:action").unwrap();

    wait_listed(&repo, "renamed", true);
    wait_listed(&repo, "feature-a", false);
}

#[test]
fn a_dirty_tree_checkout_asks_before_it_moves() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    std::fs::write(repo.join("README.md"), "dirty\n").unwrap();
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));

    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "Checkout");
    app.wait_for("id:dialog:checkout-dirty").unwrap();
    assert!(
        support::shows_part(&app, "You have uncommitted changes. Switch to 'feature-a'?"),
        "the prompt names the target"
    );
    assert!(
        app.screen().shows("Bring along") && app.screen().shows("Set aside"),
        "the two plain choices paint"
    );
    assert_eq!(
        git(&repo, &["branch", "--show-current"]).trim(),
        "main",
        "nothing moved before an answer"
    );
}

#[test]
fn remotes_reveal_per_repository_and_group_under_their_remote() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    let bare = project.path().join("origin.git");
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["clone", "--bare", "-q", repo.to_str().unwrap(), &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["push", "-q", "origin", "main:remote-only"]);
    git(&repo, &["fetch", "-q", "origin"]);

    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.wait_until(
        "the rollup counts the remote and its branches",
        Duration::from_secs(10),
        |screen| {
            screen
                .texts()
                .iter()
                .any(|t| t.contains("1 remote · 2 branches"))
        },
    )
    .unwrap();

    app.press("id:branches:band:alpha:remote").unwrap();
    app.wait_for("id:branches:band:alpha:remote-origin")
        .unwrap();
    app.press("id:branches:band:alpha:remote-origin").unwrap();
    app.wait_for("id:branches:row:alpha:remote-only").unwrap();
}

#[test]
fn a_new_branch_is_created_from_the_toolbar_and_the_tree_follows() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &[]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));

    app.press("id:branches:new").unwrap();
    app.wait_for("id:dialog:new-branch").unwrap();
    app.clear("id:dialog:new-branch:name").unwrap();
    app.type_text("id:dialog:new-branch:name", "feature/new")
        .unwrap();
    app.wait_for_enabled("id:dialog:new-branch:action").unwrap();
    app.press("id:dialog:new-branch:action").unwrap();

    wait_listed(&repo, "feature/new", true);
}
