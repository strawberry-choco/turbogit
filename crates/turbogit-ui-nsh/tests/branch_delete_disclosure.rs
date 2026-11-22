//! The delete disclosure, ported from the egui `branch_delete_disclosure`
//! suite: the unmerged link's count and viewer, the merged row's silence,
//! the tracking-remote offer, and Restore where the branch is really gone.

use std::path::{Path, PathBuf};
use std::time::Duration;

use nsh_testing::DialogAnswer;
use support::{Project, branches_tab, open_branch_menu, press_menu, shows_part};
use test_support::git_seed::git;

mod support;

fn unmerged_repo(project: &Project) -> PathBuf {
    let repo = support::seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    support::commit_into(&repo, "feature.txt", "feature work");
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["branch", "--set-upstream-to=main", "feature"]);
    repo
}

fn merged_repo(project: &Project) -> PathBuf {
    let repo = support::seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    git(&repo, &["checkout", "-q", "main"]);
    repo
}

fn listed(repo: &Path, branch: &str) -> bool {
    git(repo, &["branch", "--list", branch]).contains(branch)
}

#[test]
fn an_unmerged_delete_offers_its_commit_count_as_a_link() {
    let project = Project::new();
    let repo = unmerged_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.on_dialog(|_| DialogAnswer::Press("Cancel".to_string()));
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Delete branch");
    app.wait_for("id:dialog:delete-local-branch").unwrap();
    assert!(
        app.screen().shows("View 1 unmerged commit"),
        "the link counts one commit in the singular"
    );
    assert!(
        shows_part(&app, "this branch has 1 commit(s) not on main"),
        "the consequence names the count"
    );
}

#[test]
fn a_fully_merged_delete_offers_no_unmerged_link() {
    let project = Project::new();
    let repo = merged_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.answer_dialogs_myself();
    open_branch_menu(&mut app, "alpha", "feature");
    app.choose_menu_item("Delete branch").unwrap();
    for _ in 0..200 {
        if let Some(dialog) = app.pending_dialog() {
            let text = format!(
                "{} {}",
                dialog.title.clone().unwrap_or_default(),
                dialog.message.clone().unwrap_or_default()
            );
            assert!(
                text.contains("Delete local branch 'feature'?"),
                "the ask paints: {text}"
            );
            assert!(
                !text.contains("not merged") && !text.contains("unmerged"),
                "a merged branch gets no warning: {text}"
            );
            let _ = app.answer_dialog(DialogAnswer::Press("OK".to_string()));
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the delete ask never appeared");
}

#[test]
fn the_viewer_names_the_branch_and_the_reference_it_never_reached() {
    let project = Project::new();
    let repo = unmerged_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.on_dialog(|_| DialogAnswer::Press("Cancel".to_string()));
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Delete branch");
    app.wait_for("id:dialog:delete-local-branch").unwrap();
    app.press("text:View 1 unmerged commit").unwrap();
    app.wait_for("id:dialog:branch-not-fully-merged").unwrap();
    assert!(
        shows_part(&app, "The branch 'feature' was not fully merged to main."),
        "the viewer names both refs"
    );
    assert!(
        !app.screen().matches("text:Restore").unwrap_or(false),
        "the branch still exists, so it is not restored"
    );
}

#[test]
fn restore_is_offered_only_where_the_branch_is_gone() {
    let project = Project::new();
    let repo = unmerged_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Delete branch");
    app.wait_for("id:dialog:delete-local-branch").unwrap();
    app.press("id:dialog:delete-local-branch:action").unwrap();
    for _ in 0..400 {
        if !listed(&repo, "feature") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!listed(&repo, "feature"), "the branch is gone");

    app.wait_for("id:branches:undo").unwrap();
    app.press("id:branches:unmerged-link").unwrap();
    app.wait_for("id:dialog:branch-not-fully-merged").unwrap();
    assert!(
        app.screen().shows("Restore"),
        "a gone branch offers Restore"
    );
    app.press("text:Restore").unwrap();
    for _ in 0..400 {
        if listed(&repo, "feature") {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("Restore never recreated the branch");
}

#[test]
fn a_deleted_tracking_branch_offers_to_remove_the_remote_it_left_behind() {
    let project = Project::new();
    let repo = unmerged_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.on_dialog(|_| DialogAnswer::Press("OK".to_string()));
    open_branch_menu(&mut app, "alpha", "feature");
    press_menu(&mut app, "Delete branch");
    app.wait_for("id:dialog:delete-local-branch").unwrap();
    app.press("id:dialog:delete-local-branch:action").unwrap();
    app.wait_for("id:branches:undo").unwrap();
    assert!(
        app.screen()
            .matches("id:branches:remove-remote")
            .unwrap_or(false),
        "the tracking remote is offered for removal"
    );
}
