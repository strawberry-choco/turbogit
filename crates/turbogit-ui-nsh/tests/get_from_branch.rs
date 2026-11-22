//! Get from branch, ported from the egui `get_from_branch` suite: the
//! three status rungs, the swap, the single-row take through the confirm,
//! and the detached-head refusal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots};
use test_support::git_seed::{commit, git};

mod support;

fn repo(project: &Project) -> PathBuf {
    let repo = project.path().join("app");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    commit(
        &repo,
        "base.txt",
        "same on both sides\n",
        "the shared commit",
    );
    commit(&repo, "both.txt", "one\n", "both.txt exists");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    commit(
        &repo,
        "only-feature.txt",
        "theirs\n",
        "a file only the branch has",
    );
    commit(&repo, "both.txt", "two\n", "both.txt rewritten");
    git(&repo, &["checkout", "-q", "main"]);
    commit(&repo, "only-main.txt", "mine\n", "a file only main has");
    repo
}

fn mounted(project: &Project, repo: &Path) -> Mounted {
    let roots = vec![repo.to_path_buf()];
    let mut app = for_roots(project.path(), &roots);
    app.first_render().expect("the shell mounts");
    app.press("id:toolbar:n5-get_from_branch")
        .expect("the get band");
    app.wait_for("id:route:get_from_branch")
        .expect("the get route");
    app.press("id:get:open").expect("open the comparison");
    app.wait_for("id:dialog:get-from-branch")
        .expect("the comparison paints");
    app
}

#[test]
fn all_three_rungs_paint_each_on_its_own_row() {
    let project = Project::new();
    let repo = repo(&project);
    let app = mounted(&project, &repo);
    for (path, word) in [
        ("only-feature.txt", "Deleted"),
        ("only-main.txt", "Added"),
        ("both.txt", "Modified"),
    ] {
        assert!(support::shows_part(&app, path), "{path} is a row");
        assert!(support::shows_part(&app, word), "{path} names {word}");
    }
    assert!(
        !support::shows_part(&app, "base.txt"),
        "identical files are not rows"
    );
}

#[test]
fn swapping_the_branches_exchanges_the_two_sides() {
    let project = Project::new();
    let repo = repo(&project);
    let mut app = mounted(&project, &repo);
    assert!(support::shows_part(
        &app,
        "Comparing feature with main (working tree)"
    ));
    app.press("id:get:swap").expect("swap");
    app.wait_for("text:Comparing main with feature (working tree)")
        .expect("the posture flips");
    assert!(support::shows_part(&app, "only-main.txt"));
    assert!(support::shows_part(&app, "only-feature.txt"));
}

#[test]
fn the_verb_is_offered_only_on_the_differing_row() {
    let project = Project::new();
    let repo = repo(&project);
    let app = mounted(&project, &repo);
    assert!(
        app.screen()
            .matches("id:get:take:both.txt")
            .unwrap_or(false),
        "the modified row carries the take"
    );
    assert!(
        !app.screen()
            .matches("id:get:take:only-main.txt")
            .unwrap_or(true),
        "an added row offers no take"
    );
    assert!(
        !app.screen()
            .matches("id:get:take:only-feature.txt")
            .unwrap_or(true),
        "a deleted row offers no take"
    );
}

#[test]
fn a_take_arms_the_confirmation_and_writes_the_file() {
    let project = Project::new();
    let repo = repo(&project);
    let mut app = mounted(&project, &repo);
    app.on_dialog(|_| nsh_testing::DialogAnswer::Press("OK".to_string()));
    app.press("id:get:take:both.txt").expect("take");
    assert!(
        test_support::wait_until(10_000, || {
            std::fs::read_to_string(repo.join("both.txt"))
                .map(|text| text == "two\n")
                .unwrap_or(false)
        }),
        "the branch's contents land in the working tree"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("only-main.txt")).unwrap(),
        "mine\n",
        "one row's verb touches one path"
    );
    app.wait_until("the taken row leaves", Duration::from_secs(5), |screen| {
        !screen.matches("id:get:take:both.txt").unwrap_or(false)
    })
    .expect("the taken path leaves the list");
}

#[test]
fn a_detached_head_opens_nothing() {
    let project = Project::new();
    let repo = repo(&project);
    git(&repo, &["checkout", "-q", "--detach"]);
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().expect("the shell mounts");
    app.press("id:toolbar:n5-get_from_branch")
        .expect("the get band");
    app.wait_for("id:route:get_from_branch").unwrap();
    app.press("id:get:open").expect("open");
    assert!(
        !app.screen()
            .matches("id:dialog:get-from-branch")
            .unwrap_or(false),
        "nothing compares against a detached HEAD"
    );
}
