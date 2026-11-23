//! The branch tree's rows, ported from the egui `branch_tree_view` suite at
//! the reduced fidelity the port uses: per-repository bands with their
//! state in words, the directory grouping, the upstream beside a name, the
//! live filter, and the dead end's create offer.

use std::path::PathBuf;
use std::time::Duration;

use support::{Project, branch_repo, branches_tab, seeded_repo, shows_part};
use test_support::git_seed::git;

mod support;

#[test]
fn a_rows_render_with_their_state_in_words_alone() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a"]);
    let app = branches_tab(project.path(), std::slice::from_ref(&repo));
    assert!(shows_part(&app, "▾ alpha"), "the repository band paints");
    assert!(
        app.screen().shows("✓"),
        "the current branch carries its marker"
    );
    assert!(
        app.screen().shows("now"),
        "a fresh branch's quiet age label paints"
    );
    assert!(
        !app.screen().shows("2019-01-01"),
        "the rows never paint absolute dates"
    );
}

#[test]
fn an_upstream_name_sits_beside_the_branch_name() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", "tracked"]);
    support::commit_into(&repo, "t.txt", "tracked work");
    git(&repo, &["checkout", "-q", "main"]);
    git(&repo, &["branch", "--set-upstream-to=main", "tracked"]);

    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.wait_until(
        "the upstream paints beside the row",
        Duration::from_secs(10),
        |screen| screen.texts().contains(&"main"),
    )
    .unwrap();
    assert!(app.screen().shows("tracked"), "the row itself paints");
}

#[test]
fn the_filter_narrows_the_tree_and_a_dead_end_offers_the_name() {
    let project = Project::new();
    let repo = branch_repo(project.path(), "alpha", &["feature-a", "hotfix"]);
    let mut app = branches_tab(project.path(), &[repo]);
    app.type_text("id:branches:search", "hot").unwrap();
    app.wait_until("the tree narrows live", Duration::from_secs(10), |screen| {
        screen
            .matches("id:branches:row:alpha:hotfix")
            .unwrap_or(false)
            && !screen
                .matches("id:branches:row:alpha:feature-a")
                .unwrap_or(false)
    })
    .unwrap();

    app.clear("id:branches:search").unwrap();
    app.type_text("id:branches:search", "no-such-branch")
        .unwrap();
    app.wait_for("id:branches:no-match").unwrap();
}

#[test]
fn a_directory_groups_its_branches_under_a_counted_header() {
    let project = Project::new();
    let repo = seeded_repo(project.path(), "alpha");
    git(&repo, &["branch", "feature/x", "main"]);
    git(&repo, &["branch", "feature/y", "main"]);
    let app = branches_tab(project.path(), std::slice::from_ref(&repo));
    assert!(
        shows_part(&app, "feature/ (2)"),
        "the derived directory count paints"
    );
}

#[test]
fn a_repository_band_reports_the_dirty_state_in_words() {
    let project = Project::new();
    let repo: PathBuf = branch_repo(project.path(), "alpha", &[]);
    std::fs::write(repo.join("README.md"), "dirty\n").unwrap();
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    app.wait_until(
        "the band states the dirty tree",
        Duration::from_secs(10),
        |screen| screen.texts().iter().any(|t| t.contains("dirty")),
    )
    .unwrap();
}
