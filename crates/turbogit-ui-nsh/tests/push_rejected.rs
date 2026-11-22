//! The rejection pane, ported from the egui `push_rejected` suite: each
//! class's own sentence, the next step, the update scope, and the retry.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots};
use test_support::git_seed::git;

mod support;

fn ahead_repo(project: &Project, name: &str) -> (PathBuf, PathBuf) {
    let repo = support::seeded_repo(project.path(), name);
    let bare = project.path().join(format!("{name}.git"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["init", "-q", "--bare", "-b", "main", &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    (repo, bare)
}

fn diverged_repo(project: &Project, name: &str) -> (PathBuf, PathBuf) {
    let (repo, bare) = ahead_repo(project, name);
    // The remote moves forward through a second clone.
    let other = project.path().join(format!("{name}-other"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["clone", "-q", &bare_s, other.to_str().unwrap()],
    );
    git(&other, &["config", "user.email", "t@t"]);
    git(&other, &["config", "user.name", "t"]);
    support::commit_into(&other, "remote.txt", "remote work");
    git(&other, &["push", "-q", "origin", "main"]);
    // The local side commits without pulling.
    support::commit_into(&repo, "local.txt", "local work");
    (repo, bare)
}

fn open_push(project: &Project, repo: &Path) -> Mounted {
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    app.first_render().unwrap();
    app.press("id:tab:Branches").unwrap();
    app.wait_for("id:branches:tree").unwrap();
    app.press("id:branches:push").unwrap();
    app.wait_for("id:sync:push-dialog").unwrap();
    app.press("id:sync:row-check-alpha").unwrap();
    app.press("text:Push").unwrap();
    app.wait_for("id:sync:rejection").unwrap();
    app
}

#[test]
fn a_rejected_push_paints_its_sentence_and_the_update_step() {
    let project = Project::new();
    let (repo, _bare) = diverged_repo(&project, "alpha");
    let app = open_push(&project, &repo);
    assert!(
        support::shows_part(&app, "Push to origin was rejected."),
        "the sentence names the remote"
    );
    assert!(
        app.screen().shows("Update and retry"),
        "the next step paints"
    );
    assert!(app.screen().shows("Dismiss"), "the dismissal paints");
}

#[test]
fn update_and_retry_updates_and_pushes_again() {
    let project = Project::new();
    let (repo, bare) = diverged_repo(&project, "alpha");
    let mut app = open_push(&project, &repo);
    app.press("text:Update and retry").unwrap();
    app.wait_until("the rejection clears", Duration::from_secs(10), |screen| {
        !screen.matches("id:sync:rejection").unwrap_or(false)
    })
    .unwrap();
    for _ in 0..600 {
        let local = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        if git(&bare, &["rev-parse", "refs/heads/main"]).trim() == local {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the retry never pushed the reconciled history");
}

#[test]
fn dismissing_keeps_the_branches_as_they_are() {
    let project = Project::new();
    let (repo, bare) = diverged_repo(&project, "alpha");
    let before = git(&bare, &["rev-parse", "refs/heads/main"])
        .trim()
        .to_string();
    let mut app = open_push(&project, &repo);
    app.press("text:Dismiss").unwrap();
    app.wait_until("the pane closes", Duration::from_secs(10), |screen| {
        !screen.matches("id:sync:rejection").unwrap_or(false)
    })
    .unwrap();
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/main"]).trim(),
        before,
        "dismissing writes nothing"
    );
}

#[test]
fn the_update_scope_offers_affected_only_and_every_repository() {
    let project = Project::new();
    let (repo, _bare) = diverged_repo(&project, "alpha");
    let app = open_push(&project, &repo);
    assert!(
        app.screen()
            .matches("id:sync:reject-scope-affected")
            .unwrap_or(false),
        "the affected-only scope paints"
    );
    assert!(
        app.screen()
            .matches("id:sync:reject-scope-all")
            .unwrap_or(false),
        "the all-repositories scope paints"
    );
    assert!(
        app.screen()
            .matches("id:sync:reject-remember")
            .unwrap_or(false),
        "the remember opt-in paints"
    );
}
