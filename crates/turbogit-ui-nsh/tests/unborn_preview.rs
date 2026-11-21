//! The unborn-repository behavior, ported from the egui `unborn_preview`
//! suite: a repository with no commits must never be asked for a first
//! commit, and the failure must name itself rather than paint rows.

use support::{Project, shows_part, wait_for_part};

mod support;

/// A repository with staged work and no commits.
fn unborn_tracked_repo(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
    let work = parent.join(name);
    std::fs::create_dir_all(&work).unwrap();
    test_support::git_seed::git(&work, &["init", "-q", "-b", "main"]);
    test_support::git_seed::git(&work, &["config", "user.email", "test@example.com"]);
    test_support::git_seed::git(&work, &["config", "user.name", "Test"]);
    std::fs::write(work.join("base.txt"), "alpha\nbravo\n").unwrap();
    test_support::git_seed::git(&work, &["add", "--", "base.txt"]);
    work
}

#[test]
fn an_unborn_repo_logs_nothing_and_names_why() {
    let project = Project::new();
    let work = unborn_tracked_repo(project.path(), "unborn");
    let mut app = support::for_roots(project.path(), &[work]);
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("id:route:log").unwrap();

    // The listing read answers with the unborn refusal; the pane names it
    // and paints no rows.
    wait_for_part(&mut app, "no commits yet").unwrap();
    assert!(
        !shows_part(&app, "alpha"),
        "no changed row of the unreadable history may be painted"
    );
    assert!(!shows_part(&app, "bravo"));
}

#[test]
fn painting_the_unreadable_listing_starts_no_git_operation() {
    let project = Project::new();
    let work = unborn_tracked_repo(project.path(), "unborn-noop");
    let status_before = test_support::git_seed::git(&work, &["status", "--porcelain"]);
    let staged_before = test_support::git_seed::git(&work, &["diff", "--cached"]);

    let mut app = support::for_roots(project.path(), std::slice::from_ref(&work));
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    wait_for_part(&mut app, "no commits yet").unwrap();
    app.wait(std::time::Duration::from_secs(2)).unwrap();

    assert_eq!(
        test_support::git_seed::git(&work, &["status", "--porcelain"]),
        status_before,
        "rendering a failed listing is not an operation"
    );
    assert_eq!(
        test_support::git_seed::git(&work, &["diff", "--cached"]),
        staged_before,
        "the index is exactly where the fixture left it"
    );
}

#[test]
fn a_first_commit_lands_the_listing_without_a_restart() {
    let project = Project::new();
    let work = unborn_tracked_repo(project.path(), "unborn-first");
    let mut app = support::for_roots(project.path(), std::slice::from_ref(&work));
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    wait_for_part(&mut app, "no commits yet").unwrap();

    // The user commits outside the app; the poll tick refreshes and the
    // listing arrives.
    test_support::git_seed::git(&work, &["commit", "-q", "-m", "first"]);
    app.wait(std::time::Duration::from_secs(6)).unwrap();
    app.wait_until(
        "the first commit lists",
        std::time::Duration::from_secs(15),
        |screen| screen.shows("first"),
    )
    .unwrap();
}
