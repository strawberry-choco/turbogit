//! Issue #16 — Settings modal: a category-list dialog opened ONLY from the
//! toolbar gear, replacing the deleted tab-strip Settings page (spec §8.8,
//! §9.1 correction).
//!
//! Headless egui_kittest harness driving [`turbogit::ui::render`] end-to-end.
//! Asserts only on public surfaces:
//!
//! - **Painted output** — text galleys / filled rects from `FullOutput`.
//! - **Accessibility tree** — widget roles, labels, and the disabled flag.
//! - **State transitions** — public `AppState` fields + persisted settings.

mod common;

use common::{assert_not_painted, assert_painted, settle, shell_harness};
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use turbogit::model::VcsSettings;
use turbogit::persistence;
use turbogit::state::{AppState, Tab};

/// The toolbar gear's accessible label: `icon_button` exposes the Lucide
/// icon name (`"settings"`, lowercase) — distinct from the old tab label.
const GEAR: &str = "settings";

fn open_settings(harness: &mut Harness<'_, AppState>) {
    harness.get_by_label(GEAR).click();
    settle(harness);
    assert!(
        harness.state().ui.settings_open,
        "clicking the gear must open the Settings modal"
    );
}

fn toggle_staging(harness: &mut Harness<'_, AppState>) {
    harness
        .get_by_label("Use staging area instead of classic commit")
        .click();
    settle(harness);
}

// --- Cycle 1: gear-only entry -------------------------------------------------

#[test]
fn settings_is_a_modal_opened_only_from_the_gear() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    // No Settings surface exists before the gear is clicked: the tab strip
    // no longer offers it (issue #16) and the gear paints no text.
    assert_not_painted(&harness, "Settings");
    assert!(
        harness.query_by_label("Settings").is_none(),
        "no widget may be labeled 'Settings' before the modal opens"
    );

    open_settings(&mut harness);

    // The modal chrome is really painted: title, category list, footer.
    assert_painted(&harness, "Settings");
    for label in [
        "Version Control",
        "Notifications",
        "Keymap",
        "Reset",
        "Cancel",
        "Apply",
    ] {
        assert_painted(&harness, label);
    }
    // …and closing the modal removes every trace again.
    harness.get_by_label("Cancel").click();
    settle(&mut harness);
    assert!(!harness.state().ui.settings_open);
    assert_not_painted(&harness, "Version Control");
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

    // Edit two backed rows: staging mode checkbox + git executable input.
    toggle_staging(&mut harness);
    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable path");
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

    // Dropdowns paint their current selection from the loaded settings…
    assert_painted(&harness, "Merge");
    assert_painted(&harness, "Stash");
    assert_painted(&harness, "Relative");
    // …and every backed row of the Version Control page is present.
    for label in [
        "Git executable path",
        "Use staging area instead of classic commit",
        "Sync branch operations across roots",
        "Update method:",
        "Clean-tree method:",
        "Protected branches patterns",
        "Warn before committing CRLF",
        "Date format:",
    ] {
        assert_painted(&harness, label);
    }
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

    // Reset restores the loaded values → clean again.
    harness.get_by_label("Reset").click();
    settle(&mut harness);
    assert!(
        apply_disabled(&harness),
        "Reset must restore loaded values and re-disable Apply"
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
fn reset_restores_loaded_values_without_persisting() {
    let (mut harness, project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    toggle_staging(&mut harness);
    {
        let edit = harness.get_by_role_and_label(Role::TextInput, "Git executable path");
        edit.focus();
        edit.type_text("should-not-survive");
    }
    settle(&mut harness);

    harness.get_by_label("Reset").click();
    settle(&mut harness);

    let draft = harness.state().ui.settings_draft.as_ref().unwrap();
    assert!(!draft.staging_area, "Reset must restore the loaded value");
    assert_eq!(draft.git_executable, "");
    let on_disk = persistence::load_settings(project.path()).expect("state.ron readable");
    assert_eq!(on_disk, VcsSettings::default(), "Reset must not persist");
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

    for label in [
        "Manage Remotes",
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

#[test]
fn notifications_and_keymap_categories_are_placeholders() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    open_settings(&mut harness);

    for label in ["Notifications", "Keymap"] {
        let node = harness.get_by_label(label);
        assert!(
            node.accesskit_node().is_disabled(),
            "{label} has no backing settings yet and must be disabled"
        );
        node.click();
    }
    settle(&mut harness);

    // The Version Control page stays selected and fully rendered.
    assert_painted(&harness, "Use staging area instead of classic commit");
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
    // like a keyboard user would — the full list scrolls.
    {
        let query = harness
            .get_all_by_role(Role::TextInput)
            .next()
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
    assert_painted(&harness, "Version Control");
}
