//! The grouping toggles, ported from the egui `branches_grouping` suite: the
//! gear's three documented toggles, directory nesting and prefix stripping,
//! the tags band, and repository-level grouping.

use std::time::Duration;

use support::{Project, branches_tab, seeded_repo, shows_part};
use test_support::git_seed::git;

mod support;

fn grouped_project() -> (Project, std::path::PathBuf) {
    let project = Project::new();
    let repo = seeded_repo(project.path(), "alpha");
    for branch in ["feature/a", "feature/b", "hotfix"] {
        git(&repo, &["branch", branch, "main"]);
    }
    git(&repo, &["tag", "v1.0"]);
    (project, repo)
}

fn open_gear(app: &mut support::Mounted) {
    app.press("id:branches:settings").unwrap();
    app.wait_for("id:branches:settings-menu").unwrap();
}

#[test]
fn the_gear_exposes_the_three_documented_toggles() {
    let (project, repo) = grouped_project();
    let mut app = branches_tab(project.path(), &[repo]);
    open_gear(&mut app);
    for toggle in ["toggle-directory", "toggle-repository", "toggle-tags"] {
        app.wait_for(&format!("id:branches:{toggle}")).unwrap();
    }
    assert!(shows_part(&app, "Group by Directory"));
    assert!(shows_part(&app, "Group by Repository"));
    assert!(shows_part(&app, "Show Tags"));
}

#[test]
fn directory_grouping_nests_the_rows_under_a_header() {
    let (project, repo) = grouped_project();
    let app = branches_tab(project.path(), &[repo]);
    assert!(
        shows_part(&app, "feature/ (2)"),
        "the directory header paints"
    );
    assert!(
        app.screen().shows("a") && app.screen().shows("b"),
        "the leaves strip the directory prefix"
    );
}

#[test]
fn turning_directory_grouping_off_paints_the_full_names() {
    let (project, repo) = grouped_project();
    let mut app = branches_tab(project.path(), &[repo]);
    open_gear(&mut app);
    app.press("id:branches:toggle-directory").unwrap();
    app.wait_until("the full names paint", Duration::from_secs(10), |screen| {
        screen
            .matches("id:branches:row:alpha:feature/a")
            .unwrap_or(false)
            && !screen.shows("feature/ (2)")
    })
    .unwrap();
}

#[test]
fn tags_show_only_under_the_toggle_and_expand_under_their_band() {
    let (project, repo) = grouped_project();
    let mut app = branches_tab(project.path(), &[repo]);
    assert!(shows_part(&app, "TAGS"), "the collapsed band paints");
    assert!(
        !app.screen().shows("v1.0"),
        "a collapsed tags band paints no tag row"
    );

    app.wait_until(
        "the tag evidence arrives",
        Duration::from_secs(10),
        |screen| screen.texts().iter().any(|t| t.contains("TAGS (1)")),
    )
    .unwrap();
    app.press("id:branches:band-tags").unwrap();
    app.wait_for("id:branches:tag-v1.0").unwrap();
    assert!(
        app.screen().shows("v1.0"),
        "the expanded band lists the tag"
    );

    open_gear(&mut app);
    app.press("id:branches:toggle-tags").unwrap();
    app.wait_until(
        "Show Tags off removes the band",
        Duration::from_secs(10),
        |screen| !screen.texts().iter().any(|t| t.contains("TAGS")),
    )
    .unwrap();
}

#[test]
fn repository_grouping_off_removes_the_repository_level_and_keeps_every_branch() {
    let (project, repo) = grouped_project();
    let mut app = branches_tab(project.path(), &[repo]);
    open_gear(&mut app);
    app.press("id:branches:toggle-repository").unwrap();
    app.wait_until(
        "the repository band is gone while the locals stay",
        Duration::from_secs(10),
        |screen| {
            !screen
                .matches("id:branches:band:alpha:repo")
                .unwrap_or(false)
                && screen
                    .matches("id:branches:row:alpha:hotfix")
                    .unwrap_or(false)
                && screen
                    .matches("id:branches:row:alpha:feature/a")
                    .unwrap_or(false)
        },
    )
    .unwrap();
}
