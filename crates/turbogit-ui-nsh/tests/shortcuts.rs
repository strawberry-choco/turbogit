//! The shortcuts window, ported from the egui `shortcuts_window` and the
//! `r4_polish` key cases: the table is the keymap, and every listed key is
//! a bound key that moves the state it names.

use nsh_core::keymap::Keystroke;
use nsh_core::platform::Os;
use support::{Mounted, Project};

mod support;

use turbogit_ui_nsh::screens::shortcuts::{Binding, KEYMAP};

fn shell(project: &Project) -> Mounted {
    let repo = support::seeded_repo(project.path(), "alpha");
    support::for_roots(project.path(), std::slice::from_ref(&repo))
}

#[test]
fn every_listed_key_parses_and_has_an_action() {
    assert!(!KEYMAP.is_empty());
    for Binding {
        action,
        display,
        keystroke,
    } in KEYMAP
    {
        assert!(!action.is_empty(), "the action names what the key does");
        assert!(!display.is_empty(), "the table shows a key");
        assert!(
            Keystroke::parse(keystroke, Os::MacOs).is_ok(),
            "{keystroke} is a keystroke"
        );
    }
}

#[test]
fn the_window_lists_exactly_the_keymap() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.press("id:toolbar:shortcuts").unwrap();
    app.wait_for("id:dialog:shortcuts").unwrap();
    for Binding {
        action, display, ..
    } in KEYMAP
    {
        assert!(app.screen().shows(action), "the {action} row paints");
        assert!(app.screen().shows(display), "the {display} key paints");
    }
    app.press("id:dialog:shortcuts:cancel").unwrap();
    app.wait_for_gone("id:dialog:shortcuts", std::time::Duration::from_secs(5))
        .unwrap();
}

#[test]
fn three_listed_keys_move_their_state() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();
    app.press_key("ctrl+k").unwrap();
    app.wait_for("id:route:changes").unwrap();
    assert!(app.screen().selected("id:tab:Commit").unwrap());

    app.press_key("ctrl+shift+a").unwrap();
    app.wait_for("id:dialog:command-palette").unwrap();
    assert!(app.screen().shows("Find Action"));

    app.press_key("ctrl+t").unwrap();
    app.wait_for("id:dialog:command-palette").unwrap();
    assert!(
        app.screen().selected("id:tab:Commit").unwrap(),
        "refresh leaves the shell where it was"
    );
}

#[test]
fn an_unmodified_key_triggers_no_shortcut() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();
    app.press("id:tab:Log").unwrap();
    app.wait_for("text:Commits").unwrap();

    for key in ["k", "t", "a"] {
        app.press_key(key).unwrap();
    }
    assert!(app.screen().selected("id:tab:Log").unwrap());
    assert!(!app.screen().shows("Find Action"));
    assert!(!app.screen().shows("Push Commits"));
}

#[test]
fn the_route_paints_the_same_table() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.press("id:toolbar:n6-shortcuts").unwrap();
    app.wait_for("id:route:shortcuts").unwrap();
    assert!(app.screen().shows("Shortcuts"));
    for Binding { display, .. } in KEYMAP {
        assert!(app.screen().shows(display), "the {display} key paints");
    }
}
