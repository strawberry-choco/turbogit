//! The command palette, ported from the egui `popups` command path: the
//! keyboard entry, the filtered list, and the entries that land on the
//! branch, log, and settings surfaces.

use support::{Mounted, Project, shows_part, wait_for_part};

mod support;

fn shell(project: &Project) -> Mounted {
    let repo = support::seeded_repo(project.path(), "alpha");
    support::for_roots(project.path(), std::slice::from_ref(&repo))
}

fn open_palette(app: &mut Mounted) {
    app.press_key("ctrl+shift+a").unwrap();
    app.wait_for("id:dialog:command-palette").unwrap();
}

#[test]
fn ctrl_shift_a_opens_the_palette_and_settings_lands_on_its_dialog() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    open_palette(&mut app);

    assert!(app.screen().shows("Find Action"));
    app.type_text("label:Search actions", "Settings").unwrap();
    app.press("text:Settings…").unwrap();
    app.wait_for("id:dialog:settings").unwrap();
    assert!(
        !app.screen()
            .matches("id:dialog:command-palette")
            .unwrap_or(false),
        "the palette closes behind the surface it opened"
    );
}

#[test]
fn the_palette_lands_on_the_branches_and_the_log() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();

    open_palette(&mut app);
    app.type_text("label:Search actions", "Branches").unwrap();
    app.press("text:Branches…").unwrap();
    app.wait_for("id:route:branches").unwrap();
    assert!(
        app.screen().selected("id:tab:Branches").unwrap(),
        "Branches… selects the Branches tab"
    );

    open_palette(&mut app);
    app.type_text("label:Search actions", "Go to Log").unwrap();
    app.press("text:Go to Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    assert!(app.screen().selected("id:tab:Log").unwrap());
}

#[test]
fn ctrl_t_refreshes_without_disturbing_the_shell_and_ctrl_k_returns_to_commit() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    app.press_key("ctrl+t").unwrap();
    app.wait_for("text:Commits").unwrap();
    assert!(!app.screen().shows("Find Action"));

    app.press_key("ctrl+k").unwrap();
    app.wait_for("id:route:changes").unwrap();
    assert!(app.screen().selected("id:tab:Commit").unwrap());
}

#[test]
fn an_empty_result_names_itself() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    open_palette(&mut app);

    app.type_text("label:Search actions", "zzzz-no-action")
        .unwrap();
    wait_for_part(&mut app, "No matching actions.").unwrap();
}

#[test]
fn the_custom_command_entry_opens_the_fleet_preflight() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    open_palette(&mut app);

    app.type_text("label:Search actions", "Custom Command")
        .unwrap();
    app.press("text:Custom Command in All Repositories")
        .unwrap();
    app.wait_for("id:bulk:preflight").unwrap();
    assert!(shows_part(&app, "Custom command"));
    assert!(shows_part(&app, "Command"));
}

#[test]
fn the_route_lists_every_action_and_runs_it() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.press("id:toolbar:n6-palette").unwrap();
    app.wait_for("id:route:palette").unwrap();

    for slug in ["settings", "branches", "go-to-log", "custom-all"] {
        assert!(
            app.screen()
                .matches(&format!("id:palette:action:{slug}"))
                .unwrap(),
            "the {slug} entry paints"
        );
    }
    app.press("id:palette:action:settings").unwrap();
    app.wait_for("id:dialog:settings").unwrap();
}
