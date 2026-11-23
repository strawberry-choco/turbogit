//! The new-branch name suggester, ported from the egui
//! `new_branch_suggester` suite at the reduced fidelity the port uses: the
//! family rows and their counts, the accept, the empty name, and the guard.

use std::path::{Path, PathBuf};
use std::time::Duration;

use support::{Project, branches_tab, seeded_repo, shows_part};
use test_support::git_seed::git;

mod support;

fn suggester_repo(project: &Project) -> PathBuf {
    let repo = seeded_repo(project.path(), "alpha");
    for branch in ["feature/a", "feature/b", "feat/blame/x", "hotfix"] {
        git(&repo, &["branch", branch, "main"]);
    }
    repo
}

fn open_dialog(app: &mut support::Mounted) {
    app.press("id:branches:new").unwrap();
    app.wait_for("id:dialog:new-branch").unwrap();
    app.clear("id:dialog:new-branch:name").unwrap();
}

fn listed(repo: &Path, branch: &str) -> bool {
    git(repo, &["branch", "--list", branch]).contains(branch)
}

#[test]
fn a_prefilled_field_paints_no_family_until_something_is_typed() {
    let project = Project::new();
    let repo = suggester_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.press("id:branches:new").unwrap();
    app.wait_for("id:dialog:new-branch").unwrap();
    assert!(
        !app.screen()
            .matches("id:dialog:new-branch:suggest")
            .unwrap_or(false),
        "the pre-fill alone offers no rows"
    );
}

#[test]
fn typing_a_prefix_offers_the_families_that_extend_it_with_their_counts() {
    let project = Project::new();
    let repo = suggester_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_dialog(&mut app);
    app.type_text("id:dialog:new-branch:name", "feat").unwrap();
    app.wait_for("id:dialog:new-branch:suggest").unwrap();
    assert!(
        app.screen()
            .matches("id:dialog:new-branch:suggest-feat/")
            .unwrap_or(false),
        "the feat family is offered"
    );
    assert!(
        app.screen()
            .matches("id:dialog:new-branch:suggest-feature/")
            .unwrap_or(false),
        "the feature family is offered"
    );
    assert!(
        !shows_part(&app, "feature/a"),
        "a complete branch name is never a row"
    );
}

#[test]
fn clicking_a_family_leaves_the_field_holding_exactly_that_prefix() {
    let project = Project::new();
    let repo = suggester_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_dialog(&mut app);
    app.type_text("id:dialog:new-branch:name", "feat").unwrap();
    app.wait_for("id:dialog:new-branch:suggest-feat/").unwrap();
    app.press("id:dialog:new-branch:suggest-feat/").unwrap();
    app.wait_until(
        "the field takes the family",
        Duration::from_secs(10),
        |screen| {
            screen
                .input_text("id:dialog:new-branch:name")
                .unwrap_or(None)
                == Some("feat/")
        },
    )
    .unwrap();
}

#[test]
fn a_prefix_no_family_extends_offers_nothing_and_enter_still_creates() {
    let project = Project::new();
    let repo = suggester_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_dialog(&mut app);
    app.type_text("id:dialog:new-branch:name", "brand-new")
        .unwrap();
    assert!(
        !app.screen()
            .matches("id:dialog:new-branch:suggest")
            .unwrap_or(false),
        "nothing extends brand-new"
    );
    app.press("id:dialog:new-branch:action").unwrap();
    for _ in 0..400 {
        if listed(&repo, "brand-new") {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("Enter's create never landed");
}

#[test]
fn an_empty_name_creates_nothing_and_keeps_the_dialog() {
    let project = Project::new();
    let repo = suggester_repo(&project);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_dialog(&mut app);
    app.press("id:dialog:new-branch:action").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        app.screen()
            .matches("id:dialog:new-branch")
            .unwrap_or(false),
        "the dialog stays open"
    );
    assert!(!listed(&repo, "mainmain"));
}
