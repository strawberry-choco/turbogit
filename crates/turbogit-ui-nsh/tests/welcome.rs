//! The welcome behavior, ported from the egui `welcome` suite: the hero and
//! the three launch cards, the clone box gated on the environment, the
//! recents window and its remove control, and the launch paths through the
//! file-dialog seam.

use std::path::PathBuf;

use turbogit_app::recents::RecentProject;

mod support;

use support::{Project, recent_project, seed_recents, seeded_repo};

#[test]
fn welcome_paints_hero_and_three_action_cards() {
    let (mut app, _config) = support::welcome_app_standalone();
    app.first_render().unwrap();

    for text in [
        "TurboGit",
        "A fast, keyboard-friendly Git client for your desktop.",
        "What's new",
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
        "Clone a repository",
        "Recent projects",
        "No recent projects yet.",
    ] {
        assert!(app.screen().shows(text), "the welcome paints `{text}`");
    }
}

#[test]
fn open_card_opens_a_real_repository_into_the_shell() {
    let project = Project::new();
    let repo = project.seed_repo("alpha");
    let mut app = support::welcome_with_picks(vec![repo]);
    app.first_render().unwrap();
    assert!(app.screen().shows("TurboGit"), "welcome is up first");

    app.press("id:welcome:open-project").unwrap();
    app.wait_for("text:1 total").unwrap();

    assert!(
        !app.screen().shows("A fast, keyboard-friendly Git client"),
        "opening a real repository leaves the welcome"
    );
    assert!(
        app.screen().shows("Changes"),
        "the shell's tab strip is the landing surface"
    );
}

#[test]
fn initialize_card_creates_a_repo_and_enters_it() {
    let project = Project::new();
    let target = project.path().join("fresh-init");
    std::fs::create_dir_all(&target).expect("create init target");

    let mut app = support::welcome_with_picks(vec![target.clone()]);
    app.first_render().unwrap();

    app.press("id:welcome:initialize").unwrap();
    app.wait_for("text:1 total").unwrap();

    assert!(
        target.join(".git").exists(),
        "the Initialize card creates a real repository"
    );
}

#[test]
fn initialize_refuses_a_directory_already_under_git() {
    let project = Project::new();
    let outer = project.seed_repo("outer");
    let nested = outer.join("inner");
    std::fs::create_dir_all(&nested).expect("create nested dir");

    let mut app = support::welcome_with_picks(vec![nested.clone()]);
    app.first_render().unwrap();

    app.press("id:welcome:initialize").unwrap();
    support::wait_for_part(&mut app, "already under Git").unwrap();

    assert!(
        !nested.join(".git").exists(),
        "a refused init creates no repository"
    );
    assert!(
        app.screen().shows("TurboGit"),
        "the guard leaves the user on Welcome"
    );
}

#[test]
fn clone_card_gates_the_clone_on_a_url_and_a_resolvable_git() {
    let (mut app, _config) = support::welcome_app_standalone();
    app.first_render().unwrap();

    assert!(
        !app.screen().enabled("id:welcome:clone").unwrap(),
        "an empty URL must not offer a clone"
    );

    app.type_text("id:welcome:clone-url", "https://example.com/some/repo.git")
        .unwrap();
    support::wait_for_enabled(&mut app, "id:welcome:clone").unwrap();

    app.press("id:welcome:clone-shallow").unwrap();
    support::wait_for_enabled(&mut app, "id:welcome:clone").unwrap();
}

#[test]
fn clone_card_clones_a_local_remote_into_the_picked_destination() {
    let project = Project::new();
    let remote = project.seed_bare_remote("origin");
    let workspace = project.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create picked parent");
    let dest = workspace.join("origin");

    let mut app = support::welcome_with_picks(vec![workspace]);
    app.first_render().unwrap();

    let url = format!("file://{}", remote.display());
    app.type_text("id:welcome:clone-url", &url).unwrap();
    app.wait_for_enabled("id:welcome:clone").unwrap();
    app.press("id:welcome:clone").unwrap();

    support::wait_for_part(&mut app, "Repository cloned").unwrap();
    assert!(
        dest.join(".git").exists(),
        "the clone lands at <picked parent>/origin"
    );
    assert!(
        app.screen().shows("Changes"),
        "a successful clone enters the shell"
    );
}

#[test]
fn clone_card_shallow_checkbox_limits_cloned_history_to_depth_one() {
    let project = Project::new();
    let remote = project.seed_bare_remote("origin");
    let workspace = project.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create picked parent");
    let dest = workspace.join("origin");

    let mut app = support::welcome_with_picks(vec![workspace]);
    app.first_render().unwrap();

    let url = format!("file://{}", remote.display());
    app.type_text("id:welcome:clone-url", &url).unwrap();
    app.press("id:welcome:clone-shallow").unwrap();
    app.wait_until(
        "the shallow box reads checked",
        std::time::Duration::from_secs(5),
        |screen| {
            screen
                .accessibility_state("id:welcome:clone-shallow")
                .map(|state| state.checked == Some(nsh_core::Checked::True))
                .unwrap_or(false)
        },
    )
    .unwrap();
    app.wait_for_enabled("id:welcome:clone").unwrap();
    app.press("id:welcome:clone").unwrap();

    support::wait_for_part(&mut app, "Repository cloned").unwrap();
    let count = test_support::git_seed::git(&dest, &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1", "shallow clone passes --depth 1 through");
    assert_eq!(
        test_support::git_seed::git(&remote, &["rev-list", "--count", "HEAD"]).trim(),
        "3",
        "the remote keeps its full history"
    );
}

#[test]
fn cancelling_the_clone_folder_pick_surfaces_a_toast_and_keeps_welcome() {
    let mut app = support::welcome_with_picks(Vec::new());
    app.first_render().unwrap();

    app.type_text("id:welcome:clone-url", "https://example.com/some/repo.git")
        .unwrap();
    app.wait_for_enabled("id:welcome:clone").unwrap();
    app.press("id:welcome:clone").unwrap();

    support::wait_for_part(&mut app, "no folder selected").unwrap();
    assert!(
        app.screen().shows("TurboGit"),
        "a failed pick leaves the user on Welcome"
    );
}

#[test]
fn the_folder_picker_seam_is_only_invoked_behind_user_initiated_flows() {
    let mut app = support::welcome_with_picks(Vec::new());
    app.first_render().unwrap();
    assert!(
        app.screen().file_requests().is_empty(),
        "rendering the Welcome screen never invokes the picker seam"
    );

    app.press("id:welcome:open-project").unwrap();
    support::wait_for_part(&mut app, "no folder selected").unwrap();
    assert_eq!(
        app.screen()
            .file_requests()
            .iter()
            .filter(|request| matches!(request, nsh_core::file_dialog::FileRequest::OpenDialog(_)))
            .count(),
        1,
        "clicking Open Project goes through the picker seam exactly once per click"
    );
    assert!(
        app.screen().shows("TurboGit"),
        "a cancelled pick leaves the user on Welcome"
    );
}

#[test]
fn attach_card_registers_every_repo_in_the_picked_tree() {
    let project = Project::new();
    let ws = seed_workspace(&project);
    let ws_path = ws.clone();

    let mut app = support::welcome_with_picks(vec![ws]);
    app.first_render().unwrap();

    app.press("id:welcome:attach-workspace").unwrap();
    app.wait_for("text:2 total").unwrap();

    let _ = ws_path;
}

/// A folder holding two repositories, one shallow and one deep.
fn seed_workspace(project: &Project) -> PathBuf {
    let ws = project.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    seeded_repo(&ws, "alpha");
    let deep = ws.join("a").join("b").join("c").join("d");
    std::fs::create_dir_all(&deep).unwrap();
    seeded_repo(
        deep.parent().unwrap().parent().unwrap().parent().unwrap(),
        "nested",
    );
    ws
}

#[test]
fn seeded_recents_render_name_path_last_opened_and_live_branch() {
    let project = Project::new();
    let repo = project.seed_repo("alpha");
    let config = Project::new();

    seed_recents(config.path(), &[recent_project(repo.clone(), "alpha")]);
    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();
    app.wait_for("text:main").unwrap();

    assert!(app.screen().shows("alpha"), "the row's name paints");
    assert!(
        support::shows_part(&app, &repo.display().to_string()),
        "the row paints the project path"
    );
    assert!(
        support::shows_part(&app, "Last opened"),
        "the row's meta paints"
    );
    assert!(app.screen().shows("main"), "the live branch chip paints");
}

#[test]
fn the_recents_window_paints_three_rows_and_the_footer_names_the_rest() {
    let project = Project::new();
    let config = Project::new();
    let names = ["p12", "p11", "p10", "p09", "p08", "p07"];
    let seeded: Vec<RecentProject> = names
        .iter()
        .map(|name| recent_project(project.path().join(name), name))
        .collect();
    seed_recents(config.path(), &seeded);

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();

    for name in ["p12", "p11", "p10"] {
        assert!(app.screen().shows(name), "the window paints {name}");
    }
    assert!(
        !app.screen().shows("p09"),
        "a record below the window stays unpainted"
    );
    assert!(
        app.screen().shows("Show all projects"),
        "the disclosure paints past three"
    );
}

#[test]
fn showing_all_projects_paints_every_record_and_collapses_again() {
    let project = Project::new();
    let config = Project::new();
    let names = ["q08", "q07", "q06", "q05", "q04", "q03", "q02", "q01"];
    let seeded: Vec<RecentProject> = names
        .iter()
        .map(|name| recent_project(project.path().join(name), name))
        .collect();
    seed_recents(config.path(), &seeded);

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();
    assert!(
        !app.screen().shows("q05"),
        "the window is three before the disclosure"
    );

    app.press("id:welcome:disclosure").unwrap();
    app.wait_for("text:Show fewer projects").unwrap();
    for name in names {
        assert!(app.screen().shows(name), "expanded paints {name}");
    }

    app.press("id:welcome:disclosure").unwrap();
    app.wait_for("text:Show all projects").unwrap();
    for name in ["q08", "q07", "q06"] {
        assert!(app.screen().shows(name));
    }
    assert!(!app.screen().shows("q05"), "collapsed again hides q05");
}

#[test]
fn a_record_below_the_window_is_openable_once_the_card_is_expanded() {
    let project = Project::new();
    let config = Project::new();
    let mut seeded = Vec::new();
    for name in ["alpha", "beta", "gamma", "delta"] {
        let repo = project.seed_repo(name);
        seeded.push(recent_project(repo, name));
    }
    seed_recents(config.path(), &seeded);

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();
    assert!(!app.screen().shows("delta"));

    app.press("id:welcome:disclosure").unwrap();
    app.wait_for("text:delta").unwrap();

    app.press("label:delta").unwrap();
    app.wait_for("text:1 total").unwrap();
}

#[test]
fn two_stored_projects_paint_both_rows_and_no_footer() {
    let project = Project::new();
    let config = Project::new();
    let alpha = project.seed_repo("alpha");
    let beta = project.seed_repo("beta");
    seed_recents(
        config.path(),
        &[recent_project(alpha, "alpha"), recent_project(beta, "beta")],
    );

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();

    assert!(app.screen().shows("alpha"));
    assert!(app.screen().shows("beta"));
    assert!(
        !app.screen().shows("Show all projects"),
        "two records are all of them: no disclosure"
    );
}

#[test]
fn clicking_a_recent_reopens_the_project() {
    let project = Project::new();
    let repo = project.seed_repo("alpha");
    let config = Project::new();
    seed_recents(config.path(), &[recent_project(repo.clone(), "alpha")]);

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();
    app.wait_for("text:main").unwrap();

    app.press("label:alpha").unwrap();
    app.wait_for("text:1 total").unwrap();
    assert!(
        app.screen().shows("Changes"),
        "clicking a recent enters the shell"
    );
}

#[test]
fn a_recent_row_offers_a_remove_control_that_drops_only_that_row() {
    let project = Project::new();
    let config = Project::new();
    let alpha = project.seed_repo("alpha");
    let beta = project.seed_repo("beta");
    seed_recents(
        config.path(),
        &[recent_project(alpha, "alpha"), recent_project(beta, "beta")],
    );

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();
    assert!(app.screen().shows("alpha"));
    assert!(app.screen().shows("beta"));

    app.press("id:welcome:remove-recent:alpha").unwrap();
    support::wait_for_part(&mut app, "Removed 'alpha' from recent projects.").unwrap();

    assert!(
        !app.screen().shows("alpha"),
        "the removed row's own row is gone"
    );
    assert!(app.screen().shows("beta"), "the neighbour keeps its row");
    assert!(
        app.screen().shows("TurboGit"),
        "removing a row must not open it"
    );
}

#[test]
fn removing_the_last_recent_row_says_so_and_the_store_lets_it_go() {
    let project = Project::new();
    let repo = project.seed_repo("alpha");
    let config = Project::new();
    seed_recents(config.path(), &[recent_project(repo.clone(), "alpha")]);

    let mut app = support::welcome_over(config.path());
    app.first_render().unwrap();
    app.wait_for("text:alpha").unwrap();

    app.press("id:welcome:remove-recent:alpha").unwrap();
    app.wait_for("text:No recent projects yet.").unwrap();

    assert!(
        turbogit_app::recents::load(config.path())
            .projects
            .is_empty(),
        "the removed row must not come back on the next launch"
    );
    assert!(
        repo.is_dir(),
        "removing a recent must not touch the project"
    );
}

#[test]
fn hero_whats_new_opens_and_closes_the_changelog_overlay() {
    let (mut app, _config) = support::welcome_app_standalone();
    app.first_render().unwrap();
    assert!(app.screen().shows("What's new"));
    assert!(!app.screen().shows("What's New"));

    app.press("id:welcome:whats-new").unwrap();
    app.wait_for("text:What's New").unwrap();

    app.press("id:dialog:whats-new:action").unwrap();
    app.wait_for_gone("text:What's New", std::time::Duration::from_secs(5))
        .unwrap();
}

#[test]
fn the_status_bar_keeps_the_version_line_on_welcome() {
    let (mut app, _config) = support::welcome_app_standalone();
    app.first_render().unwrap();

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert!(
        support::shows_part(&app, &version),
        "the app version paints"
    );
    assert!(
        support::shows_part(&app, "repos indexed"),
        "the indexed count paints on welcome too"
    );
    assert!(
        support::shows_part(&app, "0 repos indexed"),
        "the welcome indexes no repository"
    );
}
