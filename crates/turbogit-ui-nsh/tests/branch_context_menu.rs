//! The branch row menu, ported from the egui `branch_context_menu` suite:
//! the designed order, the gates read off the branch's own facts, the menu's
//! verbs reaching git, and the copy verb.

use std::path::PathBuf;
use std::time::Duration;

use nsh_testing::DialogAnswer;
use support::{
    Project, branch_repo, branches_tab, menu_disabled, menu_labels, open_branch_menu, press_menu,
    seeded_repo,
};
use test_support::git_seed::git;
use turbogit_domain::model::{Branch, BranchKind, Upstream};
use turbogit_ui_nsh::screens::branches::{BranchMenuAction, branch_menu_items};

mod support;

fn local(name: &str, tracking: Option<&str>, ahead: usize, behind: usize) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: tracking.map(|up| Upstream::from_git_ref(up).expect("an upstream")),
        favorite: false,
        protected: false,
        exists: true,
        ahead,
        behind,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }
}

fn remote(name: &str, upstream: &str) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Remote,
        tracking: None,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: Some(upstream.to_string()),
    }
}

fn repo_with_remote(project: &Project, name: &str) -> PathBuf {
    let repo = seeded_repo(project.path(), name);
    for branch in ["feature-a", "feature-b"] {
        git(&repo, &["branch", branch, "main"]);
    }
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

#[test]
fn the_current_row_offers_no_checkout_and_no_pull() {
    let branch = local("main", Some("origin/main"), 0, 0);
    let items = branch_menu_items(&branch, Some("main"));
    assert!(!items[BranchMenuAction::Checkout.index()].enabled);
    assert!(!items[BranchMenuAction::Pull.index()].enabled);
    assert!(!items[BranchMenuAction::Delete.index()].enabled);
    assert!(items[BranchMenuAction::Rename.index()].enabled);
}

#[test]
fn an_untracked_branch_cannot_pull_but_still_checks_out_and_pushes() {
    let branch = local("feature", None, 0, 0);
    let items = branch_menu_items(&branch, Some("main"));
    assert!(!items[BranchMenuAction::Pull.index()].enabled);
    assert!(items[BranchMenuAction::Checkout.index()].enabled);
    assert!(
        items[BranchMenuAction::Push.index()].enabled,
        "a first push has an upstream to record"
    );
}

#[test]
fn a_gone_upstream_blocks_both_pull_items_and_the_push() {
    let mut branch = local("feature", Some("origin/feature"), 0, 0);
    branch.gone = true;
    let items = branch_menu_items(&branch, Some("main"));
    assert!(!items[BranchMenuAction::Pull.index()].enabled);
    assert!(!items[BranchMenuAction::CheckoutAndPull.index()].enabled);
    assert!(!items[BranchMenuAction::Push.index()].enabled);
}

#[test]
fn push_follows_what_the_upstream_owes_the_branch() {
    let up_to_date = local("feature", Some("origin/feature"), 0, 0);
    assert!(!branch_menu_items(&up_to_date, Some("main"))[BranchMenuAction::Push.index()].enabled);
    let ahead = local("feature", Some("origin/feature"), 2, 0);
    assert!(branch_menu_items(&ahead, Some("main"))[BranchMenuAction::Push.index()].enabled);
}

#[test]
fn a_remote_row_cannot_be_renamed_merged_or_rebased() {
    let branch = remote("origin/main", "origin");
    let items = branch_menu_items(&branch, Some("main"));
    assert!(!items[BranchMenuAction::Rename.index()].enabled);
    assert!(!items[BranchMenuAction::Merge.index()].enabled);
    assert!(!items[BranchMenuAction::Rebase.index()].enabled);
    assert!(items[BranchMenuAction::Delete.index()].enabled);
}

#[test]
fn the_menu_paints_the_twelve_items_in_the_designed_order() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "feature-a");
    assert_eq!(
        menu_labels(&app),
        vec![
            "Copy branch name",
            "Checkout",
            "New branch from",
            "Checkout and pull",
            "Merge into main",
            "Rebase onto main",
            "Compare with main",
            "Show Diff with Working Tree",
            "Pull",
            "Push",
            "Rename branch",
            "Delete branch",
        ]
    );
    assert!(menu_disabled(&app, "Pull"), "an untracked row cannot pull");
}

#[test]
fn checkout_moves_head_in_the_real_repository() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "Checkout");
    for _ in 0..400 {
        if git(&repo, &["branch", "--show-current"]).trim() == "feature-a" {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the checkout never moved HEAD");
}

#[test]
fn copy_branch_name_copies_the_row_it_was_opened_on() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "Copy branch name");
    assert_eq!(
        app.screen().clipboard(),
        Some("feature-a"),
        "the copied text is the row's own name"
    );
}

#[test]
fn new_branch_from_opens_the_dialog_over_the_rows_own_commit() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    open_branch_menu(&mut app, "alpha", "feature-a");
    press_menu(&mut app, "New branch from");
    app.wait_for("id:dialog:new-branch").unwrap();
    app.wait_until(
        "the start point names the row",
        Duration::from_secs(10),
        |screen| screen.texts().contains(&"feature-a"),
    )
    .unwrap();
}

#[test]
fn delete_asks_before_removing_and_a_remote_row_asks_upstream() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.answer_dialogs_myself();
    open_branch_menu(&mut app, "alpha", "feature-a");
    app.choose_menu_item("Delete branch").unwrap();
    for _ in 0..200 {
        if let Some(dialog) = app.pending_dialog() {
            let text = format!(
                "{} {}",
                dialog.title.clone().unwrap_or_default(),
                dialog.message.clone().unwrap_or_default()
            );
            assert!(
                text.contains("Delete local branch 'feature-a'?"),
                "the ask names the row: {text}"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the delete confirm never asked");
}

#[test]
fn a_remote_rows_delete_asks_about_the_upstream_branch() {
    let project = Project::new();
    let repo = repo_with_remote(&project, "alpha");
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.wait_until(
        "the remote band arrives",
        Duration::from_secs(10),
        |screen| {
            screen
                .matches("id:branches:band:alpha:remote")
                .unwrap_or(false)
        },
    )
    .unwrap();
    app.press("id:branches:band:alpha:remote").unwrap();
    app.wait_for("id:branches:band:alpha:remote-origin")
        .unwrap();
    app.press("id:branches:band:alpha:remote-origin").unwrap();
    app.wait_until(
        "the remote row arrives",
        Duration::from_secs(10),
        |screen| {
            screen
                .matches("id:branches:row:alpha:remote-only")
                .unwrap_or(false)
        },
    )
    .unwrap();
    app.answer_dialogs_myself();
    open_branch_menu(&mut app, "alpha", "remote-only");
    app.choose_menu_item("Delete branch").unwrap();
    for _ in 0..200 {
        if let Some(dialog) = app.pending_dialog() {
            let text = format!(
                "{} {}",
                dialog.title.clone().unwrap_or_default(),
                dialog.message.clone().unwrap_or_default()
            );
            assert!(
                text.contains("Delete remote branch 'origin/remote-only'?"),
                "the ask names the remote branch: {text}"
            );
            let _ = app.answer_dialog(DialogAnswer::Dismiss);
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the remote delete confirm never asked");
}
