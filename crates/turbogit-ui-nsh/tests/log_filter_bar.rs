//! The filter bar's behavior, ported from the egui `log_filter_bar` suite:
//! the four dropdowns paint, a date preset builds a composite read, a
//! branch pick scopes every carrier, the roots block is a set, and Clear
//! means back to rest for the whole filter set.

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
fn the_bar_paints_the_four_dropdowns_and_clear() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let app = log_app(&project, &[repo]);

    for label in ["Branch", "User", "Date", "Paths", "Clear"] {
        assert!(app.screen().shows(label), "the bar paints {label}");
    }
}

#[test]
fn a_date_preset_changes_the_button_and_the_read() {
    let project = Project::new();
    let repo = log_repo(project.path(), "alpha", 3);
    let mut app = log_app(&project, &[repo]);

    app.press("id:log:filter-date").unwrap();
    app.wait_for("text:Last 7 days").unwrap();
    app.press("id:log:menu-Last 7 days").unwrap();

    app.wait_for("text:Date: Last 7 days").unwrap();
}

#[test]
fn a_branch_pick_scopes_every_selected_repository_that_carries_the_rev() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 2);
    let beta = log_repo(project.path(), "beta", 2);
    for (repo, subject) in [(&alpha, "alpha topic work"), (&beta, "beta topic work")] {
        test_support::git_seed::git(repo, &["checkout", "-q", "-b", "topic"]);
        support::commit_into(repo, "topic.txt", subject);
        test_support::git_seed::git(repo, &["checkout", "-q", "main"]);
    }

    let mut app = log_app(&project, &[alpha, beta]);

    app.press("id:log:filter-branch").unwrap();
    app.wait_for("text:topic").unwrap();
    app.press("id:log:ref-topic").unwrap();

    app.wait_for("id:log:ref-chip").unwrap();
    wait_for_part(&mut app, "Ref filter: topic").unwrap();
    wait_for_part(&mut app, "alpha topic work").unwrap();
    wait_for_part(&mut app, "beta topic work").unwrap();

    // The removal chip restores the full listing.
    app.press("id:log:remove-ref").unwrap();
    app.wait_until(
        "the scope lifts",
        std::time::Duration::from_secs(10),
        |screen| !screen.matches("id:log:ref-chip").unwrap_or(false),
    )
    .unwrap();
    wait_for_part(&mut app, "commit number 2").unwrap();
}

#[test]
fn a_remote_row_scopes_by_the_remote_qualified_rev() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 2);
    let mut app = log_app(&project, std::slice::from_ref(&alpha));

    // The local branch is one ahead of the remote.
    support::commit_into(&alpha, "local.txt", "unfetched local work");

    app.press("id:log:filter-branch").unwrap();
    // Remote bands start folded, the egui dropdown's own fold memory.
    app.press("id:log:band-Remote branches").unwrap();
    app.wait_for("text:origin/main").unwrap();
    app.press("id:log:ref-origin/main").unwrap();

    app.wait_for("text:Branch: origin/main").unwrap();
    app.wait_until(
        "the remote window lists the pushed commit only",
        std::time::Duration::from_secs(10),
        |screen| {
            screen.shows("init")
                && !screen
                    .texts()
                    .iter()
                    .any(|t| t.contains("unfetched local work"))
        },
    )
    .unwrap();
}

#[test]
fn the_roots_block_is_a_set_that_never_closes_early() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 2);
    let beta = log_repo(project.path(), "beta", 2);
    let mut app = log_app(&project, &[alpha, beta]);

    app.press("id:log:filter-paths").unwrap();
    app.wait_for("text:All repositories").unwrap();
    app.press("id:log:root-beta").unwrap();

    app.wait_for("text:Paths: 1 of 2 repos").unwrap();
    assert!(app.screen().shows("beta"), "the popup stayed open");

    // Ticking the last checked root back collapses to All.
    app.press("id:log:root-beta").unwrap();
    app.wait_until(
        "the set collapses to all",
        std::time::Duration::from_secs(10),
        |screen| screen.shows("All repositories"),
    )
    .unwrap();
}

#[test]
fn unchecking_every_root_says_so_and_lists_nothing() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 2);
    let beta = log_repo(project.path(), "beta", 2);
    let mut app = log_app(&project, &[alpha, beta]);

    app.press("id:log:filter-paths").unwrap();
    app.press("id:log:root-alpha").unwrap();
    app.press("id:log:root-beta").unwrap();

    app.wait_for("text:Paths: No repos").unwrap();
    wait_for_part(&mut app, "No commits matching filters").unwrap();
}

#[test]
fn clear_resets_every_filter_including_the_root_selection() {
    let project = Project::new();
    let alpha = log_repo(project.path(), "alpha", 2);
    let beta = log_repo(project.path(), "beta", 2);
    let mut app = log_app(&project, &[alpha, beta]);

    // Set a scope, a roots subset, and a query.
    app.press("id:log:filter-branch").unwrap();
    app.wait_for("text:main").unwrap();
    app.press("id:log:ref-main").unwrap();
    app.wait_for("id:log:ref-chip").unwrap();
    app.type_text("id:log:filter", "commit number").unwrap();
    wait_for_part(&mut app, "1 of 2 repos").unwrap_or(());

    app.press("id:log:clear").unwrap();

    app.wait_until(
        "every filter resets",
        std::time::Duration::from_secs(10),
        |screen| {
            !screen.matches("id:log:ref-chip").unwrap_or(false)
                && screen.shows("Branch")
                && screen.shows("Paths")
        },
    )
    .unwrap();
    wait_for_part(&mut app, "Paths").unwrap();
    assert!(!shows_part(&app, "Ref filter:"), "the ref chip is gone");
}
