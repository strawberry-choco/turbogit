use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use test_support::harness::{assert_painted, repo_shell_harness, settle_reads};
use turbogit_app::state::Tab;

#[test]
fn the_settings_button_opens_the_settings_modal() {
    let (mut harness, _project) = repo_shell_harness();
    settle_reads(&mut harness);
    assert!(!harness.state().ui.settings_open);

    harness.get_by_label("Open settings").click();
    settle_reads(&mut harness);

    assert!(harness.state().ui.settings_open);
    assert_painted(&harness, "Settings");
    assert_painted(&harness, "Restore defaults");
    assert_painted(&harness, "Cancel");
}

#[test]
fn the_shortcuts_button_opens_the_shortcuts_window() {
    let (mut harness, _project) = repo_shell_harness();
    settle_reads(&mut harness);
    assert!(!harness.state().ui.shortcuts_open);

    harness.get_by_label("Show shortcuts").click();
    settle_reads(&mut harness);

    assert!(harness.state().ui.shortcuts_open);
    assert_painted(&harness, "Shortcuts");
    assert_painted(&harness, "Ctrl+Shift+K");
    assert_painted(&harness, "Alt+`");
    assert_painted(&harness, "command palette");
}

#[test]
fn frozen_shortcuts_do_not_dispatch_while_the_shortcuts_window_is_open() {
    let (mut harness, _project) = repo_shell_harness();
    settle_reads(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle_reads(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Commit);

    harness.get_by_label("Log").click();
    settle_reads(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Log);

    harness.get_by_label("Show shortcuts").click();
    settle_reads(&mut harness);
    assert!(harness.state().ui.shortcuts_open);

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle_reads(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Log);
    assert!(harness.state().ui.shortcuts_open);
}
