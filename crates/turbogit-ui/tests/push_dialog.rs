//! The Push dialog, ported from the egui `push_dialog` suite: consent, the
//! up-to-date state, the dry-run preview, the force acknowledgment, the
//! tag and hook options, and the guarded push.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Mounted, Project, for_roots};
use test_support::git_seed::git;

mod support;

fn repo_ahead(project: &Project, name: &str) -> (PathBuf, PathBuf) {
    let repo = support::seeded_repo(project.path(), name);
    let bare = project.path().join(format!("{name}.git"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["init", "-q", "--bare", "-b", "main", &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    support::commit_into(&repo, "ahead.txt", "ahead work");
    (repo, bare)
}

fn open_push(project: &Project, repo: &Path) -> Mounted {
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo.to_path_buf()));
    app.first_render().unwrap();
    app.press("id:tab:Branches").unwrap();
    app.wait_for("id:branches:tree").unwrap();
    app.press("id:branches:push").unwrap();
    app.wait_for("id:sync:push-dialog").unwrap();
    app
}

fn bare_tip(bare: &Path, refname: &str) -> String {
    git(bare, &["rev-parse", refname]).trim().to_string()
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn wait_equal(repo_tip: String, bare: &Path, refname: &str, what: &str) {
    for _ in 0..600 {
        if bare_tip(bare, refname) == repo_tip {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "{what}: the remote tip {:?} never matched {repo_tip}",
        bare_tip(bare, refname)
    );
}

#[test]
fn consenting_the_row_pushes_the_branch() {
    let project = Project::new();
    let (repo, bare) = repo_ahead(&project, "alpha");
    let local_tip = head(&repo);
    let mut app = open_push(&project, &repo);
    app.press("id:sync:row-check-alpha").unwrap();
    app.press("text:Push").unwrap();
    wait_equal(local_tip, &bare, "refs/heads/main", "consented push");
}

#[test]
fn an_up_to_date_row_offers_nothing_to_update() {
    let project = Project::new();
    let (repo, bare) = repo_ahead(&project, "alpha");
    git(&repo, &["push", "-q", "origin", "main"]);
    let tip = bare_tip(&bare, "refs/heads/main");
    let mut app = open_push(&project, &repo);
    app.press("id:sync:row-check-alpha").unwrap();
    assert!(
        support::shows_part(&app, "Nothing to update"),
        "an empty plan says so instead of reporting a success"
    );
    assert_eq!(bare_tip(&bare, "refs/heads/main"), tip);
}

#[test]
fn the_preview_paints_its_counts_and_reports() {
    let project = Project::new();
    let (repo, _bare) = repo_ahead(&project, "alpha");
    let mut app = open_push(&project, &repo);
    app.press("id:sync:row-check-alpha").unwrap();
    app.press("text:Preview dry-run").unwrap();
    app.wait_for("id:sync:preview").unwrap();
    app.wait_until("the counts paint", Duration::from_secs(10), |screen| {
        screen.texts().iter().any(|t| t.contains("commits →"))
    })
    .unwrap();
}

#[test]
fn the_force_checkbox_acknowledges_the_lease() {
    let project = Project::new();
    let (repo, _bare) = repo_ahead(&project, "alpha");
    let mut app = open_push(&project, &repo);
    app.press("id:sync:force").unwrap();
    app.wait_until("the lease is named", Duration::from_secs(10), |screen| {
        screen
            .texts()
            .iter()
            .any(|t| t.contains("--force-with-lease"))
    })
    .unwrap();
}

#[test]
fn the_tags_mode_reaches_the_push() {
    let project = Project::new();
    let (repo, bare) = repo_ahead(&project, "alpha");
    git(&repo, &["tag", "v9.0"]);
    let mut app = open_push(&project, &repo);
    app.press("id:sync:row-check-alpha").unwrap();
    app.press("id:sync:tags-All").unwrap();
    app.press("text:Push").unwrap();
    for _ in 0..600 {
        if git(&bare, &["tag", "--list", "v9.0"]).contains("v9.0") {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("--tags never reached the remote");
}

#[test]
fn a_row_that_cannot_be_pushed_offers_no_push() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    let mut app = open_push(&project, &repo);
    app.press("id:sync:row-check-alpha").unwrap();
    assert!(
        support::shows_part(&app, "no remote to push to"),
        "the obstacle is named"
    );
    assert!(
        support::shows_part(&app, "Nothing to update"),
        "a blocked-only dialog cannot push"
    );
}
