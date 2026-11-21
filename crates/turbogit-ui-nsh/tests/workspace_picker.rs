//! The workspace picker's behavior, ported from the egui
//! `workspace_picker` suite: the trigger opens it, Current names the open
//! workspace, a recent row re-enters, and the two launch verbs go through
//! the folder seam.

use std::path::PathBuf;

use support::{Project, shows_part, wait_for_part};

mod support;

fn shell_with_recents(project: &Project, recents: &[PathBuf]) -> support::Mounted {
    // The production launch opens the one repository and loads the store the
    // Recent half reads.
    let repo = support::seeded_repo(project.path(), "alpha");
    let config = tempfile::tempdir().expect("config dir");
    let seeded: Vec<turbogit_app::recents::RecentProject> = recents
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            support::recent_project(path.clone(), &name)
        })
        .collect();
    support::seed_recents(config.path(), &seeded);
    // The keeper rides on the Mounted? No: launched_over clones what the
    // state needs; this test keeps the tempdir itself.
    let mounted = support::launched_over(&repo, config.path());
    std::mem::forget(config);
    mounted
}

#[test]
fn the_trigger_opens_the_picker_over_the_shell() {
    let project = Project::new();
    let mut app = shell_with_recents(&project, &[]);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:workspace:switch").unwrap();
    app.wait_for("id:workspace:picker").unwrap();

    assert!(app.screen().shows("Current"), "the Current group paints");
    assert!(app.screen().shows("Recent"), "the Recent group paints");
    assert!(app.screen().shows("alpha"), "the open workspace paints");
    assert!(app.screen().shows("current"), "the current row is marked");
    assert!(app.screen().shows("Open Project…"));
    assert!(app.screen().shows("Attach Workspace Root…"));
}

#[test]
fn a_recent_row_reenters_that_workspace() {
    let project = Project::new();
    let beta = project.seed_repo("beta");
    let mut app = shell_with_recents(&project, std::slice::from_ref(&beta));
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:workspace:switch").unwrap();
    app.wait_for("id:workspace:picker").unwrap();

    app.press("label:beta").unwrap();
    app.wait_for("text:1 total").unwrap();
    assert!(
        shows_part(&app, "beta"),
        "the picker's row opened the row's own path"
    );
}

#[test]
fn the_open_workspace_answers_stay() {
    let project = Project::new();
    let mut app = shell_with_recents(&project, &[]);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:workspace:switch").unwrap();
    app.wait_for("id:workspace:picker").unwrap();
    app.press("id:workspace:row-current").unwrap();

    // The picker closes and nothing re-dispatches: the same workspace is
    // still the one registered.
    app.wait_until(
        "the picker closes",
        std::time::Duration::from_secs(5),
        |screen| !screen.matches("id:workspace:picker").unwrap_or(true),
    )
    .unwrap();
    assert!(app.screen().shows("1 total"));
}

#[test]
fn the_launch_verbs_go_through_the_folder_seam() {
    let project = Project::new();
    let picked = project.seed_repo("picked");
    let mut app = shell_with_recents(&project, &[]);
    // The standing open-dialog answer: the verb picks the fresh repo.
    app.on_pick_paths(vec![picked.clone()])
        .expect("stub the open dialog");
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:workspace:switch").unwrap();
    app.wait_for("id:workspace:picker").unwrap();
    app.press("id:workspace:open").unwrap();

    wait_for_part(&mut app, "picked").unwrap();
    assert!(
        app.screen().shows("1 total"),
        "Open Project… opened the picked folder as the one root"
    );
}

#[test]
fn a_workspace_recent_row_announces_its_repo_count() {
    let project = Project::new();
    let ws = project.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    support::seeded_repo(&ws, "one");
    support::seeded_repo(&ws, "two");

    let repo = support::seeded_repo(project.path(), "alpha");
    let config = tempfile::tempdir().expect("config dir");
    support::seed_recents(
        config.path(),
        &[turbogit_app::recents::RecentProject {
            path: ws.clone(),
            name: "ws".into(),
            last_opened: 1_755_000_000_000,
            kind: turbogit_app::recents::RecentKind::Workspace,
            repo_count: Some(2),
        }],
    );
    let mut app = support::launched_over(&repo, config.path());
    std::mem::forget(config);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();
    app.press("id:workspace:switch").unwrap();
    app.wait_for("id:workspace:picker").unwrap();

    assert!(
        shows_part(&app, "2 repos"),
        "the workspace row's count paints: {:?}",
        app.screen().texts()
    );
    // The a11y label carries it too, for a reader that cannot see the count.
    assert!(
        app.screen().matches("label:ws · 2 repos").unwrap_or(false),
        "the row's accessible name carries its count: {:?}",
        app.screen().texts()
    );
    app.press("id:workspace:row:ws").unwrap();
    app.wait_for("text:2 total").unwrap();
}
