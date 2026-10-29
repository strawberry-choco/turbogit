use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::harness::{
    assert_not_painted, assert_painted, repo_shell_harness, settle, shell_harness,
};
use turbogit_app::persistence;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{IncomingCheckInterval, VcsSettings};

fn open_settings(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.command_palette = true;
    settle(harness);
    harness.get_by_label("Settings…").click();
    settle(harness);
    assert!(
        harness.state().ui.settings_open,
        "the palette's Settings… action must open the Settings modal"
    );
}

fn toggle_restore_workspace(harness: &mut Harness<'_, AppState>) {
    harness
        .get_by_label("Restore workspace context on branch switch")
        .click();
    settle(harness);
}

#[test]
fn settings_opens_from_the_command_palette() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    assert_not_painted(&harness, "Settings");
    assert!(
        harness.query_by_label("Settings").is_none(),
        "no widget may be labeled 'Settings' before the modal opens"
    );

    open_settings(&mut harness);

    assert_painted(&harness, "Settings");
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
        assert_painted(&harness, label);
    }
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert!(!harness.state().ui.settings_open);
    assert_not_painted(&harness, "General");
}

#[test]
fn categories_switch_the_visible_page() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    assert!(harness.state().ui.settings_category.is_general());
    assert_painted(&harness, "Restore workspace context on branch switch");

    harness.get_by_label("Protected Branches").click();
    settle(&mut harness);
    assert_painted(&harness, "Protected branch patterns");
    assert_not_painted(&harness, "Restore workspace context on branch switch");

    harness.get_by_label("Git").click();
    settle(&mut harness);
    assert_not_painted(&harness, "Protected branch patterns");

    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    open_settings(&mut harness);
    assert!(
        harness.state().ui.settings_category.is_general(),
        "the modal always reopens on General"
    );
}

#[test]
fn settings_modal_is_about_768px_wide() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    let win = harness.get_by_role_and_label(Role::Window, "Settings");
    let width = win.rect().width();
    assert!(
        (width - 768.0).abs() <= 8.0,
        "settings modal width {width} deviates from ~768px (spec §8.8)"
    );
}

#[test]
fn tab_strip_offers_no_settings_tab() {
    let (mut harness, _tmp) = repo_shell_harness();
    settle(&mut harness);

    for label in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        assert_painted(&harness, label);
    }
    assert_not_painted(&harness, "History");
    assert_not_painted(&harness, "Settings");
    assert_eq!(
        harness.state().ui.tab,
        Tab::Commit,
        "the active tool window never becomes Settings anymore"
    );
}

#[test]
fn backed_rows_round_trip_to_persisted_settings() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    toggle_restore_workspace(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);
    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable");
        edit.focus();
        edit.type_text("git-under-test");
    }
    settle(&mut harness);

    harness.get_by_label("Apply").click();
    settle(&mut harness);
    assert!(
        harness.state().settings.restore_workspace,
        "Apply must copy the draft into the live settings"
    );
    assert_eq!(harness.state().settings.git_executable, "git-under-test");
    let on_disk = persistence::load_settings(project.path()).expect("state.ron readable");
    assert!(on_disk.restore_workspace);
    assert_eq!(on_disk.git_executable, "git-under-test");

    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    open_settings(&mut harness);
    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert!(draft.restore_workspace);
    assert_eq!(draft.git_executable, "git-under-test");
}

#[test]
fn backed_rows_paint_their_loaded_values() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    assert_painted(&harness, "Restore workspace context on branch switch");

    harness.get_by_label("Multi-Root").click();
    settle(&mut harness);
    assert_painted(&harness, "Sync branch operations across roots");

    harness.get_by_label("Update Method").click();
    settle(&mut harness);
    assert_painted(&harness, "Merge");
    assert_painted(&harness, "Stash");

    harness.get_by_label("Appearance").click();
    settle(&mut harness);
    assert_painted(&harness, "Relative");
    assert_painted(&harness, "Highlight modified lines in the gutter");

    harness.get_by_label("Advanced").click();
    settle(&mut harness);
    assert_painted(&harness, "Warn before committing CRLF");
}

#[test]
fn the_git_page_offers_no_engine_choice() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);

    assert_not_painted(&harness, "Git backend");
    assert_not_painted(&harness, "CLI");
    assert_not_painted(&harness, "In-process reads");
    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default(),
        "opening the page wrote nothing"
    );
}

#[test]
fn git_executable_row_shows_a_live_version_badge_and_browse() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);

    assert_painted(&harness, "Browse");
    let badge = harness
        .state()
        .ui
        .git_version_badge
        .clone()
        .expect("opening the Git page must run the version check");
    let version = badge.expect("git on PATH must resolve");
    assert!(
        version.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "badge must carry the numeric version, got {version:?}"
    );
    assert_painted(&harness, "✓");

    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable");
        edit.focus();
        edit.type_text("/nonexistent/turbogit-missing-git");
    }
    settle(&mut harness);
    let badge = harness.state().ui.git_version_badge.clone().unwrap();
    assert!(
        badge.is_err(),
        "missing binary must surface as an error badge"
    );
    assert_painted(&harness, "✗");
}

#[test]
fn protected_patterns_are_removable_chips_and_apply_gates_immediately() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);
    harness.get_by_label("Protected Branches").click();
    settle(&mut harness);

    assert_painted(&harness, "main");
    assert_painted(&harness, "master");

    harness.get_by_label("Remove master").click();
    settle(&mut harness);
    assert_eq!(
        harness
            .state()
            .ui
            .settings_draft
            .as_ref()
            .unwrap()
            .protected_branch_patterns,
        vec!["main".to_string()],
        "removing a chip edits the draft immediately"
    );

    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "New pattern");
        edit.focus();
        edit.type_text("release/*");
    }
    settle(&mut harness);
    harness.get_by_label("+ Add pattern").click();
    settle(&mut harness);
    assert_eq!(
        harness
            .state()
            .ui
            .settings_draft
            .as_ref()
            .unwrap()
            .protected_branch_patterns,
        vec!["main".to_string(), "release/*".to_string()]
    );
    let new_pattern = &harness.state().ui.settings_new_pattern;
    assert!(new_pattern.is_empty(), "adding clears the input");

    harness.get_by_label("Apply").click();
    settle(&mut harness);
    let settings = harness.state().settings.clone();
    assert_eq!(
        settings.protected_branch_patterns,
        vec!["main".to_string(), "release/*".to_string()]
    );
    assert_eq!(
        persistence::load_settings(project.path())
            .unwrap()
            .protected_branch_patterns,
        vec!["main".to_string(), "release/*".to_string()]
    );
    assert!(
        !turbogit_services::sync_service::is_protected(&settings, "master"),
        "the removed pattern must stop gating right after Apply"
    );
    assert!(turbogit_services::sync_service::is_protected(
        &settings,
        "release/42"
    ));
    assert!(turbogit_services::sync_service::is_protected(
        &settings, "main"
    ));
}

#[test]
fn apply_is_disabled_until_dirty() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    let apply_disabled = |h: &Harness<'_, AppState>| {
        h.get_by_role_and_label(Role::Button, "Apply")
            .accesskit_node()
            .is_disabled()
    };
    assert!(
        apply_disabled(&harness),
        "Apply must start disabled (nothing edited yet)"
    );

    toggle_restore_workspace(&mut harness);
    assert!(!apply_disabled(&harness), "Apply must enable once dirty");

    harness.get_by_label("Restore defaults").click();
    settle(&mut harness);
    assert!(
        apply_disabled(&harness),
        "Restore defaults must re-disable Apply when the category is back at its defaults"
    );

    toggle_restore_workspace(&mut harness);
    harness.get_by_label("Apply").click();
    settle(&mut harness);
    assert!(
        harness.state().ui.settings_open,
        "Apply keeps the modal open"
    );
    assert!(
        apply_disabled(&harness),
        "after Apply the draft matches the saved settings again"
    );
}

#[test]
fn restore_defaults_resets_only_the_visible_category() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    toggle_restore_workspace(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);
    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable");
        edit.focus();
        edit.type_text("should-not-survive");
    }
    settle(&mut harness);

    harness.get_by_label("Restore defaults").click();
    settle(&mut harness);
    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert_eq!(draft.git_executable, "", "Git fields return to defaults");
    assert!(
        draft.restore_workspace,
        "edits on other categories must survive the reset"
    );

    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default(),
        "Restore defaults must not persist by itself"
    );
}

#[test]
fn cancel_discards_edits() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    toggle_restore_workspace(&mut harness);
    harness.get_by_label("Cancel").click();
    settle(&mut harness);

    assert!(!harness.state().ui.settings_open);
    assert!(
        harness.state().ui.settings_draft.is_none(),
        "Cancel must drop the draft"
    );
    assert!(!harness.state().settings.restore_workspace);
    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default(),
        "Cancel must never touch disk"
    );

    open_settings(&mut harness);
    toggle_restore_workspace(&mut harness);
    harness.get_by_label("Close window").click();
    settle(&mut harness);
    assert!(!harness.state().settings.restore_workspace);
    assert!(harness.state().ui.settings_draft.is_none());
}

#[test]
fn unbacked_rows_render_disabled_and_never_persist() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    harness.get_by_label("Git").click();
    settle(&mut harness);
    let node = harness.get_by_label("Manage Remotes");
    assert!(
        node.accesskit_node().is_disabled(),
        "`Manage Remotes` is unbacked and must render disabled"
    );
    node.click();

    harness.get_by_label("Advanced").click();
    settle(&mut harness);
    assert_not_painted(&harness, "Run git commit hooks");
    for label in [
        "Convert to LF on commit",
        "Convert to CRLF on checkout",
        "No conversion",
        "Sign-off commits",
    ] {
        let node = harness.get_by_label(label);
        assert!(
            node.accesskit_node().is_disabled(),
            "`{label}` is unbacked and must render disabled"
        );
        node.click();
    }
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.settings_draft.as_ref(),
        Some(&VcsSettings::default()),
        "unbacked rows must never mutate the draft"
    );
    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default()
    );
}

#[test]
fn keyboard_navigation_never_lands_on_a_settings_tab() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    use egui::{Key, Modifiers};
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);

    {
        let query = harness
            .get_all_by_role(Role::TextInput)
            .find(|n| n.accesskit_node().label().is_none())
            .expect("palette query field queryable");
        query.focus();
        query.type_text("sett");
    }
    settle(&mut harness);
    harness.get_by_label("Settings…").click();
    settle(&mut harness);

    assert!(
        harness.state().ui.settings_open,
        "palette entry opens the modal"
    );
    assert_eq!(
        harness.state().ui.tab,
        Tab::Commit,
        "opening Settings must never change the active tool window"
    );
    assert_painted(&harness, "General");
}

fn open_update_method(harness: &mut Harness<'_, AppState>) {
    open_settings(harness);
    harness.get_by_label("Update Method").click();
    settle(harness);
}

#[test]
fn incoming_check_is_a_toggle_with_an_interval_selector() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_update_method(&mut harness);

    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert!(!draft.incoming_poll, "incoming check ships disabled");
    assert_eq!(
        draft.incoming_interval,
        IncomingCheckInterval::Min60,
        "the interval selector defaults to Every hour"
    );
    assert!(
        harness
            .query_by_label("Check for incoming commits")
            .is_some(),
        "the toggle must be queryable by its own label"
    );
    assert_painted(&harness, "Every hour");
}

#[test]
fn incoming_check_edits_persist_on_apply() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_update_method(&mut harness);

    harness.get_by_label("Check for incoming commits").click();
    settle(&mut harness);
    harness.get_by_value("Every hour").click();
    settle(&mut harness);
    harness.get_by_label("Every 30 minutes").click();
    settle(&mut harness);

    harness.get_by_label("Apply").click();
    settle(&mut harness);

    let saved = harness.state().settings.clone();
    assert!(saved.incoming_poll, "Apply turns the toggle on");
    assert_eq!(
        saved.incoming_interval,
        IncomingCheckInterval::Min30,
        "Apply persists the chosen interval"
    );
    let on_disk = persistence::load_settings(project.path()).expect("state.ron readable");
    assert_eq!(on_disk, saved, "the persisted settings match the live ones");
}
