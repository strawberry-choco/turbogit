//! The branch pane builds only the rows its viewport can show, ported from
//! the egui `branches_frame_bounds` suite.

use std::time::Duration;

use support::{Project, branches_tab, seeded_repo};
use test_support::git_seed::git;

mod support;

fn many_branches(project: &Project, n: usize) -> std::path::PathBuf {
    let repo = seeded_repo(project.path(), "alpha");
    for i in 0..n {
        git(&repo, &["branch", &format!("branch-{i:04}"), "main"]);
    }
    repo
}

fn painted_rows(app: &support::Mounted) -> usize {
    app.screen()
        .texts()
        .iter()
        .filter(|text| text.starts_with("branch-"))
        .count()
}

#[test]
fn a_frame_builds_only_the_rows_its_viewport_can_show() {
    let project = Project::new();
    let repo = many_branches(&project, 400);
    let app = branches_tab(project.path(), std::slice::from_ref(&repo));
    let painted = painted_rows(&app);
    assert!(painted > 0, "the viewport paints rows");
    assert!(
        painted < 120,
        "the pane never builds all 400 rows: painted {painted}"
    );
}

#[test]
fn a_row_reached_by_scrolling_still_takes_its_click() {
    let project = Project::new();
    let repo = many_branches(&project, 400);
    let mut app = branches_tab(project.path(), std::slice::from_ref(&repo));
    for _ in 0..30 {
        app.scroll("id:branches:tree", nsh_engine::geometry::Points(400.0))
            .unwrap();
        app.wait(Duration::from_millis(20)).unwrap();
        if app
            .screen()
            .matches("id:branches:row:alpha:branch-0399")
            .unwrap_or(false)
        {
            break;
        }
    }
    app.wait_for("id:branches:row:alpha:branch-0399").unwrap();
    app.press("id:branches:row:alpha:branch-0399").unwrap();
    assert!(
        app.screen()
            .matches("id:branches:row:alpha:branch-0399")
            .unwrap_or(false),
        "the deep row is still on screen after its press"
    );
}
