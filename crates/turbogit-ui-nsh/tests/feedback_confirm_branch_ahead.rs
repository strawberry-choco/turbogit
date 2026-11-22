//! The delete confirm's ahead-and-unmerged warning, ported from the egui
//! `feedback_confirm_branch_ahead` suite.

use std::path::PathBuf;
use std::time::Duration;

use support::{Project, branches_tab, open_branch_menu, shows_part};
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

#[test]
fn the_confirm_warns_when_the_branch_is_ahead_and_unmerged() {
    let project = Project::new();
    let repo = unmerged_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "feature");
    app.choose_menu_item("Delete branch").unwrap();
    app.wait_for("id:dialog:delete-local-branch").unwrap();
    assert!(
        shows_part(
            &app,
            "Delete local branch 'feature'? This cannot be undone."
        ),
        "the ask paints"
    );
    assert!(shows_part(&app, "ahead"), "the warning names ahead");
    assert!(
        shows_part(&app, "not merged"),
        "the warning names the merge"
    );
}

#[test]
fn the_confirm_omits_the_warning_when_the_branch_is_merged() {
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
            assert!(text.contains("Delete local branch 'feature'?"), "{text}");
            assert!(
                !text.contains("not merged"),
                "no warning when merged: {text}"
            );
            let _ = app.answer_dialog(nsh_testing::DialogAnswer::Press("Cancel".into()));
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no confirm appeared");
}
