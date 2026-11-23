//! The custom command, ported from the egui `custom_command` suite: the
//! fleet preflight runs a typed command across every root, a destructive
//! command arms before it runs, and a recent command refills the field.

use std::path::PathBuf;

use support::{Mounted, Project, for_roots_seeded, shows_part, wait_for_part};
use test_support::git_seed::git;

mod support;

fn two_repo_project(project: &Project) -> (PathBuf, PathBuf) {
    let frontend = project.path().join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = support::seeded_repo(&frontend, "alpha");
    let ui = support::seeded_repo(&frontend, "ui");
    (alpha, ui)
}

fn two_roots(project: &Project) -> Mounted {
    let (alpha, ui) = two_repo_project(project);
    for_roots_seeded(project.path(), &[alpha, ui], |s| {
        s.settings.protected_branch_patterns.clear();
    })
}

fn open_custom_route(app: &mut Mounted) {
    app.press("id:toolbar:n6-custom_command").unwrap();
    app.wait_for("id:route:custom_command").unwrap();
}

fn wait_done(app: &mut Mounted) {
    app.wait_until(
        "the fleet run settles",
        std::time::Duration::from_secs(30),
        |screen| screen.texts().iter().any(|text| text.starts_with("2 done")),
    )
    .unwrap();
}

#[test]
fn the_route_runs_the_typed_command_on_every_repository() {
    let project = Project::new();
    let mut app = two_roots(&project);
    app.first_render().unwrap();
    open_custom_route(&mut app);

    app.type_text("id:custom:input", "git branch bulk-marker")
        .unwrap();
    app.press("id:custom:open").unwrap();
    app.wait_for("id:bulk:preflight").unwrap();
    assert!(shows_part(&app, "Will run on all 2 repositories."));
    assert!(app.screen().shows("2 of 2 will run"));

    app.press("id:bulk:run").unwrap();
    app.wait_for("id:bulk:monitor").unwrap();
    wait_done(&mut app);

    for repo in [
        project.path().join("frontend/alpha"),
        project.path().join("frontend/ui"),
    ] {
        assert!(
            git(&repo, &["branch", "--list", "bulk-marker"]).contains("bulk-marker"),
            "{repo:?} got the branch"
        );
    }
}

#[test]
fn a_destructive_command_arms_before_it_runs() {
    let project = Project::new();
    let mut app = two_roots(&project);
    app.first_render().unwrap();
    open_custom_route(&mut app);

    app.type_text("id:custom:input", "git reset --hard")
        .unwrap();
    app.press("id:custom:open").unwrap();
    app.wait_for("id:bulk:preflight").unwrap();
    assert!(shows_part(&app, "Destructive command"));

    app.press("id:bulk:run").unwrap();
    assert!(
        !app.screen().matches("id:bulk:monitor").unwrap_or(false),
        "nothing dispatched on the first press"
    );
    assert!(
        shows_part(&app, "Yes, run git reset --hard"),
        "the confirm takes the first press"
    );
    app.press("id:bulk:run").unwrap();
    app.wait_for("id:bulk:monitor").unwrap();
}

#[test]
fn an_empty_command_runs_nothing() {
    let project = Project::new();
    let mut app = two_roots(&project);
    app.first_render().unwrap();
    open_custom_route(&mut app);

    app.press("id:custom:open").unwrap();
    app.wait_for("id:bulk:preflight").unwrap();
    assert!(
        !app.screen().enabled("id:bulk:run").unwrap(),
        "the run is gated on a typed command"
    );
    assert!(
        !app.screen().matches("id:bulk:monitor").unwrap_or(false),
        "no run without a command"
    );
}

#[test]
fn a_recent_command_refills_the_preflight_and_is_recorded() {
    let project = Project::new();
    let (alpha, ui) = two_repo_project(&project);
    let mut app = for_roots_seeded(project.path(), &[alpha, ui], |s| {
        s.settings.protected_branch_patterns.clear();
        s.ui.recent_custom_commands = vec!["git gc".to_string()];
    });
    app.first_render().unwrap();
    open_custom_route(&mut app);

    app.press("id:custom:recent:git gc").unwrap();
    app.wait_for("id:bulk:preflight").unwrap();
    app.wait_for("text:git gc").unwrap();
    app.press("id:bulk:run").unwrap();
    app.wait_for("id:bulk:monitor").unwrap();
    wait_done(&mut app);
    wait_for_part(&mut app, "git gc").unwrap();
}
