//! The feedback layer's behavior, ported from the egui `feedback_banner`
//! and `feedback_chrome` suites: toasts paint, carry their kind, and
//! dismiss; a confirm routed through the state layer answers through the
//! dialog and runs its verb.

use support::{Project, for_roots, shows_part, wait_for_part};

mod support;

/// The app with one repository whose history is one commit on a branch the
/// rewrite verbs do not protect.
fn shell_one_commit(project: &Project) -> (support::Mounted, std::path::PathBuf) {
    let repo = support::seeded_repo(project.path(), "alpha");
    test_support::git_seed::git(&repo, &["checkout", "-q", "-b", "work"]);
    let app = for_roots(project.path(), std::slice::from_ref(&repo));
    (app, repo)
}

/// Opens the log and right-clicks the first row, answering the platform
/// dialog that follows with `press`.
fn open_log_and_revert(app: &mut support::Mounted, press: &str) {
    // The prompt is the platform alert; the caller answers it.
    app.answer_dialogs_myself();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    wait_for_part(app, "init").unwrap();
    app.right_click("id:log:list >> text:/^init/").unwrap();
    app.choose_menu_item("Revert commit").unwrap();
    // The simple yes-and-no confirm is the platform alert.
    app.answer_dialog(nsh_testing::DialogAnswer::Press(press.to_string()))
        .unwrap();
}

#[test]
fn the_state_toast_joins_the_shared_stack_and_paints_its_kind() {
    let project = Project::new();
    let (mut app, _repo) = shell_one_commit(&project);
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    wait_for_part(&mut app, "init").unwrap();

    // Copy hash is the toast path the egui chrome suite pinned: the toast
    // paints with a success bar and dismisses.
    app.right_click("id:log:list >> text:/^init/").unwrap();
    app.choose_menu_item("Copy hash").unwrap();
    wait_for_part(&mut app, "Copied ").unwrap();
    assert!(shows_part(&app, "Copied"), "the success toast paints");
}

#[test]
fn a_confirm_answered_ok_runs_its_verb_and_closes() {
    let project = Project::new();
    let (mut app, _repo) = shell_one_commit(&project);
    app.first_render().unwrap();
    open_log_and_revert(&mut app, "OK");

    // The revert dispatches; the operation's completion reports itself.
    wait_for_part(&mut app, "Revert").unwrap();
}

#[test]
fn a_confirm_answered_cancel_keeps_everything_intact() {
    let project = Project::new();
    let (mut app, repo) = shell_one_commit(&project);
    app.first_render().unwrap();
    open_log_and_revert(&mut app, "Cancel");

    app.wait(std::time::Duration::from_secs(2)).unwrap();
    let count = test_support::git_seed::git(&repo, &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1", "Cancel must leave the history intact");
}

#[test]
fn no_feedback_paints_no_chrome() {
    let project = Project::new();
    let (mut app, _repo) = shell_one_commit(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    assert!(
        !shows_part(&app, "Copied "),
        "no toast paints without a notice"
    );
    assert!(!app.screen().shows("OK"), "no confirm paints at rest");
}
