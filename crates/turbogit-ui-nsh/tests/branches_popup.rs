//! The top-bar branch picker, ported from the egui `branches_popup` suite:
//! the groups and members, the pinned current branch, checkout from a row,
//! the live filter, the multi-root notice, and the footer verbs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Project, for_roots, for_roots_seeded, shows_part};
use test_support::git_seed::git;

mod support;

fn repo_with_remote(project: &Project, name: &str) -> PathBuf {
    let repo = support::seeded_repo(project.path(), name);
    for branch in ["feature-a", "feature-b", "zebra"] {
        git(&repo, &["branch", branch, "main"]);
    }
    git(&repo, &["tag", "v1.0"]);
    let bare = project.path().join(format!("{name}-origin.git"));
    let bare_s = bare.to_string_lossy().to_string();
    git(
        project.path(),
        &["clone", "--bare", "-q", repo.to_str().unwrap(), &bare_s],
    );
    git(&repo, &["remote", "add", "origin", &bare_s]);
    git(&repo, &["push", "-q", "origin", "main:remote-only"]);
    git(&repo, &["fetch", "-q", "origin"]);
    repo
}

fn wait_for_branch(dir: &Path, branch: &str) {
    for _ in 0..400 {
        if git(dir, &["branch", "--show-current"]).trim() == branch {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "checkout never moved HEAD to {branch}; it is {}",
        git(dir, &["branch", "--show-current"]).trim()
    );
}

fn open_picker(app: &mut support::Mounted) {
    app.press("id:picker:trigger").expect("the picker trigger");
    app.wait_for("id:picker:list").expect("the branch list");
}

#[test]
fn the_picker_paints_its_groups_and_members() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = for_roots(project.path(), &[repo]);
    app.first_render().unwrap();
    open_picker(&mut app);

    for group in ["LOCAL", "REMOTE", "TAGS"] {
        assert!(shows_part(&app, group), "the picker paints {group}");
    }
    for member in [
        "feature-a",
        "feature-b",
        "zebra",
        "origin/remote-only",
        "v1.0",
    ] {
        assert!(shows_part(&app, member), "the picker lists {member}");
    }
}

#[test]
fn the_current_branch_is_pinned_and_checkmarked() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = for_roots(project.path(), &[repo]);
    app.first_render().unwrap();
    open_picker(&mut app);

    app.wait_for("id:picker:current").expect("the pinned row");
    assert!(shows_part(&app, "✓ main"), "the pinned row is checkmarked");
}

#[test]
fn a_local_rows_press_checks_it_out() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();
    open_picker(&mut app);

    app.press("text:feature-a").expect("the local row");
    wait_for_branch(&repo, "feature-a");
}

#[test]
fn a_remote_rows_press_creates_the_local_tracking_branch() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();
    open_picker(&mut app);

    app.press("text:origin/remote-only")
        .expect("the remote row");
    wait_for_branch(&repo, "remote-only");
}

#[test]
fn typing_filters_the_list_live() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = for_roots(project.path(), &[repo]);
    app.first_render().unwrap();
    open_picker(&mut app);

    app.type_text("label:Search branches", "zeb")
        .expect("the command's search");
    app.wait_until(
        "the list narrows to the zebra row",
        Duration::from_secs(10),
        |screen| screen.matches("text:zebra").unwrap_or(false),
    )
    .unwrap();
}

#[test]
fn a_multi_root_workspace_states_the_synchronous_scope() {
    let project = Project::new();
    let alpha = repo_with_remote(&project, "alpha");
    let beta = support::seeded_repo(project.path(), "beta");
    let mut app = for_roots_seeded(project.path(), &[alpha, beta], |s| {
        s.settings.synchronous_branches = true;
    });
    app.first_render().unwrap();
    open_picker(&mut app);
    assert!(
        shows_part(&app, "Synchronous branch operations across 2 repositories"),
        "the notice states the scope"
    );
}

#[test]
fn the_footer_verbs_open_their_dialogs() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = for_roots(project.path(), &[repo]);
    app.first_render().unwrap();
    open_picker(&mut app);
    app.press("id:picker:new").expect("the New branch verb");
    app.wait_for("id:dialog:new-branch")
        .expect("the New Branch dialog");
}
