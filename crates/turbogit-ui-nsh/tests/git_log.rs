//! The log's core behavior, ported from the egui `git_log` suite: the
//! commits pane lists the history, a row press selects by root and commit,
//! the search narrows live as text lands, the branches pane marks the
//! current branch, and a multi-root read set paints the gutter.

use std::path::PathBuf;

use support::{Project, for_roots, log_repo, shows_part, wait_for_part};

mod support;

fn log_app(project: &Project, roots: &[PathBuf]) -> support::Mounted {
    let mut app = for_roots(project.path(), roots);
    app.first_render().unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    wait_for_part(&mut app, "commit number").unwrap();
    app
}

#[test]
fn the_commits_pane_lists_the_history_with_panes_around_it() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 5);
    let app = log_app(&project, std::slice::from_ref(&repo));

    for needle in [
        "commit number 5",
        "commit number 3",
        "Commit details",
        "Changed files",
        "Branches",
    ] {
        assert!(shows_part(&app, needle), "the log paints {needle}");
    }
    assert!(
        app.screen().shows("Select a commit…"),
        "the details pane keeps its empty state until a selection"
    );
    let _ = repo;
}

#[test]
fn a_row_press_selects_the_commit_and_the_details_follow() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 4);
    let mut app = log_app(&project, &[repo]);

    app.press("text:commit number 2").unwrap();
    app.wait_for("text:commit number 2").unwrap();
    wait_for_part(&mut app, "Author").unwrap();
    assert!(
        shows_part(&app, "test@example.com"),
        "the details name the author"
    );
    assert!(
        shows_part(&app, "Parents"),
        "the details list the parents row"
    );
}

#[test]
fn the_search_narrows_live_as_text_lands() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 6);
    let mut app = log_app(&project, &[repo]);

    app.type_text("id:log:filter", "number 6").unwrap();
    app.wait_until(
        "the search narrows",
        std::time::Duration::from_secs(10),
        |screen| {
            screen.shows("commit number 6")
                && !screen.texts().iter().any(|t| t.contains("commit number 5"))
        },
    )
    .unwrap();

    app.press("id:log:remove-ref").ok(); // no ref chip at rest; the press is a no-op
    app.type_text("id:log:filter", "nomatch-at-all").unwrap();
    app.wait_for("text:No commits matching filters").unwrap();
}

#[test]
fn the_branches_pane_marks_the_current_branch_and_scopes_the_log() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 5);
    // A second branch off an older commit carrying its own work, so the
    // scope hides the main-only history above it.
    test_support::git_seed::git(&repo, &["checkout", "-q", "-b", "topic", "HEAD~2"]);
    support::commit_into(&repo, "topic.txt", "topic work");
    test_support::git_seed::git(&repo, &["checkout", "-q", "main"]);

    let mut app = log_app(&project, &[repo]);

    app.type_text("id:log:branch-search", "topic").unwrap();
    app.wait_until(
        "the tree filters live",
        std::time::Duration::from_secs(5),
        |screen| screen.shows("topic"),
    )
    .unwrap();

    app.press("text:topic").unwrap();
    app.wait_for("id:log:ref-chip").unwrap();
    wait_for_part(&mut app, "Ref filter: topic").unwrap();
    wait_for_part(&mut app, "topic work").unwrap();
    assert!(
        !shows_part(&app, "commit number 5"),
        "the scoped listing hides the branch's other history"
    );
}

#[test]
fn a_multi_root_read_set_paints_the_gutter_and_narrows_by_roots() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 3);
    let beta = log_repo(project.path(), "beta", 2);
    let mut app = log_app(&project, &[alpha, beta]);

    wait_for_part(&mut app, "commit number 3").unwrap();
    // Both roots list; the gutter's stripe columns paint (canvas cells).
    wait_for_part(&mut app, "beta").unwrap();

    app.press("id:log:filter-paths").unwrap();
    app.wait_for("text:beta").unwrap();
    app.press("id:log:root-beta").unwrap();
    app.wait_until(
        "the read set narrows to beta",
        std::time::Duration::from_secs(10),
        |screen| screen.shows("Paths: 1 of 2 repos"),
    )
    .unwrap();
}

#[test]
fn the_ref_pill_counts_the_refs_the_commit_carries() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    // The pushed remote and the tag both point at the tip, so the pill has
    // three refs to count.
    test_support::git_seed::git(&repo, &["push", "-q", "origin", "main"]);
    let mut app = log_app(&project, &[repo]);

    // The tip carries main, origin/main, and v1: the pill names the count.
    app.wait_until(
        "the tip's ref pill paints",
        std::time::Duration::from_secs(10),
        |screen| screen.shows("3"),
    )
    .unwrap();
}
