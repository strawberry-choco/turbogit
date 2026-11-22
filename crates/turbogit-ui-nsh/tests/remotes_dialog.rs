//! The Remotes dialog, ported from the egui `remotes_dialog` suite: the
//! listing, add, rename, edit both URLs, remove, set upstream, and the
//! repository scope every add runs over.

use std::path::PathBuf;
use std::time::Duration;

use support::{Project, branches_tab};
use test_support::git_seed::git;

mod support;

fn remote_repo(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    let bare = project.path().join(format!("{name}-origin.git"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["clone", "--bare", "-q", repo.to_str().unwrap(), &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["fetch", "-q", "origin"]);
    repo
}

fn open_manager(app: &mut support::Mounted) {
    app.press("id:picker:trigger").unwrap();
    app.wait_for("id:picker:list").unwrap();
    app.press("id:picker:remotes").unwrap();
    app.wait_for("id:dialog:manage-remotes").unwrap();
}

#[test]
fn the_manager_lists_each_remote_with_its_urls() {
    let project = Project::new();
    let repo = remote_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_manager(&mut app);
    app.wait_for("id:remote:row-origin").unwrap();
    assert!(
        support::shows_part(&app, "fetch: /"),
        "the fetch URL paints"
    );
}

#[test]
fn add_remote_dispatches_and_the_list_refreshes() {
    let project = Project::new();
    let repo = remote_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_manager(&mut app);
    app.type_text("id:remote:add-name", "upstream").unwrap();
    app.type_text("id:remote:add-fetch", "https://example.com/repo.git")
        .unwrap();
    app.press("id:remote:add").unwrap();
    for _ in 0..400 {
        if git(&repo, &["remote"]).contains("upstream") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(git(&repo, &["remote"]).contains("upstream"));
    app.wait_for("id:remote:row-upstream").unwrap();
}

#[test]
fn edit_url_writes_both_urls() {
    let project = Project::new();
    let repo = remote_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_manager(&mut app);
    app.press("id:remote:edit-origin").unwrap();
    app.wait_for("id:remote:edit-fetch").unwrap();
    app.clear("id:remote:edit-fetch").unwrap();
    app.type_text("id:remote:edit-fetch", "https://example.com/fetch.git")
        .unwrap();
    app.clear("id:remote:edit-push").unwrap();
    app.type_text("id:remote:edit-push", "https://example.com/push.git")
        .unwrap();
    app.press("id:remote:edit-save").unwrap();
    for _ in 0..400 {
        if git(&repo, &["remote", "get-url", "--push", "origin"]).contains("example.com/push.git")
            && git(&repo, &["remote", "get-url", "origin"]).contains("example.com/fetch.git")
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "the edited URL never landed: {:?}",
        git(&repo, &["remote", "-v"])
    );
}

#[test]
fn rename_and_remove_reach_git() {
    let project = Project::new();
    let repo = remote_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_manager(&mut app);
    app.press("id:remote:rename-origin").unwrap();
    app.wait_for("id:remote:rename-input").unwrap();
    app.clear("id:remote:rename-input").unwrap();
    app.type_text("id:remote:rename-input", "upstream").unwrap();
    app.press("id:remote:rename-save").unwrap();
    for _ in 0..400 {
        if git(&repo, &["remote"]).contains("upstream") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(git(&repo, &["remote"]).contains("upstream"));

    app.wait_for("id:remote:remove-upstream").unwrap();
    app.press("id:remote:remove-upstream").unwrap();
    for _ in 0..400 {
        if !git(&repo, &["remote"]).contains("upstream") {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the removed remote survived");
}

#[test]
fn set_upstream_per_branch_dispatches() {
    let project = Project::new();
    let repo = remote_repo(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_manager(&mut app);
    app.press("id:remote:upstream-origin").unwrap();
    app.wait_for("id:remote:upstream-main").unwrap();
    app.press("id:remote:upstream-main").unwrap();
    for _ in 0..400 {
        if git(&repo, &["config", "--get", "branch.main.remote"]).trim() == "origin" {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the upstream was never recorded");
}

#[test]
fn add_remote_runs_in_every_repository_in_scope() {
    let project = Project::new();
    let alpha = remote_repo(&project, "alpha");
    let beta = remote_repo(&project, "beta");
    let mut app = branches_tab(project.path(), &[alpha.clone(), beta.clone()]);
    open_manager(&mut app);
    app.type_text("id:remote:add-name", "team").unwrap();
    app.type_text("id:remote:add-fetch", "https://example.com/team.git")
        .unwrap();
    app.press("id:remote:add").unwrap();
    for _ in 0..600 {
        if git(&alpha, &["remote"]).contains("team") && git(&beta, &["remote"]).contains("team") {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "scope add did not reach both: alpha={:?} beta={:?}",
        git(&alpha, &["remote"]),
        git(&beta, &["remote"])
    );
}
