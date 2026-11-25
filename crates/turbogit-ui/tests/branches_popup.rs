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

/// The trigger sits in the top bar's right-hand cluster, which never shrinks,
/// so the label is the cell that gives way: the trigger is cut at
/// `BRANCH_TRIGGER_MAX_WIDTH` and the cluster stays inside the window.
#[test]
fn a_long_branch_name_is_cut_in_the_trigger() {
    const LONG_BRANCH: &str = "feature/r202610w3-johann-async-docs-extremely-long-branch-name";
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", LONG_BRANCH]);
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();

    let nodes = app.screen().layout_nodes();
    let trigger = nodes
        .iter()
        .find(|node| node.test_id.as_deref() == Some("picker:trigger"))
        .expect("the picker trigger paints");
    let viewport = test_support::nsh_harness::VIEWPORT.0;
    let reaches = trigger.bounds.origin.x.0 + trigger.bounds.size.width.0;
    assert!(
        reaches <= viewport,
        "the trigger reaches {reaches}, past the {viewport} pt window"
    );
    assert!(
        trigger.bounds.size.width.0
            <= turbogit_ui::screens::branch_picker::BRANCH_TRIGGER_MAX_WIDTH + 1.0,
        "the trigger is capped, not laid out at the branch's full width: {}",
        trigger.bounds.size.width.0
    );

    let parents: std::collections::HashMap<_, _> =
        nodes.iter().map(|node| (node.id, node.parent)).collect();
    let label = nodes
        .iter()
        .filter(|node| {
            node.text
                .as_ref()
                .is_some_and(|t| t.text.text == LONG_BRANCH)
        })
        .find(|node| {
            let mut id = node.id;
            while let Some(parent) = parents.get(&id) {
                if *parent == trigger.id {
                    return true;
                }
                id = *parent;
            }
            false
        })
        .expect("the branch name paints in the trigger");
    let text = label.text.as_ref().expect("the label is a text view");
    assert!(
        label.bounds.size.width.0
            <= turbogit_ui::screens::branch_picker::BRANCH_LABEL_MAX_WIDTH + 1.0,
        "the label paints inside its cap: {}",
        label.bounds.size.width.0
    );
    assert_eq!(text.text.max_lines, 1, "the label paints on one line");
    assert_eq!(
        text.text.ellipsize,
        nsh_core::EllipsizeMode::Tail,
        "the cut end carries the ellipsis"
    );
}
