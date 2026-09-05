//! Issue #16 — Settings modal: a category-list dialog opened ONLY from the
//! command palette's `Settings…` action (the topbar's More button), not
//! from any tab strip or toolbar surface (spec §8.8, §9.1 correction; the
//! toolbar gear itself retired with the IDE chrome in issue #03).
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] end-to-end.
//! Asserts only on public surfaces:
//!
//! - **Painted output** — text galleys / filled rects from `FullOutput`.
//! - **Accessibility tree** — widget roles, labels, and the disabled flag.
//! - **State transitions** — public `AppState` fields + persisted settings.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::harness::{assert_not_painted, assert_painted, settle, shell_harness};
use turbogit_app::persistence;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{GitBackend, IncomingCheckInterval, VcsSettings};

/// The modal opens from the command palette (issue #03 retired the toolbar
/// gear): the topbar's More button raises the palette whose `Settings…`
/// action turns the flag on.
fn open_settings(harness: &mut Harness<'_, AppState>) {
    harness.get_by_label("More").click();
    settle(harness);
    harness.get_by_label("Settings…").click();
    settle(harness);
    assert!(
        harness.state().ui.settings_open,
        "the palette's Settings… action must open the Settings modal"
    );
}

fn toggle_staging(harness: &mut Harness<'_, AppState>) {
    harness
        .get_by_label("Use staging area instead of classic commit")
        .click();
    settle(harness);
}

// --- Cycle 1: palette-only entry ----------------------------------------------

#[test]
fn settings_is_a_modal_opened_only_from_the_command_palette() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // No Settings surface exists before the palette action runs: the tab
    // strip offers no Settings page (issue #16) and the chrome carries no
    // gear anymore (issue #03).
    assert_not_painted(&harness, "Settings");
    assert!(
        harness.query_by_label("Settings").is_none(),
        "no widget may be labeled 'Settings' before the modal opens"
    );

    open_settings(&mut harness);

    // The modal chrome is really painted: title, category list, footer.
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
    // …and closing the modal removes every trace again.
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

    // The modal opens on General…
    assert!(harness.state().ui.settings_category.is_general());
    assert_painted(&harness, "Use staging area instead of classic commit");

    // …and clicking a category swaps the visible page.
    harness.get_by_label("Protected Branches").click();
    settle(&mut harness);
    assert_painted(&harness, "Protected branch patterns");
    assert_not_painted(&harness, "Use staging area instead of classic commit");

    harness.get_by_label("Git").click();
    settle(&mut harness);
    assert_painted(&harness, "Git backend");
    assert_not_painted(&harness, "Protected branch patterns");

    // The selection is modal-ephemeral state on UiState, not persisted.
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
fn tab_strip_no_longer_offers_settings() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // The strip renders Commit + Log only; History was already deleted in
    // issue #19 and Settings in issue #16. (Painted-text checks: "Commit"
    // also labels the toolbar button, so label queries stay unambiguous.)
    for label in ["Commit", "Log"] {
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

// --- Cycle 2: backed rows round-trip to persisted settings --------------------

#[test]
fn backed_rows_round_trip_to_persisted_settings() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    // Edit two backed rows: staging mode checkbox (General) + git executable
    // input (Git page).
    toggle_staging(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);
    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable");
        edit.focus();
        edit.type_text("git-under-test");
    }
    settle(&mut harness);

    // Apply persists into live state AND onto disk (.turbogit/state.ron).
    harness.get_by_label("Apply").click();
    settle(&mut harness);
    assert!(
        harness.state().settings.staging_area,
        "Apply must copy the draft into the live settings"
    );
    assert_eq!(harness.state().settings.git_executable, "git-under-test");
    let on_disk = persistence::load_settings(project.path()).expect("state.ron readable");
    assert!(on_disk.staging_area);
    assert_eq!(on_disk.git_executable, "git-under-test");

    // Cancel closes; reopening shows the persisted values, not defaults.
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    open_settings(&mut harness);
    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert!(draft.staging_area);
    assert_eq!(draft.git_executable, "git-under-test");
}

#[test]
fn backed_rows_paint_their_loaded_values() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    // General: the landing page carries the commit behavior rows.
    assert_painted(&harness, "Use staging area instead of classic commit");
    assert_painted(&harness, "Restore workspace context on branch switch");

    // Multi-Root carries the cross-root branch sync.
    harness.get_by_label("Multi-Root").click();
    settle(&mut harness);
    assert_painted(&harness, "Sync branch operations across roots");

    // Update Method carries the integration policy dropdowns.
    harness.get_by_label("Update Method").click();
    settle(&mut harness);
    assert_painted(&harness, "Merge");
    assert_painted(&harness, "Stash");

    // Appearance carries the log presentation.
    harness.get_by_label("Appearance").click();
    settle(&mut harness);
    assert_painted(&harness, "Relative");
    assert_painted(&harness, "Highlight modified lines in the gutter");

    // Advanced carries the warning/commit-policy rows.
    harness.get_by_label("Advanced").click();
    settle(&mut harness);
    assert_painted(&harness, "Warn before committing CRLF");
}

// --- Cycle 2b: the Git backend selector (issue #26, screen 11) -----------------

#[test]
fn git_backend_is_a_segmented_cli_libgit2_auto_choice_and_applies() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);

    // Screen 11: a segmented control with the three strategies painted.
    for label in ["CLI", "libgit2", "Auto"] {
        assert_painted(&harness, label);
    }

    // Default settings ship Auto…
    assert_eq!(
        harness.state().ui.settings_draft.as_ref().unwrap().backend,
        GitBackend::Auto
    );

    // …and picking CLI then Apply persists the explicit choice.
    harness.get_by_label("CLI").click();
    settle(&mut harness);
    harness.get_by_label("Apply").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings.backend, GitBackend::Cli);
    let on_disk = persistence::load_settings(project.path()).expect("state.ron readable");
    assert_eq!(on_disk.backend, GitBackend::Cli);

    // Switching back to Auto round-trips too (the engine factory rebuilds on
    // Apply behind the seam — covered at the engine level).
    harness.get_by_label("Auto").click();
    settle(&mut harness);
    harness.get_by_label("Apply").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings.backend, GitBackend::Auto);
    assert_eq!(
        persistence::load_settings(project.path()).unwrap().backend,
        GitBackend::Auto
    );
}

// --- Cycle 2c: git-executable version badge + Browse (issue #26) ---------------

#[test]
fn git_executable_row_shows_a_live_version_badge_and_browse() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);

    // The Browse picker action is present…
    assert_painted(&harness, "Browse");
    // …and the live check resolved the git on PATH into a success badge.
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

    // Pointing the executable at a missing binary flips the badge to the
    // error state live, from the draft path (not the saved settings).
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

// --- Cycle 2d: protected-branch pattern chips (issue #26, screen 11) -----------

#[test]
fn protected_patterns_are_removable_chips_and_apply_gates_immediately() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);
    harness.get_by_label("Protected Branches").click();
    settle(&mut harness);

    // The default patterns render as chips…
    assert_painted(&harness, "main");
    assert_painted(&harness, "master");

    // …and each chip carries a remove control.
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

    // "+ Add pattern" appends the new-pattern input's content.
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

    // Apply persists the edited set, and the protective gate consumes it
    // immediately (sync_service reads the live settings).
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

// --- Cycle 3: dirty gating + Reset/Cancel semantics ---------------------------

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

    // One edit → dirty → Apply enables.
    toggle_staging(&mut harness);
    assert!(!apply_disabled(&harness), "Apply must enable once dirty");

    // Restore defaults resets the visible category (General) → clean again.
    harness.get_by_label("Restore defaults").click();
    settle(&mut harness);
    assert!(
        apply_disabled(&harness),
        "Restore defaults must re-disable Apply when the category is back at its defaults"
    );

    // Apply on a dirty draft succeeds and cleans the flag, keeping the modal
    // open (IDE semantics — users tweak several pages before closing).
    toggle_staging(&mut harness);
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

    // Edit a General field, then move to Git and edit two Git fields.
    toggle_staging(&mut harness);
    harness.get_by_label("Git").click();
    settle(&mut harness);
    harness.get_by_label("CLI").click();
    settle(&mut harness);
    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable");
        edit.focus();
        edit.type_text("should-not-survive");
    }
    settle(&mut harness);

    // Restore defaults resets the visible category only.
    harness.get_by_label("Restore defaults").click();
    settle(&mut harness);
    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert_eq!(draft.git_executable, "", "Git fields return to defaults");
    assert_eq!(
        draft.backend,
        GitBackend::Auto,
        "Git fields return to defaults"
    );
    assert!(
        draft.staging_area,
        "edits on other categories must survive the reset"
    );

    // Nothing persisted yet.
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

    toggle_staging(&mut harness);
    harness.get_by_label("Cancel").click();
    settle(&mut harness);

    assert!(!harness.state().ui.settings_open);
    assert!(
        harness.state().ui.settings_draft.is_none(),
        "Cancel must drop the draft"
    );
    assert!(!harness.state().settings.staging_area);
    assert_eq!(
        persistence::load_settings(project.path()).unwrap(),
        VcsSettings::default(),
        "Cancel must never touch disk"
    );

    // The window close (X) button follows the same discard semantics.
    open_settings(&mut harness);
    toggle_staging(&mut harness);
    harness.get_by_label("Close window").click();
    settle(&mut harness);
    assert!(!harness.state().settings.staging_area);
    assert!(harness.state().ui.settings_draft.is_none());
}

// --- Cycle 4: unbacked rows are visible but disabled --------------------------

#[test]
fn unbacked_rows_render_disabled_and_never_persist() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    // Manage Remotes lives on the Git page.
    harness.get_by_label("Git").click();
    settle(&mut harness);
    let node = harness.get_by_label("Manage Remotes");
    assert!(
        node.accesskit_node().is_disabled(),
        "`Manage Remotes` is unbacked and must render disabled"
    );
    node.click();

    // The CRLF-conversion radios and commit checks live on Advanced.
    harness.get_by_label("Advanced").click();
    settle(&mut harness);
    for label in [
        "Convert to LF on commit",
        "Convert to CRLF on checkout",
        "No conversion",
        "Run git commit hooks",
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

    // Clicking disabled controls changed nothing: still clean, still default.
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

// --- Cycle 5: the deleted tab cannot be reached by state either ---------------

#[test]
fn keyboard_navigation_never_lands_on_a_settings_tab() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // Ctrl+Shift+A opens the palette; its action routes to the same modal —
    // no tab switch exists at all anymore (Tab::Settings was deleted).
    use egui::{Key, Modifiers};
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);

    // Narrow the action list first (the query field auto-focuses), exactly
    // like a keyboard user would — the full list scrolls. The Commit
    // window's file filter (spec R7) also matches Role::TextInput but
    // carries an accessible label ("Filter files"); the palette query is
    // unlabeled.
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

// --- Incoming check: toggle + interval selector (issue #27, screen 11) --------

/// Open the modal straight onto the Update Method page.
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

    // Screen 11: the Incoming check row is a checkbox plus an interval
    // selector, off by default and defaulting to every 15 minutes.
    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert!(!draft.incoming_poll, "incoming check ships disabled");
    assert_eq!(
        draft.incoming_interval,
        IncomingCheckInterval::Min15,
        "the interval selector defaults to Every 15 minutes"
    );
    assert!(
        harness
            .query_by_label("Check for incoming commits")
            .is_some(),
        "the toggle must be queryable by its own label"
    );
    assert_painted(&harness, "Every 15 minutes");
}

#[test]
fn incoming_check_edits_persist_on_apply() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_update_method(&mut harness);

    // Enable the check and widen the interval, then Apply.
    harness.get_by_label("Check for incoming commits").click();
    settle(&mut harness);
    // The interval combo is identified by its current value, not a label.
    harness.get_by_value("Every 15 minutes").click();
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
