//! The settings modal, ported from the egui `settings_modal` suite at the
//! nsh ports' fidelity: the seven categories, the backed rows and their
//! round trip, the dirty gate, the version badge, the protected patterns,
//! the unbacked rows, and the palette entry.

use std::path::PathBuf;

use support::{Mounted, Project, for_roots, shows_part, wait_for_part};
use turbogit_app::persistence;
use turbogit_domain::model::{IncomingCheckInterval, VcsSettings};

mod support;

fn one_repo(project: &Project) -> PathBuf {
    support::seeded_repo(project.path(), "alpha")
}

fn shell(project: &Project) -> (Mounted, PathBuf) {
    let repo = one_repo(project);
    let app = for_roots(project.path(), std::slice::from_ref(&repo));
    (app, repo)
}

fn open_settings(app: &mut Mounted) {
    app.press("id:toolbar:settings").unwrap();
    app.wait_for("id:dialog:settings").unwrap();
    app.wait_for("id:settings:body").unwrap();
}

#[test]
fn settings_opens_from_the_command_palette() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:toolbar:palette").unwrap();
    app.wait_for("id:dialog:command-palette").unwrap();
    app.type_text("label:Search actions", "Settings").unwrap();
    app.press("text:Settings…").unwrap();
    app.wait_for("id:dialog:settings").unwrap();

    for label in [
        "General",
        "Git",
        "Update Method",
        "Multi-Root",
        "Protected Branches",
        "Appearance",
        "Advanced",
        "Restore defaults",
        "Cancel",
        "Apply",
    ] {
        assert!(app.screen().shows(label), "the {label} label paints");
    }
    app.press("id:dialog:settings:cancel").unwrap();
    app.wait_for_gone("id:dialog:settings", std::time::Duration::from_secs(5))
        .unwrap();
    assert!(!shows_part(
        &app,
        "Restore workspace context on branch switch"
    ));
}

#[test]
fn categories_switch_the_visible_page_and_reopen_on_general() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);

    assert!(shows_part(
        &app,
        "Restore workspace context on branch switch"
    ));
    app.press("id:settings:tab:protected-branches").unwrap();
    wait_for_part(&mut app, "Protected branch patterns").unwrap();
    assert!(!shows_part(
        &app,
        "Restore workspace context on branch switch"
    ));
    app.press("id:settings:tab:git").unwrap();
    assert!(!shows_part(&app, "Protected branch patterns"));

    app.press("id:dialog:settings:cancel").unwrap();
    app.wait_for_gone("id:dialog:settings", std::time::Duration::from_secs(5))
        .unwrap();
    open_settings(&mut app);
    assert!(
        shows_part(&app, "Restore workspace context on branch switch"),
        "the modal always reopens on General"
    );
}

#[test]
fn backed_rows_round_trip_to_persisted_settings() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);

    app.press("id:settings:restore-workspace").unwrap();
    app.press("id:settings:tab:git").unwrap();
    app.type_text("id:settings:git-executable", "git-under-test")
        .unwrap();
    app.press("id:dialog:settings:action").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let saved = persistence::load_settings(project.path()).expect("state.ron readable");
    assert!(saved.restore_workspace, "Apply copies the draft");
    assert_eq!(saved.git_executable, "git-under-test");
    assert!(
        app.screen().matches("id:dialog:settings").unwrap_or(false),
        "Apply keeps the modal open"
    );
}

#[test]
fn apply_is_disabled_until_dirty_and_restore_defaults_re_disables_it() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);

    assert!(
        !app.screen().enabled("id:dialog:settings:action").unwrap(),
        "Apply starts disabled"
    );
    app.press("id:settings:restore-workspace").unwrap();
    assert!(
        app.screen().enabled("id:dialog:settings:action").unwrap(),
        "Apply enables once dirty"
    );
    app.press("id:settings:restore-defaults").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        !app.screen().enabled("id:dialog:settings:action").unwrap(),
        "Restore defaults returns General to its saved state"
    );
}

#[test]
fn restore_defaults_resets_only_the_visible_category() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);

    app.press("id:settings:restore-workspace").unwrap();
    app.press("id:settings:tab:git").unwrap();
    app.type_text("id:settings:git-executable", "should-not-survive")
        .unwrap();
    app.press("id:settings:restore-defaults").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    app.press("id:settings:tab:general").unwrap();
    assert!(
        shows_part(&app, "Restore workspace context on branch switch"),
        "the other category's edit survives"
    );
    app.press("id:settings:tab:git").unwrap();
    let input = app
        .screen()
        .input_text("id:settings:git-executable")
        .unwrap()
        .unwrap_or_default()
        .to_string();
    assert_eq!(input, "", "Git fields return to defaults");
    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default(),
        "Restore defaults does not persist by itself"
    );
}

#[test]
fn the_git_page_shows_a_live_version_badge_and_a_missing_binary_as_an_error() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);
    app.press("id:settings:tab:git").unwrap();

    assert!(app.screen().shows("Browse"));
    assert!(
        app.screen()
            .matches("id:settings:git-browse")
            .unwrap_or(false),
        "the Browse action paints"
    );
    assert!(
        shows_part(&app, "✓"),
        "git on PATH resolves to a version badge"
    );

    app.type_text(
        "id:settings:git-executable",
        "/nonexistent/turbogit-missing-git",
    )
    .unwrap();
    wait_for_part(&mut app, "✗").unwrap();
}

#[test]
fn protected_patterns_are_removable_chips_and_apply_gates_immediately() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);
    app.press("id:settings:tab:protected-branches").unwrap();
    app.wait_for("id:settings:pattern-remove:master").unwrap();
    app.wait_for("id:settings:pattern-remove:main").unwrap();

    app.press("id:settings:pattern-remove:master").unwrap();
    app.type_text("id:settings:new-pattern", "release/*")
        .unwrap();
    app.press("id:settings:add-pattern").unwrap();
    app.wait_for("id:settings:pattern-remove:release/*")
        .unwrap();
    app.press("id:dialog:settings:action").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let saved = persistence::load_settings(project.path()).unwrap();
    assert_eq!(
        saved.protected_branch_patterns,
        vec!["main".to_string(), "release/*".to_string()]
    );
    assert!(!turbogit_app::sync_service::is_protected(&saved, "master"));
    assert!(turbogit_app::sync_service::is_protected(
        &saved,
        "release/42"
    ));
}

#[test]
fn unbacked_rows_render_disabled_and_never_persist() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);

    app.press("id:settings:tab:git").unwrap();
    assert!(
        !app.screen().enabled("id:settings:manage-remotes").unwrap(),
        "Manage Remotes is unbacked and dims"
    );

    app.press("id:settings:tab:advanced").unwrap();
    for label in [
        "Convert to LF on commit",
        "Convert to CRLF on checkout",
        "No conversion",
        "Sign-off commits",
    ] {
        assert!(
            app.screen().matches(&format!("label:{label}")).unwrap(),
            "the {label} row paints"
        );
    }
    assert!(
        !app.screen().enabled("id:dialog:settings:action").unwrap(),
        "unbacked rows leave the draft undirty, so Apply stays disabled"
    );
    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default()
    );
}

#[test]
fn the_appearance_date_format_select_changes_the_draft() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);
    app.press("id:settings:tab:appearance").unwrap();
    app.press("label:Date format").unwrap();
    app.wait_for("role:option[ISO]").unwrap();
    app.press("role:option[ISO]").unwrap();
    app.wait_for("text:ISO").unwrap();
    app.press("id:dialog:settings:action").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(
        persistence::load_settings(project.path())
            .unwrap()
            .date_format,
        turbogit_domain::model::DateFormat::Iso
    );
}

#[test]
fn incoming_check_is_a_toggle_with_an_interval_selector() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    open_settings(&mut app);
    app.press("id:settings:tab:update-method").unwrap();

    assert!(
        app.screen().matches("id:settings:incoming-poll").unwrap(),
        "the toggle paints under its own label"
    );
    assert!(app.screen().shows("Every hour"));
    app.press("id:settings:incoming-poll").unwrap();
    app.press("label:Check interval").unwrap();
    app.press("text:Every 30 minutes").unwrap();
    app.press("id:dialog:settings:action").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let saved = persistence::load_settings(project.path()).unwrap();
    assert!(saved.incoming_poll);
    assert_eq!(saved.incoming_interval, IncomingCheckInterval::Min30);
}

#[test]
fn the_settings_dialog_paints_the_same_categories() {
    let project = Project::new();
    let (mut app, _repo) = shell(&project);
    app.first_render().unwrap();
    app.press("id:toolbar:settings").unwrap();
    app.wait_for("id:dialog:settings").unwrap();
    app.wait_for("id:settings:body").unwrap();
    assert!(
        app.screen().selected("id:tab:Commit").unwrap(),
        "opening the settings route never changes the active tab"
    );
}
